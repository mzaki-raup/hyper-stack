//! What this build is: version, git sha, build date, channel, target.

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionInfo {
    pub version: semver::Version,
    pub git_sha: &'static str,
    pub build_date: &'static str,
    pub channel: &'static str,
    pub target: &'static str,
}

impl VersionInfo {
    /// The running build. `CARGO_PKG_VERSION` is the workspace version; the
    /// rest comes from `build.rs`.
    pub fn current() -> Self {
        Self {
            version: semver::Version::parse(env!("CARGO_PKG_VERSION"))
                .expect("CARGO_PKG_VERSION is valid semver"),
            git_sha: env!("HS_GIT_SHA"),
            build_date: env!("HS_BUILD_DATE"),
            channel: env!("HS_CHANNEL"),
            target: env!("HS_TARGET"),
        }
    }

    /// `windows-x64`, `linux-x64`, `macos-arm64` — the asset key in a release manifest.
    pub fn platform_key(&self) -> String {
        let os = if self.target.contains("windows") {
            "windows"
        } else if self.target.contains("darwin") {
            "macos"
        } else {
            "linux"
        };
        let arch = if self.target.starts_with("aarch64") {
            "arm64"
        } else {
            "x64"
        };
        format!("{os}-{arch}")
    }
}

impl fmt::Display for VersionInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} ({} · {} · {} · {})",
            self.version,
            self.git_sha,
            self.build_date,
            self.channel,
            self.platform_key()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_version_is_valid_semver_and_prints_all_fields() {
        let v = VersionInfo::current();
        let s = v.to_string();
        assert!(s.contains(&v.version.to_string()));
        assert!(s.contains(v.git_sha));
        assert!(s.contains(v.build_date));
    }
}
