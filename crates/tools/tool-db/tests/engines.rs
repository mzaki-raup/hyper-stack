//! The ADR-030 engines against real servers (FEAT-0068). DuckDB always
//! runs (a temp file); the others run when their variable names a server
//! as `host:port:user:password:database`:
//!
//! - `HS_MSSQL=127.0.0.1:1433:sa:Pass:master`
//! - `HS_CLICKHOUSE=127.0.0.1:8123:default:pw:default`
//! - `HS_ORACLE=127.0.0.1:1521:app:pw:FREEPDB1` (the service name; needs
//!   Oracle's ODBC driver)
//! - `HS_DB2=127.0.0.1:50000:db2inst1:pw:TESTDB` (needs IBM's ODBC driver)
//!
//! Each run makes its tables, reads them back through every catalogue
//! path the tab uses, writes the table's DDL in the engine's own dialect
//! and runs it - the proof the generated DDL is that engine's SQL - and
//! drops what it made.

use std::time::{Duration, Instant};

use tool_db::ddlgen;
use tool_db::driver::{
    Collect, ConnectConfig, DbSession, Engine, ObjectKind, Scope, Value, connect,
};

fn server(var: &str, engine: Engine) -> Option<ConnectConfig> {
    let v = std::env::var(var).ok()?;
    let p: Vec<&str> = v.splitn(5, ':').collect();
    Some(ConnectConfig {
        engine,
        host: p[0].into(),
        port: p[1].parse().expect("port"),
        user: p[2].into(),
        password: Some(p[3].into()),
        database: p.get(4).unwrap_or(&"").to_string(),
        ssl: "disable".into(),
        connect_timeout: Duration::from_secs(30),
        ..Default::default()
    })
}

async fn run(s: &mut Box<dyn DbSession>, sql: &str) -> Collect {
    let mut c = Collect::default();
    s.execute(sql, &mut c)
        .await
        .unwrap_or_else(|e| panic!("{sql}: {e}"));
    c
}

/// Run and ignore failure (a drop of something that may not be there).
async fn quiet(s: &mut Box<dyn DbSession>, sql: &str) {
    let _ = s.execute(sql, &mut Collect::default()).await;
}

/// What differs between the engines in this test.
struct Case {
    /// Unquoted names fold to upper case (Oracle, Db2).
    upper: bool,
    parent: &'static str,
    child: &'static str,
    one: &'static str,
    slow: &'static str,
    /// The schema the tables land in, when it is not the session's.
    schema: Option<&'static str>,
    has_fk: bool,
}

fn case(e: Engine) -> Case {
    match e {
        Engine::ClickHouse => Case {
            upper: false,
            parent: "CREATE TABLE hs_parent (id Int32) ENGINE = MergeTree ORDER BY id",
            child: "CREATE TABLE hs_items (id Int32, name String, price Decimal(10, 2), parent_id Nullable(Int32)) \
                    ENGINE = MergeTree ORDER BY id",
            one: "SELECT 1",
            slow: "SELECT count() FROM numbers(100000000000)",
            schema: None,
            has_fk: false,
        },
        Engine::Mssql => Case {
            upper: false,
            parent: "CREATE TABLE hs_parent (id int PRIMARY KEY)",
            child: "CREATE TABLE hs_items (id int PRIMARY KEY, name varchar(40) NOT NULL, \
                    price decimal(10, 2), parent_id int REFERENCES hs_parent (id))",
            one: "SELECT 1",
            slow: "WAITFOR DELAY '00:00:20'",
            schema: Some("dbo"),
            has_fk: true,
        },
        Engine::Oracle => Case {
            upper: true,
            parent: "CREATE TABLE hs_parent (id int PRIMARY KEY)",
            child: "CREATE TABLE hs_items (id int PRIMARY KEY, name varchar2(40) NOT NULL, \
                    price number(10, 2), parent_id int REFERENCES hs_parent (id))",
            one: "SELECT 1 FROM dual",
            slow: "BEGIN DBMS_SESSION.SLEEP(20); END;",
            schema: None,
            has_fk: true,
        },
        Engine::Db2 => Case {
            upper: true,
            parent: "CREATE TABLE hs_parent (id int NOT NULL PRIMARY KEY)",
            child: "CREATE TABLE hs_items (id int NOT NULL PRIMARY KEY, name varchar(40) NOT NULL, \
                    price decimal(10, 2), parent_id int REFERENCES hs_parent (id))",
            one: "VALUES 1",
            slow: "SELECT COUNT(*) FROM syscat.columns a, syscat.columns b, syscat.columns c",
            schema: None,
            has_fk: true,
        },
        // DuckDB
        _ => Case {
            upper: false,
            parent: "CREATE TABLE hs_parent (id int PRIMARY KEY)",
            child: "CREATE TABLE hs_items (id int PRIMARY KEY, name varchar(40) NOT NULL, \
                    price decimal(10, 2), parent_id int REFERENCES hs_parent (id))",
            one: "SELECT 1",
            slow: "SELECT count(*) FROM range(100000000000) a",
            schema: Some("main"),
            has_fk: true,
        },
    }
}

async fn exercise(cfg: ConnectConfig) {
    let e = cfg.engine;
    let c = case(e);
    let n = |x: &str| {
        if c.upper {
            x.to_uppercase()
        } else {
            x.to_string()
        }
    };
    let mut s = connect(&cfg)
        .await
        .unwrap_or_else(|err| panic!("{e:?}: {err}"));
    assert!(!s.server_info().version.is_empty());
    let schema = c
        .schema
        .map(String::from)
        .unwrap_or_else(|| s.server_info().database.clone());

    quiet(&mut s, "DROP TABLE hs_items_copy").await;
    quiet(&mut s, "DROP TABLE hs_items").await;
    quiet(&mut s, "DROP TABLE hs_parent").await;
    run(&mut s, c.parent).await;
    run(&mut s, c.child).await;
    if e != Engine::ClickHouse {
        run(&mut s, "CREATE INDEX hs_items_name ON hs_items (name)").await;
    }
    run(&mut s, "INSERT INTO hs_parent (id) VALUES (1)").await;
    let values: Vec<String> = (1..=500)
        .map(|i| format!("({i}, 'n{i}', {i}.50, 1)"))
        .collect();
    let mut sink = Collect::default();
    let sum = s
        .execute(
            &format!(
                "INSERT INTO hs_items (id, name, price, parent_id) VALUES {}",
                values.join(", ")
            ),
            &mut sink,
        )
        .await
        .expect("insert");
    if e != Engine::ClickHouse {
        assert_eq!(sum.rows_affected, Some(500), "{e:?}");
    }

    // Typed values and NULL.
    let got = run(
        &mut s,
        "SELECT id, name, price, parent_id FROM hs_items WHERE id = 7",
    )
    .await;
    assert_eq!(got.columns.len(), 4);
    assert_eq!(got.columns[1].name.to_lowercase(), "name");
    let row = &got.rows[0];
    assert_eq!(row[0].display(), "7", "{e:?} {row:?}");
    assert_eq!(row[1], Value::Text("n7".into()));
    assert!(
        row[2].is_numeric(),
        "{e:?}: decimal is numeric: {:?}",
        row[2]
    );
    assert_eq!(row[2].display().trim_end_matches('0'), "7.5", "{e:?}");

    // The row limit stops the read and the session is still usable.
    let mut limited = Collect {
        limit: Some(150),
        ..Default::default()
    };
    let sum = s
        .execute("SELECT * FROM hs_items ORDER BY id", &mut limited)
        .await
        .expect("limited");
    assert!(sum.limited, "{e:?}");
    assert_eq!(limited.rows.len(), 150);
    for _ in 0..3 {
        assert_eq!(run(&mut s, c.one).await.rows.len(), 1, "{e:?}");
    }

    // An error says what was wrong.
    let err = s
        .execute("SELECT * FROM no_such_table_here", &mut Collect::default())
        .await
        .unwrap_err();
    assert!(
        err.message.to_lowercase().contains("no_such_table_here"),
        "{e:?}: {err}"
    );

    // Every catalogue path the tab uses.
    let schemas = s.introspect(&Scope::Schemas).await.expect("schemas");
    assert!(
        schemas.iter().any(|o| o.name == schema),
        "{e:?}: {schema} not in {schemas:?}"
    );
    let tables = s
        .introspect(&Scope::Objects {
            schema: schema.clone(),
            kind: ObjectKind::Table,
        })
        .await
        .expect("tables");
    assert!(
        tables.iter().any(|o| o.name == n("hs_items")),
        "{e:?}: {tables:?}"
    );
    let cols = s
        .introspect(&Scope::Columns {
            schema: schema.clone(),
            table: n("hs_items"),
        })
        .await
        .expect("columns");
    let names: Vec<String> = cols.iter().map(|c| c.name.to_lowercase()).collect();
    assert_eq!(names, ["id", "name", "price", "parent_id"], "{e:?}");
    assert!(
        cols[0].badges.contains(&"PK".to_string()),
        "{e:?}: {:?}",
        cols[0]
    );
    if c.has_fk {
        assert!(
            cols[3].badges.iter().any(|b| b.starts_with("FK → ")),
            "{e:?}: {:?}",
            cols[3]
        );
    }
    let cat = s.catalog().await.expect("catalog");
    assert!(
        cat.tables
            .iter()
            .any(|(_, t, cols)| *t == n("hs_items") && cols.len() == 4),
        "{e:?}"
    );

    // The DDL the generator writes for this engine is SQL it accepts.
    let mut t = s
        .table_schema(&schema, &n("hs_items"))
        .await
        .expect("table_schema");
    assert_eq!(
        t.primary_key
            .iter()
            .map(|k| k.to_lowercase())
            .collect::<Vec<_>>(),
        ["id"],
        "{e:?}"
    );
    if c.has_fk {
        assert_eq!(t.foreign_keys.len(), 1, "{e:?}: {:?}", t.foreign_keys);
        assert_eq!(t.foreign_keys[0].ref_table.to_lowercase(), "hs_parent");
    }
    if e != Engine::ClickHouse {
        assert!(
            t.indexes
                .iter()
                .any(|ix| ix.name.to_lowercase() == "hs_items_name"),
            "{e:?}: {:?}",
            t.indexes
        );
    }
    t.name = n("hs_items_copy");
    let ddl = ddlgen::create_table(
        &t,
        e,
        &ddlgen::Options {
            foreign_keys: false,
            ..Default::default()
        },
    );
    run(&mut s, ddl.trim_end_matches(';')).await;
    let copy = s
        .table_schema(&schema, &n("hs_items_copy"))
        .await
        .expect("copy");
    assert_eq!(copy.columns.len(), 4, "{e:?}: {ddl}");
    assert_eq!(copy.primary_key.len(), 1, "{e:?}: {ddl}");

    // Cancel stops a long statement, and the session (or its replacement)
    // answers the next one.
    let cancel = s.canceller();
    let t = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(1500));
        cancel();
    });
    let started = Instant::now();
    let _ = s.execute(c.slow, &mut Collect::default()).await;
    t.join().expect("cancel thread");
    assert!(
        started.elapsed() < Duration::from_secs(15),
        "{e:?}: cancelled"
    );
    assert_eq!(run(&mut s, c.one).await.rows.len(), 1, "{e:?} after cancel");

    run(&mut s, "DROP TABLE hs_items_copy").await;
    run(&mut s, "DROP TABLE hs_items").await;
    run(&mut s, "DROP TABLE hs_parent").await;
}

#[tokio::test(flavor = "multi_thread")]
async fn duckdb_engine() {
    let dir = tempfile::tempdir().expect("dir");
    exercise(ConnectConfig {
        engine: Engine::DuckDb,
        path: dir.path().join("t.duckdb").to_string_lossy().into(),
        ..Default::default()
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn mssql_engine() {
    let Some(cfg) = server("HS_MSSQL", Engine::Mssql) else {
        eprintln!("HS_MSSQL not set; skipping");
        return;
    };
    exercise(cfg).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn clickhouse_engine() {
    let Some(cfg) = server("HS_CLICKHOUSE", Engine::ClickHouse) else {
        eprintln!("HS_CLICKHOUSE not set; skipping");
        return;
    };
    exercise(cfg).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn oracle_engine() {
    let Some(cfg) = server("HS_ORACLE", Engine::Oracle) else {
        eprintln!("HS_ORACLE not set; skipping");
        return;
    };
    exercise(cfg).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn db2_engine() {
    let Some(cfg) = server("HS_DB2", Engine::Db2) else {
        eprintln!("HS_DB2 not set; skipping");
        return;
    };
    exercise(cfg).await;
}
