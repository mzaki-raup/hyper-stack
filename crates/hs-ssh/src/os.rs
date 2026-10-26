//! What is running on the other end, worked out from one read-only
//! probe, so a host's row and its tab can wear the right logo
//! (owner's requirement, 2026-10-26).
//!
//! The probe is deliberately harmless - it reads a file and asks for the
//! kernel name - and the *parsing* lives here as a pure function, so
//! every shape of output is testable without a server.

/// The probe. `/etc/os-release` names the distribution on every modern
/// Linux and on WSL; `uname -sr` covers the BSDs, macOS and a Linux
/// without that file, and its release string is also what gives WSL
/// away; `ver` answers on a Windows host whose shell is `cmd`, where
/// the first two are simply not commands.
///
/// Each part is allowed to fail: the parser takes whatever arrives.
pub const PROBE: &str =
    "cat /etc/os-release 2>/dev/null; uname -sr 2>/dev/null; ver 2>nul || true";

/// What the other end turned out to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HostOs {
    Ubuntu,
    Debian,
    Fedora,
    RedHat,
    CentOs,
    Alpine,
    Arch,
    SuSE,
    Linux,
    MacOs,
    Windows,
    FreeBsd,
    #[default]
    Unknown,
}

impl HostOs {
    /// The id stored on the profile, and read back by [`HostOs::from_id`].
    pub fn id(self) -> &'static str {
        match self {
            Self::Ubuntu => "ubuntu",
            Self::Debian => "debian",
            Self::Fedora => "fedora",
            Self::RedHat => "rhel",
            Self::CentOs => "centos",
            Self::Alpine => "alpine",
            Self::Arch => "arch",
            Self::SuSE => "suse",
            Self::Linux => "linux",
            Self::MacOs => "macos",
            Self::Windows => "windows",
            Self::FreeBsd => "freebsd",
            Self::Unknown => "",
        }
    }

    pub fn from_id(id: &str) -> Self {
        match id {
            "ubuntu" => Self::Ubuntu,
            "debian" => Self::Debian,
            "fedora" => Self::Fedora,
            "rhel" => Self::RedHat,
            "centos" => Self::CentOs,
            "alpine" => Self::Alpine,
            "arch" => Self::Arch,
            "suse" => Self::SuSE,
            "linux" => Self::Linux,
            "macos" => Self::MacOs,
            "windows" => Self::Windows,
            "freebsd" => Self::FreeBsd,
            _ => Self::Unknown,
        }
    }

    /// The logo, from the bundled Nerd Font's Linux set (`hs_ui::fonts`
    /// installs it as the last fallback, so these render wherever the
    /// interface font does not have them). A distribution with no logo
    /// of its own falls back to Tux rather than to a box.
    pub fn icon(self) -> &'static str {
        match self {
            Self::Ubuntu => "\u{f31b}",
            Self::Debian => "\u{f306}",
            Self::Fedora => "\u{f30a}",
            Self::RedHat => "\u{f316}",
            Self::CentOs => "\u{f304}",
            Self::Alpine => "\u{f300}",
            Self::Arch => "\u{f303}",
            Self::SuSE => "\u{f314}",
            Self::Linux => "\u{f17c}",
            Self::MacOs => "\u{f302}",
            Self::Windows => "\u{f17a}",
            Self::FreeBsd => "\u{f30c}",
            // Not detected yet: the generic host glyph, as before.
            Self::Unknown => hs_ui_icon_fallback(),
        }
    }
}

/// `ph::COMPUTER_TOWER`, without `hs-ssh` depending on `hs-ui`.
const fn hs_ui_icon_fallback() -> &'static str {
    "\u{e4b6}"
}

/// What the probe found: the family, and a line fit to show a human.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Detected {
    pub os: HostOs,
    /// `Ubuntu 24.04.1 LTS`, `Darwin 23.5.0`, and ` · WSL` appended when
    /// the kernel says so.
    pub label: String,
}

/// Read the probe's output.
///
/// `/etc/os-release` wins when it is there - it is the only source that
/// names the *distribution* - and `uname` fills in for everything else.
/// A value may be quoted; `ID` may be a word like `rhel` or
/// `opensuse-leap`, and `ID_LIKE` catches a derivative whose own id we
/// do not know (`linuxmint` is `ID_LIKE=ubuntu`).
pub fn parse(output: &str) -> Detected {
    let mut id = String::new();
    let mut id_like = String::new();
    let mut pretty = String::new();
    let mut name = String::new();
    let mut version = String::new();
    let mut uname = String::new();
    let mut windows = String::new();

    for line in output.lines() {
        let line = line.trim();
        if let Some(v) = field(line, "PRETTY_NAME=") {
            pretty = v;
        } else if let Some(v) = field(line, "ID_LIKE=") {
            id_like = v.to_ascii_lowercase();
        } else if let Some(v) = field(line, "ID=") {
            id = v.to_ascii_lowercase();
        } else if let Some(v) = field(line, "NAME=") {
            name = v;
        } else if let Some(v) = field(line, "VERSION=") {
            version = v;
        } else if line.contains("Microsoft Windows") {
            windows = line.to_string();
        } else if !line.is_empty() && !line.contains('=') && uname.is_empty() {
            // `uname -sr`: "Linux 6.8.0-45-generic", "Darwin 23.5.0".
            uname = line.to_string();
        }
    }

    let from_id = |word: &str| -> Option<HostOs> {
        Some(match word {
            "ubuntu" => HostOs::Ubuntu,
            "debian" | "raspbian" => HostOs::Debian,
            "fedora" => HostOs::Fedora,
            "rhel" | "redhat" => HostOs::RedHat,
            "centos" | "rocky" | "almalinux" => HostOs::CentOs,
            "alpine" => HostOs::Alpine,
            "arch" | "archarm" | "manjaro" => HostOs::Arch,
            _ if word.contains("suse") => HostOs::SuSE,
            _ => return None,
        })
    };

    let kernel = uname.to_ascii_lowercase();
    let os = from_id(&id)
        .or_else(|| id_like.split_whitespace().find_map(from_id))
        .or_else(|| (!id.is_empty()).then_some(HostOs::Linux))
        .or_else(|| {
            if !windows.is_empty() {
                Some(HostOs::Windows)
            } else if kernel.starts_with("darwin") {
                Some(HostOs::MacOs)
            } else if kernel.contains("freebsd") {
                Some(HostOs::FreeBsd)
            } else if kernel.starts_with("linux") {
                Some(HostOs::Linux)
            } else if kernel.contains("mingw") || kernel.contains("cygwin") {
                Some(HostOs::Windows)
            } else {
                None
            }
        })
        .unwrap_or_default();

    // The best name the output offered, in order of how much it says.
    let mut label = if !pretty.is_empty() {
        pretty
    } else if !name.is_empty() && !version.is_empty() {
        format!("{name} {version}")
    } else if !name.is_empty() {
        name
    } else if !windows.is_empty() {
        windows.trim_matches(['[', ']']).to_string()
    } else {
        uname.clone()
    };
    // WSL is still Ubuntu (or whatever it is) and wears that logo; the
    // label is where "and it is running under Windows" belongs.
    if kernel.contains("microsoft") || kernel.contains("wsl") {
        if label.is_empty() {
            label = "WSL".into();
        } else {
            label.push_str(" · WSL");
        }
    }
    Detected { os, label }
}

/// `KEY=value` or `KEY="value"`, with the quotes taken off.
fn field(line: &str, key: &str) -> Option<String> {
    let rest = line.strip_prefix(key)?;
    Some(rest.trim().trim_matches('"').trim_matches('\'').to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn os_release_names_the_distribution() {
        let out = "NAME=\"Ubuntu\"\nVERSION=\"24.04.1 LTS (Noble Numbat)\"\nID=ubuntu\n\
                   ID_LIKE=debian\nPRETTY_NAME=\"Ubuntu 24.04.1 LTS\"\nLinux 6.8.0-45-generic\n";
        let d = parse(out);
        assert_eq!(d.os, HostOs::Ubuntu);
        assert_eq!(d.label, "Ubuntu 24.04.1 LTS");
    }

    /// A derivative we do not know by name is still placed by `ID_LIKE`,
    /// rather than falling all the way back to a generic penguin.
    #[test]
    fn a_derivative_is_placed_by_id_like() {
        let d = parse("ID=linuxmint\nID_LIKE=ubuntu\nPRETTY_NAME=\"Linux Mint 21\"\n");
        assert_eq!(d.os, HostOs::Ubuntu);
        assert_eq!(d.label, "Linux Mint 21");
    }

    #[test]
    fn red_hat_and_its_rebuilds_are_told_apart() {
        assert_eq!(parse("ID=\"rhel\"\n").os, HostOs::RedHat);
        assert_eq!(parse("ID=\"rocky\"\n").os, HostOs::CentOs);
        assert_eq!(parse("ID=\"opensuse-leap\"\n").os, HostOs::SuSE);
    }

    /// No `/etc/os-release` at all: `uname` is the whole answer.
    #[test]
    fn uname_alone_is_enough_for_mac_and_bsd() {
        assert_eq!(parse("Darwin 23.5.0\n").os, HostOs::MacOs);
        assert_eq!(parse("FreeBSD 14.0-RELEASE\n").os, HostOs::FreeBsd);
        assert_eq!(parse("Linux 5.15.0\n").os, HostOs::Linux);
    }

    /// WSL keeps the distribution's own logo - it *is* Ubuntu - and says
    /// where it is running in the label instead.
    #[test]
    fn wsl_keeps_its_distribution_and_says_so() {
        let out = "ID=ubuntu\nPRETTY_NAME=\"Ubuntu 24.04 LTS\"\n\
                   Linux 5.15.167.4-microsoft-standard-WSL2\n";
        let d = parse(out);
        assert_eq!(d.os, HostOs::Ubuntu, "WSL Ubuntu is still Ubuntu");
        assert!(d.label.ends_with("· WSL"), "{}", d.label);
    }

    #[test]
    fn a_windows_host_answers_ver() {
        let d = parse("Microsoft Windows [Version 10.0.26100.2033]\n");
        assert_eq!(d.os, HostOs::Windows);
        assert!(d.label.contains("Microsoft Windows"));
    }

    /// Nothing recognisable must not crash or claim a guess.
    #[test]
    fn an_empty_or_odd_probe_is_unknown() {
        assert_eq!(parse("").os, HostOs::Unknown);
        assert_eq!(parse("   \n\n").os, HostOs::Unknown);
    }

    /// The id is what goes on the profile, so it has to survive a round
    /// trip - a typo here would silently reset every host to Unknown.
    #[test]
    fn every_os_round_trips_through_its_id() {
        for os in [
            HostOs::Ubuntu,
            HostOs::Debian,
            HostOs::Fedora,
            HostOs::RedHat,
            HostOs::CentOs,
            HostOs::Alpine,
            HostOs::Arch,
            HostOs::SuSE,
            HostOs::Linux,
            HostOs::MacOs,
            HostOs::Windows,
            HostOs::FreeBsd,
        ] {
            assert_eq!(HostOs::from_id(os.id()), os, "{os:?}");
        }
    }
}
