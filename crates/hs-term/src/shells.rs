//! Shell profiles: what to run for "a terminal" on this machine.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ShellProfile {
    /// Stable id: "pwsh", "powershell", "cmd", "git-bash", "wsl:Ubuntu", "sh:/bin/zsh".
    pub id: String,
    pub name: String,
    pub icon: String,
    pub exe: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

impl ShellProfile {
    fn new(id: &str, name: &str, icon: &str, exe: impl Into<String>, args: &[&str]) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            icon: icon.into(),
            exe: exe.into(),
            args: args.iter().map(|a| a.to_string()).collect(),
            env: Vec::new(),
        }
    }
}

/// Detect the shells available on this machine, best first.
/// Detected profiles, computed once per process (detection runs `wsl -l`,
/// which can take seconds). Call [`prefetch`] at startup so the first
/// terminal opens without waiting.
pub fn profiles() -> std::sync::Arc<Vec<ShellProfile>> {
    static CACHE: std::sync::OnceLock<std::sync::Arc<Vec<ShellProfile>>> =
        std::sync::OnceLock::new();
    CACHE.get_or_init(|| std::sync::Arc::new(detect())).clone()
}

/// Run detection on a background thread so [`profiles`] is instant later.
pub fn prefetch() {
    std::thread::Builder::new()
        .name("hs-shell-detect".into())
        .spawn(|| {
            let _ = profiles();
        })
        .ok();
}

pub fn detect() -> Vec<ShellProfile> {
    let mut out = Vec::new();
    if cfg!(windows) {
        detect_windows(&mut out);
    } else {
        detect_unix(&mut out);
    }
    out
}

fn on_path(exe: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(exe);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

#[cfg(windows)]
fn detect_windows(out: &mut Vec<ShellProfile>) {
    let pf = std::env::var("ProgramFiles").unwrap_or_else(|_| r"C:\Program Files".into());
    let pf86 =
        std::env::var("ProgramFiles(x86)").unwrap_or_else(|_| r"C:\Program Files (x86)".into());
    let local = std::env::var("LOCALAPPDATA").unwrap_or_default();
    let windir = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());

    // PowerShell 7 (pwsh): PATH, Program Files, or the Store install.
    let pwsh = on_path("pwsh.exe")
        .or_else(|| {
            Some(PathBuf::from(format!(r"{pf}\PowerShell\7\pwsh.exe"))).filter(|p| p.is_file())
        })
        .or_else(|| {
            Some(PathBuf::from(format!(
                r"{local}\Microsoft\WindowsApps\pwsh.exe"
            )))
            .filter(|p| p.is_file())
        });
    if let Some(p) = pwsh {
        out.push(ShellProfile::new(
            "pwsh",
            "PowerShell 7",
            "",
            p.to_string_lossy(),
            &["-NoLogo"],
        ));
    }
    let ps5 = PathBuf::from(format!(
        r"{windir}\System32\WindowsPowerShell\v1.0\powershell.exe"
    ));
    if ps5.is_file() {
        out.push(ShellProfile::new(
            "powershell",
            "Windows PowerShell",
            "",
            ps5.to_string_lossy(),
            &["-NoLogo"],
        ));
    }
    let cmd = PathBuf::from(format!(r"{windir}\System32\cmd.exe"));
    if cmd.is_file() {
        out.push(ShellProfile::new(
            "cmd",
            "Command Prompt",
            "",
            cmd.to_string_lossy(),
            &[],
        ));
    }
    for base in [&pf, &pf86] {
        let bash = PathBuf::from(format!(r"{base}\Git\bin\bash.exe"));
        if bash.is_file() {
            let mut p = ShellProfile::new(
                "git-bash",
                "Git Bash",
                "",
                bash.to_string_lossy(),
                &["--login", "-i"],
            );
            p.env.push(("TERM".into(), "xterm-256color".into()));
            out.push(p);
            break;
        }
    }
    // WSL distributions: `wsl.exe -l -q` prints UTF-16LE, one name per line.
    if let Some(wsl) = on_path("wsl.exe")
        && let Ok(o) = std::process::Command::new(&wsl).args(["-l", "-q"]).output()
    {
        let text = decode_utf16le_or_utf8(&o.stdout);
        for name in text
            .lines()
            .map(|l| l.trim().trim_matches('\0'))
            .filter(|l| !l.is_empty())
        {
            out.push(ShellProfile::new(
                &format!("wsl:{name}"),
                &format!("WSL · {name}"),
                "",
                wsl.to_string_lossy(),
                &["-d", name],
            ));
        }
    }
}

#[cfg(not(windows))]
fn detect_windows(_out: &mut Vec<ShellProfile>) {}

fn detect_unix(out: &mut Vec<ShellProfile>) {
    if let Ok(sh) = std::env::var("SHELL")
        && std::path::Path::new(&sh).is_file()
    {
        let name = std::path::Path::new(&sh)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        out.push(ShellProfile::new(
            &format!("sh:{sh}"),
            &format!("{name} (login shell)"),
            "",
            sh.clone(),
            &["-l"],
        ));
    }
    for (exe, name) in [
        ("/bin/zsh", "zsh"),
        ("/bin/bash", "bash"),
        ("/usr/bin/fish", "fish"),
        ("/bin/sh", "sh"),
    ] {
        if std::path::Path::new(exe).is_file() && !out.iter().any(|p| p.exe == exe) {
            out.push(ShellProfile::new(
                &format!("sh:{exe}"),
                name,
                "",
                exe,
                &["-l"],
            ));
        }
    }
}

fn decode_utf16le_or_utf8(bytes: &[u8]) -> String {
    if bytes.len() >= 2 && bytes[1] == 0 {
        let units: Vec<u16> = bytes
            .chunks(2)
            .map(|c| u16::from_le_bytes([c[0], *c.get(1).unwrap_or(&0)]))
            .collect();
        String::from_utf16_lossy(&units)
    } else {
        String::from_utf8_lossy(bytes).to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn at_least_one_shell_is_detected() {
        let shells = detect();
        assert!(!shells.is_empty(), "no shell found on this machine");
        for s in &shells {
            assert!(
                std::path::Path::new(&s.exe).is_file(),
                "{} does not exist",
                s.exe
            );
            assert!(!s.id.is_empty() && !s.name.is_empty());
        }
    }

    #[test]
    fn utf16_decoding_handles_wsl_output() {
        let text = "Ubuntu\r\nDebian\r\n";
        let bytes: Vec<u8> = text.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
        assert_eq!(decode_utf16le_or_utf8(&bytes), text);
        assert_eq!(decode_utf16le_or_utf8(b"plain"), "plain");
    }
}
