//! The provider model (docs/tools/analytics.md §4.1): one trait the
//! screens talk to, one module per product. What a product cannot do is
//! said through [`Caps`], never by hiding a section.

use async_trait::async_trait;

pub mod metabase;

/// Errors are sentences (`hs_core::service_http`): they reach the tab as
/// they are.
pub type Result<T> = std::result::Result<T, String>;

/// The oldest Metabase this tab supports (§8.3): API keys arrived in 49,
/// dashboard tabs in 47, and the card query paths are as they are from 50.
pub const OLDEST_MINOR: u32 = 48;

/// The newest Metabase this tab has been tried against. A newer one is
/// tried all the same and its header says so.
pub const NEWEST_TESTED_MINOR: u32 = 52;

/// The first Metabase that takes an API key.
pub const API_KEYS_FROM_MINOR: u32 = 49;

/// A Metabase version tag: `v0.52.4` is the open-source build, `v1.52.4`
/// the Enterprise build; the second number is the release.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Version {
    pub edition: u32,
    pub minor: u32,
    pub patch: u32,
}

impl Version {
    /// `v0.52.4`, `1.52.4` or `v0.52` (a missing patch reads as 0); `None`
    /// for anything else, such as `vLOCAL_DEV`.
    pub fn parse(tag: &str) -> Option<Version> {
        let mut parts = tag.trim().trim_start_matches('v').split('.');
        let mut next = || parts.next()?.trim().parse::<u32>().ok();
        let edition = next()?;
        let minor = next()?;
        let patch = next().unwrap_or(0);
        Some(Version {
            edition,
            minor,
            patch,
        })
    }

    pub fn support(self) -> Support {
        if self.minor < OLDEST_MINOR {
            Support::TooOld
        } else if self.minor > NEWEST_TESTED_MINOR {
            Support::Newer
        } else {
            Support::Supported
        }
    }
}

/// Whether the server's version is one this tab has been built against.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Support {
    /// The server did not say.
    #[default]
    Unknown,
    Supported,
    TooOld,
    Newer,
}

/// What a session learned when it connected.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ServerInfo {
    pub product: String,
    /// The version as the server wrote it ("v0.52.4").
    pub version_text: String,
    pub version: Option<Version>,
    /// The account's name or email; an API key shows the key's user.
    pub signed_in_as: String,
    pub support: Support,
}

/// What a product can do, for the screens to enable or disable with a
/// reason.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Caps {
    /// Collections, questions and dashboards can be browsed.
    pub browse: bool,
    /// Ad-hoc SQL can be run through the server.
    pub native_sql: bool,
}

#[async_trait]
pub trait BiProvider: Send + Sync {
    /// Sign in and read the server's facts. Called once per session.
    async fn connect(&self) -> Result<ServerInfo>;
    fn capabilities(&self) -> Caps;
    /// The server's own web interface, for Open in Browser.
    fn web_url(&self) -> String;
    /// End the server-side session, where there is one.
    async fn disconnect(&self);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_metabase_tag_is_parsed_for_both_editions() {
        assert_eq!(
            Version::parse("v0.52.4"),
            Some(Version {
                edition: 0,
                minor: 52,
                patch: 4
            })
        );
        assert_eq!(Version::parse("v1.49.1").map(|v| v.edition), Some(1));
        assert_eq!(Version::parse("1.50.0").map(|v| v.minor), Some(50));
        assert_eq!(Version::parse("v0.52").map(|v| v.patch), Some(0));
        assert_eq!(Version::parse("vLOCAL_DEV"), None);
        assert_eq!(Version::parse(""), None);
    }

    #[test]
    fn the_version_decides_the_support_chip() {
        let at = |minor| Version {
            edition: 0,
            minor,
            patch: 0,
        };
        assert_eq!(at(OLDEST_MINOR - 1).support(), Support::TooOld);
        assert_eq!(at(OLDEST_MINOR).support(), Support::Supported);
        assert_eq!(at(NEWEST_TESTED_MINOR).support(), Support::Supported);
        assert_eq!(at(NEWEST_TESTED_MINOR + 1).support(), Support::Newer);
    }
}
