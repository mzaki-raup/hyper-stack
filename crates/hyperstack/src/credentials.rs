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
    /// The profile's kind (`db`, `ssh`, …); the page turns it into a label.
    pub kind: String,
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
    let mut r = Row {
        id: profile.id.clone(),
        name: profile.name.clone(),
        kind: profile.kind.clone(),
        host: String::new(),
        port: String::new(),
        user: String::new(),
        password: Password::None,
        key_secret: Password::None,
        key: String::new(),
    };
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
        _ => return None,
    }
    Some(r)
}

/// Every connection that has something to show, in the order the store
/// keeps them.
pub fn rows(connections: &hs_core::connections::Connections) -> Vec<Row> {
    connections.all().iter().filter_map(row).collect()
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
