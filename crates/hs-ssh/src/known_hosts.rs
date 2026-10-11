//! Host key memory (docs/tools/ssh-sftp.md §5): `HyperStackData/known_hosts`
//! in a simple line format `[host]:port algorithm base64-key`, so trust
//! decisions travel with the portable folder.

use std::path::{Path, PathBuf};

use parking_lot::Mutex;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Known,
    Unknown,
    /// A key is on file but a different one was presented.
    Changed {
        stored: String,
    },
}

pub struct KnownHosts {
    path: PathBuf,
    lines: Mutex<Vec<(String, String, String)>>, // (hostport, algorithm, key)
}

impl KnownHosts {
    pub fn load(path: &Path) -> Self {
        let mut lines = Vec::new();
        if let Ok(text) = std::fs::read_to_string(path) {
            for l in text.lines() {
                let parts: Vec<&str> = l.split_whitespace().collect();
                if parts.len() >= 3 && !l.starts_with('#') {
                    lines.push((
                        parts[0].to_string(),
                        parts[1].to_string(),
                        parts[2].to_string(),
                    ));
                }
            }
        }
        Self {
            path: path.to_path_buf(),
            lines: Mutex::new(lines),
        }
    }

    pub fn key_for(host: &str, port: u16) -> String {
        if port == 22 {
            host.to_string()
        } else {
            format!("[{host}]:{port}")
        }
    }

    pub fn check(&self, host: &str, port: u16, algorithm: &str, key_b64: &str) -> Verdict {
        let hp = Self::key_for(host, port);
        let lines = self.lines.lock();
        let same_host: Vec<&(String, String, String)> =
            lines.iter().filter(|(h, _, _)| *h == hp).collect();
        if same_host.is_empty() {
            return Verdict::Unknown;
        }
        if same_host
            .iter()
            .any(|(_, a, k)| a == algorithm && k == key_b64)
        {
            return Verdict::Known;
        }
        match same_host.iter().find(|(_, a, _)| a == algorithm) {
            Some((_, _, k)) => Verdict::Changed { stored: k.clone() },
            // Different algorithm than the one on file: treat as unknown (new key type).
            None => Verdict::Unknown,
        }
    }

    /// Remember (or replace) the key for a host and write the file.
    pub fn trust(&self, host: &str, port: u16, algorithm: &str, key_b64: &str) {
        let hp = Self::key_for(host, port);
        let mut lines = self.lines.lock();
        lines.retain(|(h, a, _)| !(*h == hp && a == algorithm));
        lines.push((hp, algorithm.to_string(), key_b64.to_string()));
        let text: String = std::iter::once(
            "# HyperStack known hosts: [host]:port algorithm key (edit or delete lines to forget)\n"
                .to_string(),
        )
        .chain(lines.iter().map(|(h, a, k)| format!("{h} {a} {k}\n")))
        .collect();
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Err(e) = std::fs::write(&self.path, text) {
            tracing::warn!("known_hosts not saved: {e}");
        }
    }

    pub fn forget(&self, host: &str, port: u16) {
        let hp = Self::key_for(host, port);
        self.lines.lock().retain(|(h, _, _)| *h != hp);
        let lines = self.lines.lock();
        let text: String = lines
            .iter()
            .map(|(h, a, k)| format!("{h} {a} {k}\n"))
            .collect();
        let _ = std::fs::write(&self.path, text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trust_check_change_and_persist() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("known_hosts");
        let kh = KnownHosts::load(&path);
        assert_eq!(kh.check("web", 22, "ssh-ed25519", "AAAA"), Verdict::Unknown);
        kh.trust("web", 22, "ssh-ed25519", "AAAA");
        assert_eq!(kh.check("web", 22, "ssh-ed25519", "AAAA"), Verdict::Known);
        assert_eq!(
            kh.check("web", 22, "ssh-ed25519", "BBBB"),
            Verdict::Changed {
                stored: "AAAA".into()
            }
        );
        assert_eq!(
            kh.check("web", 2222, "ssh-ed25519", "AAAA"),
            Verdict::Unknown
        );
        let again = KnownHosts::load(&path);
        assert_eq!(
            again.check("web", 22, "ssh-ed25519", "AAAA"),
            Verdict::Known
        );
        again.forget("web", 22);
        assert_eq!(
            again.check("web", 22, "ssh-ed25519", "AAAA"),
            Verdict::Unknown
        );
    }
}
