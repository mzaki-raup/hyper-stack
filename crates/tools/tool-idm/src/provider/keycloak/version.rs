//! Version gating (docs/tools/idm.md §8.3): `GET /admin/serverinfo` gives
//! the version (to master admins) and the features; [`caps_for`] turns
//! them into the [`Caps`] the screens ask.

use serde_json::Value;

use crate::provider::{Caps, Feature, Support, Version};

/// The tested range.
pub const OLDEST: u32 = 22;
pub const NEWEST: u32 = 26;

/// `26.0.6`, `26.0.6.redhat-00001`, `25.0.0-SNAPSHOT`, `24.0`.
pub fn parse_version(s: &str) -> Option<Version> {
    let mut parts = s.trim().split(['.', '-']).map(|p| {
        let digits: String = p.chars().take_while(char::is_ascii_digit).collect();
        digits.parse::<u32>().ok()
    });
    let major = parts.next()??;
    let minor = parts.next().flatten().unwrap_or(0);
    let patch = parts.next().flatten().unwrap_or(0);
    Some(Version {
        major,
        minor,
        patch,
    })
}

pub fn support(v: Option<Version>) -> Support {
    match v {
        None => Support::Unknown,
        Some(v) if v.major < OLDEST => Support::TooOld,
        Some(v) if v.major > NEWEST => Support::Newer,
        Some(_) => Support::Supported,
    }
}

/// The features from `features` (newer servers: name and enabled), or
/// from `profileInfo` (older ones: lists of disabled, preview and
/// experimental features). The second value says whether the `features`
/// list was there - only newer servers send it.
pub fn features(info: &Value) -> (Vec<Feature>, bool) {
    if let Some(list) = info.get("features").and_then(Value::as_array) {
        let out = list
            .iter()
            .filter_map(|f| {
                Some(Feature {
                    name: f.get("name")?.as_str()?.to_string(),
                    enabled: f.get("enabled").and_then(Value::as_bool).unwrap_or(false),
                })
            })
            .collect();
        return (out, true);
    }
    let names = |k: &str| -> Vec<String> {
        info.get("profileInfo")
            .and_then(|p| p.get(k))
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(String::from)
            .collect()
    };
    let disabled = names("disabledFeatures");
    let mut out: Vec<Feature> = disabled
        .iter()
        .map(|n| Feature {
            name: n.clone(),
            enabled: false,
        })
        .collect();
    for n in names("previewFeatures")
        .into_iter()
        .chain(names("experimentalFeatures"))
    {
        if !disabled.contains(&n) {
            out.push(Feature {
                name: n,
                enabled: true,
            });
        }
    }
    (out, false)
}

fn on(features: &[Feature], name: &str) -> bool {
    features
        .iter()
        .any(|f| f.enabled && f.name.eq_ignore_ascii_case(name))
}

/// The capabilities of a server of version `v` with these features.
///
/// When the version is unknown (a realm-scoped admin is not shown it),
/// a server that sends the `features` list is a recent one and is treated
/// as 24+; one that does not is treated as 22. Either way a 404 on a newer
/// endpoint later hides that feature for the session instead of failing
/// the screen (`Keycloak::group_children` falls back to `subGroups`).
pub fn caps_for(v: Option<Version>, features: &[Feature], features_listed: bool) -> Caps {
    let at_least = |major: u32, minor: u32| match v {
        Some(v) => (v.major, v.minor) >= (major, minor),
        None => features_listed && (major, minor) <= (24, 0),
    };
    Caps {
        user_profile: at_least(24, 0) || on(features, "DECLARATIVE_USER_PROFILE"),
        // Unknown versions try /children and fall back on a 404.
        group_children: v.is_none() || at_least(23, 0),
        organizations: match v {
            Some(v) if v.major == 25 => on(features, "ORGANIZATION"),
            Some(v) if v.major >= 26 => on(features, "ORGANIZATION"),
            None => on(features, "ORGANIZATION"),
            _ => false,
        },
        fgap_v2: (at_least(26, 2) || v.is_none()) && on(features, "ADMIN_FINE_GRAINED_AUTHZ_V2"),
        writes: !matches!(support(v), Support::TooOld),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(major: u32, minor: u32, patch: u32) -> Option<Version> {
        Some(Version {
            major,
            minor,
            patch,
        })
    }

    #[test]
    fn versions_parse_including_red_hat_builds() {
        assert_eq!(parse_version("26.8.2"), v(26, 8, 2));
        assert_eq!(parse_version("26.0.6.redhat-00001"), v(26, 0, 6));
        assert_eq!(parse_version("25.0.0-SNAPSHOT"), v(25, 0, 0));
        assert_eq!(parse_version("24.0"), v(24, 0, 0));
        assert_eq!(parse_version(""), None);
        assert_eq!(parse_version("nightly"), None);
    }

    #[test]
    fn support_follows_the_tested_range() {
        assert_eq!(support(v(21, 1, 0)), Support::TooOld);
        assert_eq!(support(v(22, 0, 0)), Support::Supported);
        assert_eq!(support(v(26, 8, 2)), Support::Supported);
        assert_eq!(support(v(27, 0, 0)), Support::Newer);
        assert_eq!(support(None), Support::Unknown);
    }

    fn f(name: &str, enabled: bool) -> Feature {
        Feature {
            name: name.into(),
            enabled,
        }
    }

    /// One case per row of §8.3's table.
    #[test]
    fn caps_follow_each_version_row() {
        let none: Vec<Feature> = Vec::new();
        let c22 = caps_for(v(22, 0, 5), &none, false);
        assert!(!c22.user_profile && !c22.group_children && c22.writes);
        // 22-23 with the user-profile preview turned on count as 24+.
        let c22p = caps_for(v(22, 0, 5), &[f("DECLARATIVE_USER_PROFILE", true)], false);
        assert!(c22p.user_profile);
        let c23 = caps_for(v(23, 0, 7), &none, false);
        assert!(c23.group_children && !c23.user_profile);
        let c24 = caps_for(v(24, 0, 5), &none, true);
        assert!(c24.user_profile && !c24.organizations);
        let org = [f("ORGANIZATION", true)];
        assert!(caps_for(v(25, 0, 0), &org, true).organizations);
        assert!(!caps_for(v(25, 0, 0), &none, true).organizations);
        assert!(caps_for(v(26, 0, 0), &org, true).organizations);
        let v2 = [f("ADMIN_FINE_GRAINED_AUTHZ_V2", true)];
        assert!(!caps_for(v(26, 1, 0), &v2, true).fgap_v2);
        assert!(caps_for(v(26, 2, 0), &v2, true).fgap_v2);
        // Below the range: reads only.
        assert!(!caps_for(v(21, 0, 0), &none, false).writes);
        // Above it: everything offered.
        let c27 = caps_for(v(27, 0, 0), &none, true);
        assert!(c27.writes && c27.user_profile && c27.group_children);
    }

    #[test]
    fn an_unknown_version_is_read_from_the_features_list() {
        // A realm-scoped service account sees no systemInfo, but a recent
        // server still sends `features`.
        let recent = caps_for(None, &[f("PASSKEYS", true)], true);
        assert!(recent.user_profile && recent.group_children && recent.writes);
        let old = caps_for(None, &[], false);
        assert!(!old.user_profile);
        // /children is tried, and a 404 falls back to subGroups.
        assert!(old.group_children);
    }

    #[test]
    fn features_come_from_the_list_or_from_profile_info() {
        let new = serde_json::json!({"features": [
            {"name": "ORGANIZATION", "enabled": true},
            {"name": "DOCKER", "enabled": false}
        ]});
        let (list, listed) = features(&new);
        assert!(listed);
        assert!(on(&list, "ORGANIZATION") && !on(&list, "DOCKER"));
        let old = serde_json::json!({"profileInfo": {
            "disabledFeatures": ["DOCKER"],
            "previewFeatures": ["DECLARATIVE_USER_PROFILE"],
            "experimentalFeatures": []
        }});
        let (list, listed) = features(&old);
        assert!(!listed);
        assert!(on(&list, "DECLARATIVE_USER_PROFILE"));
        assert!(!on(&list, "DOCKER"));
    }
}
