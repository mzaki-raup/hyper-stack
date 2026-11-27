//! Which saved Database connection a server's database is (docs/tools/
//! analytics.md §4.6, "Open in Database"). Pure: the screen asks with the
//! database's details and the saved profiles and gets the ids that match.
//!
//! A profile matches when its engine is the same product, its host is the
//! same machine, its port equals the server's (when both are known) and its
//! database name equals the server's (when both are known). Only the
//! server's admins see a database's details, so for anyone else nothing
//! matches and the Database tab opens with its connection picker.

use hs_core::connections::ConnectionProfile;

use crate::provider::Database;

/// The Database tool's engine names a Metabase driver can be.
fn engines(metabase: &str) -> &'static [&'static str] {
    match metabase {
        "postgres" => &["postgres"],
        "mysql" => &["mysql", "mariadb"],
        "sqlserver" => &["mssql"],
        "clickhouse" => &["clickhouse"],
        "oracle" => &["oracle"],
        "sqlite" => &["sqlite"],
        "duckdb" => &["duckdb"],
        _ => &[],
    }
}

/// A host written so that two names for this machine compare equal.
fn host_key(host: &str) -> String {
    let h = host.trim().to_lowercase();
    match h.as_str() {
        "127.0.0.1" | "::1" | "[::1]" | "0.0.0.0" => "localhost".to_string(),
        _ => h,
    }
}

fn text(t: &toml::Table, key: &str) -> String {
    t.get(key)
        .and_then(toml::Value::as_str)
        .unwrap_or_default()
        .to_string()
}

/// The ids of the saved database connections that are `db`, best first
/// (the order they are saved in).
pub fn matching(db: &Database, profiles: &[ConnectionProfile]) -> Vec<String> {
    let wanted = engines(&db.engine);
    if wanted.is_empty() || db.host.trim().is_empty() {
        return Vec::new();
    }
    profiles
        .iter()
        .filter(|p| p.kind == "db")
        .filter(|p| {
            let Some(t) = p.fields.get("db").and_then(toml::Value::as_table) else {
                return false;
            };
            if !wanted.contains(&text(t, "engine").as_str()) {
                return false;
            }
            if host_key(&text(t, "host")) != host_key(&db.host) {
                return false;
            }
            let port = t
                .get("port")
                .and_then(toml::Value::as_integer)
                .map(|p| p.to_string());
            if let Some(port) = port
                && !db.port.is_empty()
                && port != db.port
            {
                return false;
            }
            let name = text(t, "database");
            name.is_empty() || db.dbname.is_empty() || name.eq_ignore_ascii_case(&db.dbname)
        })
        .map(|p| p.id.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(id: &str, engine: &str, host: &str, port: i64, database: &str) -> ConnectionProfile {
        let mut db = toml::Table::new();
        db.insert("engine".into(), engine.into());
        db.insert("host".into(), host.into());
        db.insert("port".into(), port.into());
        db.insert("database".into(), database.into());
        let mut fields = toml::Table::new();
        fields.insert("db".into(), toml::Value::Table(db));
        ConnectionProfile {
            id: id.into(),
            name: id.into(),
            kind: "db".into(),
            fields,
            ..ConnectionProfile::default()
        }
    }

    fn server(engine: &str, host: &str, port: &str, dbname: &str) -> Database {
        Database {
            id: "2".into(),
            name: "Sales".into(),
            engine: engine.into(),
            native: true,
            host: host.into(),
            port: port.into(),
            dbname: dbname.into(),
        }
    }

    #[test]
    fn one_profile_for_the_same_server_and_database() {
        let profiles = [
            profile("a", "postgres", "db.example.com", 5432, "sales"),
            profile("b", "postgres", "db.example.com", 5432, "other"),
            profile("c", "mysql", "db.example.com", 3306, "sales"),
        ];
        let db = server("postgres", "DB.example.com", "5432", "Sales");
        assert_eq!(matching(&db, &profiles), ["a"], "host and name ignore case");
    }

    #[test]
    fn the_machines_own_names_are_one_host() {
        let profiles = [profile("a", "postgres", "localhost", 5432, "sales")];
        let db = server("postgres", "127.0.0.1", "5432", "sales");
        assert_eq!(matching(&db, &profiles), ["a"]);
    }

    #[test]
    fn a_mysql_server_matches_a_mariadb_profile() {
        let profiles = [profile("a", "mariadb", "h", 3306, "shop")];
        assert_eq!(
            matching(&server("mysql", "h", "3306", "shop"), &profiles),
            ["a"]
        );
    }

    #[test]
    fn a_different_port_is_a_different_server() {
        let profiles = [profile("a", "postgres", "h", 5433, "sales")];
        assert!(matching(&server("postgres", "h", "5432", "sales"), &profiles).is_empty());
    }

    #[test]
    fn several_profiles_for_one_server_are_all_returned() {
        let profiles = [
            profile("a", "postgres", "h", 5432, "sales"),
            profile("b", "postgres", "h", 5432, ""),
        ];
        assert_eq!(
            matching(&server("postgres", "h", "5432", "sales"), &profiles),
            ["a", "b"],
            "a profile with no database name fits any"
        );
    }

    #[test]
    fn a_server_whose_details_are_hidden_matches_nothing() {
        let profiles = [profile("a", "postgres", "h", 5432, "sales")];
        assert!(matching(&server("postgres", "", "", ""), &profiles).is_empty());
        assert!(matching(&server("h2", "h", "1", "x"), &profiles).is_empty());
    }

    #[test]
    fn only_database_profiles_are_candidates() {
        let mut p = profile("a", "postgres", "h", 5432, "sales");
        p.kind = "redis".into();
        assert!(matching(&server("postgres", "h", "5432", "sales"), &[p]).is_empty());
    }
}
