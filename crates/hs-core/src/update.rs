//! Release discovery and verification (docs/09 §5). Phase 0 ships the pure
//! logic: parsing the forge's releases API, choosing a candidate, parsing and
//! verifying the signed manifest, and checking a download against it. The
//! Settings → Updates UI and the install sequence arrive in M1.

use serde::Deserialize;

use crate::version::VersionInfo;

/// Public keys accepted for `manifest.json.minisig` (keys/minisign.pub).
pub const SIGNING_KEYS: &[&str] = &["RWSrHknGl6PCmbQLELU5/Y7cxjPzb/fgHUSZJPW4Jxpv03/dAiVCHcAM"];

pub const RELEASES_URL: &str =
    "https://git.zacx.net/api/v1/repos/owner/hyper-stack/releases?limit=20";

pub const EXE_ASSET: &str = "HyperStack.exe";
pub const MANIFEST_ASSET: &str = "manifest.json";
pub const SIGNATURE_ASSET: &str = "manifest.json.minisig";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Channel {
    Stable,
    Beta,
}

impl Channel {
    pub fn from_setting(s: &str) -> Self {
        if s.eq_ignore_ascii_case("beta") {
            Self::Beta
        } else {
            Self::Stable
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ApiRelease {
    pub tag_name: String,
    #[serde(default)]
    pub draft: bool,
    #[serde(default)]
    pub prerelease: bool,
    #[serde(default)]
    pub html_url: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub assets: Vec<ApiAsset>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ApiAsset {
    pub name: String,
    #[serde(default)]
    pub browser_download_url: String,
    #[serde(default)]
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub version: semver::Version,
    pub tag: String,
    pub html_url: String,
    pub notes: String,
    pub manifest_url: String,
    pub signature_url: String,
}

/// Parse a tag like `v1.4.0` or `v1.5.0-beta.2`.
pub fn parse_tag(tag: &str) -> Option<semver::Version> {
    semver::Version::parse(tag.strip_prefix('v').unwrap_or(tag)).ok()
}

/// The newest installable release newer than `current` on `channel`.
/// Releases without a manifest and signature are skipped (never "up to date").
pub fn newer_than(
    json: &str,
    current: &semver::Version,
    channel: Channel,
) -> Result<Option<Candidate>, String> {
    let releases: Vec<ApiRelease> = serde_json::from_str(json).map_err(|e| e.to_string())?;
    let mut best: Option<Candidate> = None;
    for r in releases.iter().filter(|r| !r.draft) {
        if r.prerelease && channel == Channel::Stable {
            continue;
        }
        let Some(version) = parse_tag(&r.tag_name) else {
            continue;
        };
        if version <= *current {
            continue;
        }
        let asset = |name: &str| {
            r.assets
                .iter()
                .find(|a| a.name == name && !a.browser_download_url.is_empty())
        };
        let (Some(m), Some(s)) = (asset(MANIFEST_ASSET), asset(SIGNATURE_ASSET)) else {
            continue;
        };
        let c = Candidate {
            version,
            tag: r.tag_name.clone(),
            html_url: r.html_url.clone(),
            notes: r.body.clone(),
            manifest_url: m.browser_download_url.clone(),
            signature_url: s.browser_download_url.clone(),
        };
        if best.as_ref().is_none_or(|b| c.version > b.version) {
            best = Some(c);
        }
    }
    Ok(best)
}

/// `manifest.json` as written by scripts/build-and-package.ps1.
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct Manifest {
    pub schema: u32,
    pub product: String,
    pub version: String,
    pub channel: String,
    pub published_at: String,
    pub tag: String,
    #[serde(default)]
    pub notes_url: String,
    #[serde(default)]
    pub min_supported_version: Option<String>,
    #[serde(default)]
    pub data_schema: Option<u32>,
    pub assets: std::collections::BTreeMap<String, ManifestAsset>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct ManifestAsset {
    pub name: String,
    pub url: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum VerifyError {
    #[error("the manifest signature does not verify against any accepted key")]
    BadSignature,
    #[error("manifest is not valid: {0}")]
    Format(String),
    #[error("manifest is for {found}, expected {expected}")]
    WrongVersion { found: String, expected: String },
    #[error("downloaded {name} is {found} bytes, manifest says {expected}")]
    Size {
        name: String,
        found: u64,
        expected: u64,
    },
    #[error("downloaded {0} does not match the manifest's sha256")]
    Hash(String),
    #[error(
        "this copy ({current}) is older than the oldest version that can update in place ({min})"
    )]
    TooOld { current: String, min: String },
}

/// Verify `manifest_bytes` against `signature` with the embedded keys and parse it.
pub fn verify_manifest(
    manifest_bytes: &[u8],
    signature: &str,
    expected_tag: &str,
) -> Result<Manifest, VerifyError> {
    let sig =
        minisign_verify::Signature::decode(signature).map_err(|_| VerifyError::BadSignature)?;
    let ok = SIGNING_KEYS.iter().any(|k| {
        minisign_verify::PublicKey::from_base64(k)
            .map(|pk| pk.verify(manifest_bytes, &sig, false).is_ok())
            .unwrap_or(false)
    });
    if !ok {
        return Err(VerifyError::BadSignature);
    }
    let m: Manifest =
        serde_json::from_slice(manifest_bytes).map_err(|e| VerifyError::Format(e.to_string()))?;
    if m.tag != expected_tag {
        return Err(VerifyError::WrongVersion {
            found: m.tag,
            expected: expected_tag.to_string(),
        });
    }
    Ok(m)
}

/// Check a downloaded asset's size and sha256 against the manifest entry.
pub fn verify_asset(asset: &ManifestAsset, bytes: &[u8]) -> Result<(), VerifyError> {
    if bytes.len() as u64 != asset.size {
        return Err(VerifyError::Size {
            name: asset.name.clone(),
            found: bytes.len() as u64,
            expected: asset.size,
        });
    }
    use sha2::Digest;
    let hash = sha2::Sha256::digest(bytes);
    let hex: String = hash.iter().map(|b| format!("{b:02x}")).collect();
    if hex != asset.sha256.to_lowercase() {
        return Err(VerifyError::Hash(asset.name.clone()));
    }
    Ok(())
}

/// Can `current` update in place to this manifest's version?
pub fn check_supported(m: &Manifest, current: &VersionInfo) -> Result<(), VerifyError> {
    if let Some(min) = &m.min_supported_version
        && let Ok(min_v) = semver::Version::parse(min)
        && current.version < min_v
    {
        return Err(VerifyError::TooOld {
            current: current.version.to_string(),
            min: min.clone(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(tag: &str, draft: bool, pre: bool, with_manifest: bool) -> String {
        let assets = if with_manifest {
            format!(
                r#"[{{"name":"HyperStack.exe","browser_download_url":"https://f/{tag}/HyperStack.exe","size":1}},
                    {{"name":"manifest.json","browser_download_url":"https://f/{tag}/manifest.json","size":1}},
                    {{"name":"manifest.json.minisig","browser_download_url":"https://f/{tag}/manifest.json.minisig","size":1}}]"#
            )
        } else {
            r#"[{"name":"HyperStack.exe","browser_download_url":"https://f/x","size":1}]"#
                .to_string()
        };
        format!(
            r#"{{"tag_name":"{tag}","draft":{draft},"prerelease":{pre},"html_url":"https://f/r/{tag}","body":"notes","assets":{assets}}}"#
        )
    }

    fn v(s: &str) -> semver::Version {
        semver::Version::parse(s).unwrap()
    }

    #[test]
    fn picks_the_newest_complete_release_on_the_channel() {
        let json = format!(
            "[{},{},{},{},{}]",
            release("v1.1.0", false, false, true),
            release("v1.10.0", false, false, true),
            release("v1.2.0", false, false, true),
            release("v2.0.0", false, false, false), // no manifest → skipped
            release("v3.0.0", true, false, true),   // draft → skipped
        );
        let c = newer_than(&json, &v("1.0.0"), Channel::Stable)
            .unwrap()
            .unwrap();
        assert_eq!(c.version, v("1.10.0"));
        assert_eq!(c.manifest_url, "https://f/v1.10.0/manifest.json");
        assert!(
            newer_than(&json, &v("1.10.0"), Channel::Stable)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn prereleases_only_on_beta_channel() {
        let json = format!("[{}]", release("v1.5.0-beta.1", false, true, true));
        assert!(
            newer_than(&json, &v("1.4.0"), Channel::Stable)
                .unwrap()
                .is_none()
        );
        let c = newer_than(&json, &v("1.4.0"), Channel::Beta)
            .unwrap()
            .unwrap();
        assert_eq!(c.tag, "v1.5.0-beta.1");
        // A beta is older than the stable it precedes.
        assert!(
            newer_than(&json, &v("1.5.0"), Channel::Beta)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn asset_verification_checks_size_and_hash() {
        let bytes = b"hello world";
        let a = ManifestAsset {
            name: "HyperStack.exe".into(),
            url: String::new(),
            size: 11,
            sha256: "B94D27B9934D3E08A52E52D7DA7DABFAC484EFE37A5380EE9088F7ACE2EFCDE9".into(),
        };
        assert_eq!(verify_asset(&a, bytes), Ok(()));
        let short = ManifestAsset {
            size: 3,
            ..a.clone()
        };
        assert!(matches!(
            verify_asset(&short, bytes),
            Err(VerifyError::Size { .. })
        ));
        let wrong = ManifestAsset {
            sha256: "00".repeat(32),
            ..a
        };
        assert!(matches!(
            verify_asset(&wrong, bytes),
            Err(VerifyError::Hash(_))
        ));
    }

    #[test]
    fn bad_signature_is_rejected_before_parsing() {
        let err = verify_manifest(b"{}", "untrusted comment: x\nAAAA\n", "v1.0.0").unwrap_err();
        assert_eq!(err, VerifyError::BadSignature);
    }

    #[test]
    fn embedded_public_key_parses() {
        for k in SIGNING_KEYS {
            assert!(minisign_verify::PublicKey::from_base64(k).is_ok());
        }
    }
}
