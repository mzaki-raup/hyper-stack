//! The credentials table on Settings → Security.
//!
//! Every saved connection, flattened into the same eight columns whatever
//! kind it is, so one page answers "what is stored, and where does it point"
//! without opening six different forms. The password is masked; revealing one
//! row costs the master password and lapses on its own.
//!
//! Nothing here decrypts anything: a row carries the vault *reference*, and
//! the page reads the value through `SecretStore` only while that row is
//! revealed, so no plaintext is ever held in UI state.

use hs_core::connections::ConnectionProfile;
use hs_core::secrets::SecretRef;

/// What a row's password column holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Password {
    /// Nothing stored for this connection.
    None,
    /// In the vault. Revealing it asks for the master password.
    Vault(SecretRef),
    /// A `{{variable}}`: the name of an environment value, not a secret, so
    /// it is shown as it is — hiding it would hide which variable is used.
    Variable(String),
    /// Text sitting in the profile. The forms refuse to write this, but a
    /// hand-edited file can carry it; masked like a vault secret so a
    /// shoulder does not read it off the page.
    Plain(String),
}

impl Password {
    fn read(value: &str) -> Self {
        let v = value.trim();
        if v.is_empty() {
            Password::None
        } else if let Some(r) = SecretRef::parse(v) {
            Password::Vault(r)
        } else if v.starts_with("{{") {
            Password::Variable(v.to_string())
        } else {
            Password::Plain(v.to_string())
        }
    }
}

/// One connection, as the table shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub id: String,
    pub name: String,
    /// The profile's kind (`db`, `ssh`, …), or the source of a credential
    /// that is not a connection (`git`, `registry`, `ai`); the page turns
    /// it into a label.
    pub kind: String,
    /// What the source adds to that label: the forge for a Git credential,
    /// the provider for an AI key. Empty for connections.
    pub detail: String,
    /// Host, endpoint, the URI a document store was given, or the file for
    /// SQLite — whatever this profile points at, in one column. A separate
    /// connection-string column was dropped: it was empty for every kind
    /// but one, and that one puts its URI here instead.
    pub host: String,
    /// Empty when the kind has no port (S3, SQLite).
    pub port: String,
    pub user: String,
    pub password: Password,
    /// The second secret a kind stores. For SSH key auth this is the key
    /// passphrase; the file path stays in `key`.
    pub key_secret: Password,
    /// Key material that is not itself a secret: the SSH key file.
    pub key: String,
}

fn port(p: u16) -> String {
    if p == 0 { String::new() } else { p.to_string() }
}

/// Flatten one profile. Kinds with no server of their own (an API
/// collection) return `None` rather than an empty row.
pub fn row(profile: &ConnectionProfile) -> Option<Row> {
    let f = &profile.fields;
    let mut r = blank(profile.id.clone(), profile.name.clone(), &profile.kind);
    match profile.kind.as_str() {
        "ssh" => {
            let v = hs_ssh::SshFields::from_table(f);
            r.host = v.host;
            r.port = port(v.port);
            r.user = v.user;
            r.key = v.key_path;
            // A key-auth host has two credentials: the key file (a path,
            // shown as it is) and the passphrase that opens it (the secret
            // in the Key column). Password auth has one.
            if v.auth == "key" {
                r.key_secret = Password::read(&v.key_passphrase);
            } else {
                r.password = Password::read(&v.password);
            }
        }
        "db" => {
            let v = tool_db::profile::DbFields::from_table(f);
            // SQLite has a file where the others have a host.
            r.host = if v.path.is_empty() { v.host } else { v.path };
            r.port = port(v.port);
            r.user = v.user;
            r.password = Password::read(&v.password);
        }
        "redis" | "valkey" => {
            let v = tool_redis::profile::RedisFields::from_table(f);
            r.host = v.host;
            r.port = port(v.port);
            r.user = v.user;
            r.password = Password::read(&v.password);
        }
        "mongodb" | "couchdb" | "qdrant" => {
            let v = tool_nosql::profile::NoSqlFields::from_table(&profile.kind, f);
            // A URI wins over host and port in the driver, so it is what
            // this profile actually points at.
            r.host = if v.uri.is_empty() { v.host } else { v.uri };
            r.port = port(v.port);
            r.user = v.user;
            r.password = Password::read(&v.password);
        }
        kind if hs_core::connections::is_automation_kind(kind) => {
            let v = tool_automation::profile::AutomationFields::from_table(kind, f);
            r.host = v.url;
            r.user = v.credential.user;
            r.password = Password::read(&v.credential.secret);
        }
        kind if hs_core::connections::is_tracker_kind(kind) => {
            let v = tool_tracker::profile::TrackerFields::from_table(kind, f);
            r.host = v.url;
            r.user = v.credential.user;
            r.password = Password::read(&v.credential.secret);
        }
        kind if hs_core::connections::is_queue_kind(kind) => {
            let v = tool_queue::profile::QueueFields::from_table(kind, f);
            r.host = v.address;
            r.user = v.user;
            r.password = Password::read(&v.secret);
        }
        "s3" => {
            let v = hs_core::objstore::profile::S3Fields::from_table(f);
            r.host = v.endpoint;
            r.user = v.access_key;
            r.password = Password::read(&v.secret_key);
        }
        "webdav" => {
            let v = hs_core::objstore::http_profile::WebdavFields::from_table(f);
            r.host = v.url;
            r.user = v.user;
            r.password = Password::read(&v.password);
        }
        "nexus" => {
            let v = hs_core::objstore::http_profile::NexusFields::from_table(f);
            r.host = v.url;
            r.user = v.user;
            r.password = Password::read(&v.password);
        }
        "k8s" => {
            // A cluster signs in with a token, or with credentials inside
            // a kubeconfig pasted into the profile.
            let v = hs_container::profile::ContainerFields::from_table(f);
            r.host = if v.context.is_empty() {
                v.kubeconfig_path
            } else {
                v.context
            };
            r.user = v.impersonate_user;
            r.password = Password::read(&v.token);
            r.key_secret = vault_only(&v.kubeconfig_inline);
        }
        kind if hs_core::connections::is_container_kind(kind) => {
            // An engine's secrets are the registry it pushes to and the
            // TLS client key, which may instead be a file path.
            let v = hs_container::profile::ContainerFields::from_table(f);
            r.host = if v.host.is_empty() { v.socket } else { v.host };
            r.port = port(v.port);
            r.user = v.registry_user;
            r.password = Password::read(&v.registry_password);
            r.key_secret = vault_only(&v.tls_key);
            if r.key_secret == Password::None {
                r.key = v.tls_key;
            }
        }
        _ => return None,
    }
    Some(r)
}

/// A field that is either a vault reference or something else entirely (a
/// file path): only the reference is a secret to show.
fn vault_only(value: &str) -> Password {
    match SecretRef::parse(value.trim()) {
        Some(r) => Password::Vault(r),
        None => Password::None,
    }
}

/// The id prefix of a row that is not a connection profile.
pub const GIT: &str = "git:";
pub const REGISTRY: &str = "registry:";
pub const AI: &str = "ai:";

fn blank(id: String, name: String, kind: &str) -> Row {
    Row {
        id,
        name,
        kind: kind.to_string(),
        detail: String::new(),
        host: String::new(),
        port: String::new(),
        user: String::new(),
        password: Password::None,
        key_secret: Password::None,
        key: String::new(),
    }
}

/// Every saved credential, from every place one is kept: the connection
/// profiles, then Git credentials, container registries and AI providers.
pub fn rows_from(
    profiles: &[ConnectionProfile],
    git: &[hs_core::git_hosts::GitCredential],
    registries: &[hs_core::settings::Registry],
    providers: &[hs_core::settings::AiProvider],
) -> Vec<Row> {
    let mut out: Vec<Row> = profiles.iter().filter_map(row).collect();
    for g in git {
        let mut r = blank(format!("{GIT}{}", g.id), g.label(), "git");
        r.detail = g.kind.name().to_string();
        r.host = g.host();
        r.user = g.username.clone();
        r.password = Password::read(&g.token);
        out.push(r);
    }
    for (i, reg) in registries.iter().enumerate() {
        let name = if reg.name.trim().is_empty() {
            reg.host().to_string()
        } else {
            reg.name.clone()
        };
        let mut r = blank(format!("{REGISTRY}{i}"), name, "registry");
        r.host = reg.host().to_string();
        r.user = reg.user.clone();
        r.password = Password::read(&reg.password);
        out.push(r);
    }
    for p in providers {
        let mut r = blank(format!("{AI}{}", p.id), p.name.clone(), "ai");
        r.detail = p.kind.label().to_string();
        r.host = p.base_url.clone();
        r.password = Password::read(&p.api_key);
        r.key_secret = Password::read(&p.secret_key);
        out.push(r);
    }
    out
}

/// Every saved credential the services know of, for Settings → Security.
pub fn rows(s: &hs_core::Services) -> Vec<Row> {
    let settings = s.settings.read();
    rows_from(
        s.connections.read().all(),
        s.git_credentials.read().all(),
        &settings.containers.registries,
        &settings.ai.providers,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(kind: &str, table: &str) -> ConnectionProfile {
        ConnectionProfile {
            name: format!("a {kind}"),
            kind: kind.into(),
            fields: toml::from_str(table).expect("fixture parses"),
            ..Default::default()
        }
    }

    #[test]
    fn every_kind_flattens_into_the_same_columns() {
        let db = row(&profile(
            "db",
            r#"[db]
engine = "postgres"
host = "db.example"
port = 5432
user = "app"
password = "secret:01HQ"
"#,
        ))
        .expect("db is listed");
        assert_eq!(db.host, "db.example");
        assert_eq!(db.port, "5432");
        assert_eq!(db.user, "app");
        assert_eq!(db.password, Password::Vault(SecretRef("01HQ".into())));

        let mongo = row(&profile(
            "mongodb",
            r#"[mongodb]
uri = "mongodb://shop:27017/orders"
user = "reader"
password = "{{mongo_pw}}"
"#,
        ))
        .expect("mongodb is listed");
        assert_eq!(
            mongo.host, "mongodb://shop:27017/orders",
            "a URI is what the profile points at, so it is the Host/URL"
        );
        assert_eq!(
            mongo.password,
            Password::Variable("{{mongo_pw}}".into()),
            "a variable names an environment value, so it is not masked"
        );

        let s3 = row(&profile(
            "s3",
            r#"[s3]
endpoint = "https://s3.example"
access_key = "AKIA"
secret_key = "secret:01HR"
"#,
        ))
        .expect("s3 is listed");
        assert_eq!(s3.host, "https://s3.example");
        assert_eq!(s3.user, "AKIA");
        assert_eq!(s3.port, "", "S3 has no port of its own");

        // An API collection has no server, so it is not a credential.
        assert!(row(&profile("api", "")).is_none());
    }

    #[test]
    fn sqlite_shows_its_file_where_a_server_shows_its_host() {
        let r = row(&profile(
            "db",
            r#"[db]
engine = "sqlite"
path = "D:/data/shop.db"
"#,
        ))
        .expect("db is listed");
        assert_eq!(r.host, "D:/data/shop.db");
        assert_eq!(r.port, "", "SQLite has no port");
        assert_eq!(r.password, Password::None);
    }

    #[test]
    fn a_key_auth_host_shows_its_key_and_its_passphrase() {
        let r = row(&profile(
            "ssh",
            r#"[ssh]
host = "build.example"
port = 22
user = "deploy"
auth = "key"
key_path = "~/.ssh/id_ed25519"
key_passphrase = "secret:01HS"
password = ""
"#,
        ))
        .expect("ssh is listed");
        assert_eq!(r.key, "~/.ssh/id_ed25519");
        assert_eq!(
            r.key_secret,
            Password::Vault(SecretRef("01HS".into())),
            "with key auth the passphrase is the secret that opens the key"
        );
        assert_eq!(
            r.password,
            Password::None,
            "a key-auth host has no password of its own"
        );
    }

    /// BUG-0036: Git credentials, container registries, AI providers and
    /// the container, WebDAV and Nexus connections all keep secrets in the
    /// vault, and none of them were on the page.
    #[test]
    fn every_saved_credential_is_listed_not_only_database_style_connections() {
        let mut conns = Vec::new();
        for (kind, table) in [
            (
                "docker",
                "[container]
host = \"10.0.0.5\"
port = 2376
registry_user = \"ci\"
registry_password = \"secret:01DK\"
",
            ),
            (
                "k8s",
                "[container]
context = \"prod\"
token = \"secret:01K8\"
",
            ),
            (
                "webdav",
                "[webdav]
url = \"https://dav.example\"
user = \"me\"
password = \"secret:01WD\"
",
            ),
            (
                "nexus",
                "[nexus]
url = \"https://nexus.example\"
user = \"ci\"
password = \"secret:01NX\"
",
            ),
        ] {
            conns.push(profile(kind, table));
        }
        let git = vec![hs_core::git_hosts::GitCredential {
            name: "Work GitLab".into(),
            kind: hs_core::git_hosts::HostKind::GitLab,
            base_url: "https://gitlab.example".into(),
            username: "ada".into(),
            token: "secret:01GT".into(),
            ..Default::default()
        }];
        let registries = vec![hs_core::settings::Registry {
            name: "Harbour".into(),
            url: "harbour.example:8082".into(),
            user: "robot".into(),
            password: "secret:01RG".into(),
        }];
        let providers = vec![hs_core::settings::AiProvider {
            id: "ai1".into(),
            name: "Work OpenAI".into(),
            api_key: "secret:01AI".into(),
            ..Default::default()
        }];
        let rows = rows_from(&conns, &git, &registries, &providers);
        let find = |kind: &str| {
            rows.iter()
                .find(|r| r.kind == kind)
                .unwrap_or_else(|| panic!("no {kind} row in {rows:#?}"))
        };
        assert_eq!(
            find("docker").password,
            Password::Vault(SecretRef("01DK".into()))
        );
        assert_eq!(find("docker").user, "ci");
        assert_eq!(
            find("k8s").password,
            Password::Vault(SecretRef("01K8".into()))
        );
        assert_eq!(find("webdav").host, "https://dav.example");
        assert_eq!(
            find("nexus").password,
            Password::Vault(SecretRef("01NX".into()))
        );
        let g = find("git");
        assert_eq!((g.name.as_str(), g.user.as_str()), ("Work GitLab", "ada"));
        assert_eq!(g.password, Password::Vault(SecretRef("01GT".into())));
        assert_eq!(find("registry").host, "harbour.example:8082");
        assert_eq!(
            find("ai").password,
            Password::Vault(SecretRef("01AI".into()))
        );
        // Ids stay unique across sources, so reveal and edit find the row.
        let mut ids: Vec<&str> = rows.iter().map(|r| r.id.as_str()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), rows.len());
    }

    #[test]
    fn nothing_stored_is_not_a_masked_empty_string() {
        assert_eq!(Password::read(""), Password::None);
        assert_eq!(Password::read("   "), Password::None);
        assert_eq!(
            Password::read("{{pw}}"),
            Password::Variable("{{pw}}".into())
        );
        assert_eq!(
            Password::read("hunter2"),
            Password::Plain("hunter2".into()),
            "text in the profile is still masked on the page"
        );
    }
}
