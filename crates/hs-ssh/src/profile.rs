//! The `[ssh]` table of a `kind = "ssh"` connection profile
//! (docs/tools/ssh-sftp.md §4), read and written as plain strings so the
//! settings form and the tool agree on the keys.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshFields {
    pub host: String,
    pub port: u16,
    pub user: String,
    /// "password" | "key" | "agent"
    pub auth: String,
    /// `secret:<id>` or empty.
    pub password: String,
    pub key_path: String,
    /// `secret:<id>` or empty.
    pub key_passphrase: String,
    pub remote_dir: String,
    pub startup_command: String,
    pub keepalive_secs: u64,
    pub connect_timeout_secs: u64,
    /// What the host turned out to be, from one read-only probe on the
    /// first connect (`os::PROBE`). Kept on the profile so the sidebar
    /// can draw the right logo without connecting, and so a restored tab
    /// wears it before anything has happened. Empty until detected.
    pub os_id: String,
    /// The human form of the same: `Ubuntu 24.04.1 LTS`, `· WSL` and all.
    pub os_label: String,
    /// Port forwards kept with the host (docs/tools/ssh-sftp.md §8), the
    /// ones with `auto` started when a tab connects.
    pub forwards: Vec<SavedForward>,
}

/// One `ssh -L` kept on the profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedForward {
    pub remote_host: String,
    pub remote_port: u16,
    /// 0 = any free port.
    pub local_port: u16,
    /// Start it when a tab connects to the host.
    pub auto: bool,
}

impl Default for SshFields {
    fn default() -> Self {
        Self {
            host: String::new(),
            port: 22,
            user: String::new(),
            auth: "password".into(),
            password: String::new(),
            key_path: String::new(),
            key_passphrase: String::new(),
            remote_dir: String::new(),
            startup_command: String::new(),
            keepalive_secs: 30,
            connect_timeout_secs: 20,
            os_id: String::new(),
            os_label: String::new(),
            forwards: Vec::new(),
        }
    }
}

impl SshFields {
    pub fn from_table(fields: &toml::Table) -> Self {
        let t = fields
            .get("ssh")
            .and_then(|v| v.as_table())
            .cloned()
            .unwrap_or_default();
        let s = |k: &str| t.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
        let n = |k: &str, d: i64| t.get(k).and_then(|v| v.as_integer()).unwrap_or(d);
        let d = Self::default();
        Self {
            host: s("host"),
            port: n("port", 22).clamp(1, 65535) as u16,
            user: s("user"),
            auth: {
                let a = s("auth");
                if a.is_empty() { d.auth } else { a }
            },
            password: s("password"),
            key_path: s("key_path"),
            key_passphrase: s("key_passphrase"),
            remote_dir: s("remote_dir"),
            startup_command: s("startup_command"),
            keepalive_secs: n("keepalive_secs", 30).max(0) as u64,
            connect_timeout_secs: n("connect_timeout_secs", 20).max(1) as u64,
            os_id: s("os_id"),
            os_label: s("os_label"),
            forwards: t
                .get("forwards")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_table())
                        .map(|f| SavedForward {
                            remote_host: f
                                .get("remote_host")
                                .and_then(|v| v.as_str())
                                .unwrap_or("localhost")
                                .to_string(),
                            remote_port: f
                                .get("remote_port")
                                .and_then(|v| v.as_integer())
                                .unwrap_or(0)
                                .clamp(0, 65535) as u16,
                            local_port: f
                                .get("local_port")
                                .and_then(|v| v.as_integer())
                                .unwrap_or(0)
                                .clamp(0, 65535) as u16,
                            auto: f.get("auto").and_then(|v| v.as_bool()).unwrap_or(true),
                        })
                        .collect()
                })
                .unwrap_or_default(),
        }
    }

    pub fn write_into(&self, fields: &mut toml::Table) {
        let mut t = toml::Table::new();
        t.insert("host".into(), toml::Value::String(self.host.clone()));
        t.insert("port".into(), toml::Value::Integer(self.port as i64));
        t.insert("user".into(), toml::Value::String(self.user.clone()));
        t.insert("auth".into(), toml::Value::String(self.auth.clone()));
        t.insert(
            "password".into(),
            toml::Value::String(self.password.clone()),
        );
        t.insert(
            "key_path".into(),
            toml::Value::String(self.key_path.clone()),
        );
        t.insert(
            "key_passphrase".into(),
            toml::Value::String(self.key_passphrase.clone()),
        );
        t.insert(
            "remote_dir".into(),
            toml::Value::String(self.remote_dir.clone()),
        );
        t.insert(
            "startup_command".into(),
            toml::Value::String(self.startup_command.clone()),
        );
        t.insert(
            "keepalive_secs".into(),
            toml::Value::Integer(self.keepalive_secs as i64),
        );
        t.insert(
            "connect_timeout_secs".into(),
            toml::Value::Integer(self.connect_timeout_secs as i64),
        );
        t.insert("os_id".into(), toml::Value::String(self.os_id.clone()));
        t.insert(
            "os_label".into(),
            toml::Value::String(self.os_label.clone()),
        );
        if !self.forwards.is_empty() {
            let list = self
                .forwards
                .iter()
                .map(|f| {
                    let mut ft = toml::Table::new();
                    ft.insert(
                        "remote_host".into(),
                        toml::Value::String(f.remote_host.clone()),
                    );
                    ft.insert(
                        "remote_port".into(),
                        toml::Value::Integer(f.remote_port as i64),
                    );
                    ft.insert(
                        "local_port".into(),
                        toml::Value::Integer(f.local_port as i64),
                    );
                    ft.insert("auto".into(), toml::Value::Boolean(f.auto));
                    toml::Value::Table(ft)
                })
                .collect();
            t.insert("forwards".into(), toml::Value::Array(list));
        }
        fields.insert("ssh".into(), toml::Value::Table(t));
    }

    pub fn label(&self) -> String {
        if self.user.is_empty() {
            self.host.clone()
        } else {
            format!("{}@{}", self.user, self.host)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_the_toml_table() {
        let f = SshFields {
            host: "web-01".into(),
            user: "ubuntu".into(),
            port: 2222,
            auth: "key".into(),
            key_path: "~/.ssh/id_ed25519".into(),
            ..Default::default()
        };
        let mut table = toml::Table::new();
        f.write_into(&mut table);
        let back = SshFields::from_table(&table);
        assert_eq!(back, f);
        assert_eq!(back.label(), "ubuntu@web-01");
        assert_eq!(SshFields::from_table(&toml::Table::new()).port, 22);
    }
}

#[cfg(test)]
mod forward_tests {
    use super::*;

    #[test]
    fn saved_forwards_round_trip() {
        let mut f = SshFields {
            host: "h".into(),
            ..Default::default()
        };
        f.forwards.push(SavedForward {
            remote_host: "localhost".into(),
            remote_port: 5432,
            local_port: 0,
            auto: true,
        });
        f.forwards.push(SavedForward {
            remote_host: "db".into(),
            remote_port: 6379,
            local_port: 16379,
            auto: false,
        });
        let mut table = toml::Table::new();
        f.write_into(&mut table);
        let back = SshFields::from_table(&table);
        assert_eq!(back.forwards, f.forwards);
        let text = toml::to_string(&table).unwrap();
        assert!(text.contains("[[ssh.forwards]]"), "{text}");
        assert!(
            SshFields::from_table(&toml::Table::new())
                .forwards
                .is_empty()
        );
    }
}
