//! The three drivers against real servers. SQLite always runs (a temp file);
//! PostgreSQL and MySQL/MariaDB run when `HS_PG` / `HS_MYSQL` are set to
//! `host:port:user:password:database`, for example
//! `HS_PG=127.0.0.1:5432:postgres:pg:postgres HS_MYSQL=127.0.0.1:3306:dev:dev:shop`.

use std::time::{Duration, Instant};

use tool_db::driver::{
    Collect, ConnectConfig, DbSession, Engine, ObjectKind, Scope, Value, connect,
};

fn server(var: &str, engine: Engine) -> Option<ConnectConfig> {
    let v = std::env::var(var).ok()?;
    let p: Vec<&str> = v.splitn(5, ':').collect();
    Some(ConnectConfig {
        engine,
        host: p[0].into(),
        port: p[1].parse().unwrap(),
        user: p[2].into(),
        password: Some(p[3].into()),
        database: p.get(4).unwrap_or(&"").to_string(),
        ssl: "prefer".into(),
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

async fn exercise(cfg: ConnectConfig, table: &str) {
    let e = cfg.engine;
    let mut s = connect(&cfg).await.expect("connect");
    assert!(!s.server_info().version.is_empty());
    let int_pk = match e {
        Engine::Postgres => "id serial primary key",
        Engine::Mysql => "id int auto_increment primary key",
        Engine::Sqlite => "id integer primary key",
    };
    let blob = match e {
        Engine::Postgres => "bytea",
        Engine::Mysql => "varbinary(16)",
        Engine::Sqlite => "blob",
    };
    let parent = format!("{table}_parent");
    run(&mut s, &format!("drop table if exists {table}")).await;
    run(&mut s, &format!("drop table if exists {parent}")).await;
    run(
        &mut s,
        &format!("create table {parent} (id int primary key)"),
    )
    .await;
    run(&mut s, &format!("insert into {parent} values (1)")).await;
    run(
        &mut s,
        &format!(
            "create table {table} ({int_pk}, name varchar(40) not null, price decimal(10,2), \
             note text, raw {blob}, parent_id int references {parent}(id))"
        ),
    )
    .await;
    run(
        &mut s,
        &format!("create index {table}_name on {table} (name)"),
    )
    .await;
    // 1,000 rows in one statement.
    let values: Vec<String> = (1..=1000)
        .map(|i| format!("('n{i}', {i}.50, null, null, 1)"))
        .collect();
    let mut c = Collect::default();
    let sum = s
        .execute(
            &format!(
                "insert into {table} (name, price, note, raw, parent_id) values {}",
                values.join(",")
            ),
            &mut c,
        )
        .await
        .unwrap();
    assert_eq!(sum.rows_affected, Some(1000));

    // Typed values.
    let hex = match e {
        Engine::Postgres => "'\\x00ff'::bytea",
        _ => "x'00ff'",
    };
    run(
        &mut s,
        &format!("insert into {table} (name, price, note, raw) values ('bin', 2, 'it''s', {hex})"),
    )
    .await;
    let c = run(
        &mut s,
        &format!("select id, name, price, note, raw from {table} where name = 'bin'"),
    )
    .await;
    assert_eq!(c.columns.len(), 5);
    assert_eq!(c.columns[1].name, "name");
    let row = &c.rows[0];
    assert!(matches!(row[0], Value::Int(1001)), "{:?}", row[0]);
    assert_eq!(row[1], Value::Text("bin".into()));
    assert!(row[2].is_numeric(), "decimal is numeric: {:?}", row[2]);
    assert_eq!(row[3], Value::Text("it's".into()));
    match e {
        // bytea arrives in its text form
        Engine::Postgres => assert_eq!(row[4], Value::Text("\\x00ff".into())),
        _ => assert_eq!(row[4], Value::Bytes(vec![0, 255])),
    }
    let c = run(&mut s, &format!("select note from {table} where id = 1")).await;
    assert_eq!(c.rows[0][0], Value::Null);

    // Row limit: stop early, the session stays usable, and the next
    // statement is not hit by a late cancel.
    let mut c = Collect {
        limit: Some(150),
        ..Default::default()
    };
    let sum = s
        .execute(&format!("select * from {table} order by id"), &mut c)
        .await
        .unwrap();
    assert!(sum.limited);
    assert_eq!(c.rows.len(), 150);
    for _ in 0..5 {
        let c = run(&mut s, "select 1").await;
        assert_eq!(c.rows.len(), 1);
    }

    // Transactions.
    run(&mut s, "begin").await;
    assert!(s.in_transaction());
    run(&mut s, &format!("delete from {table} where id <= 10")).await;
    run(&mut s, "rollback").await;
    assert!(!s.in_transaction());
    let c = run(&mut s, &format!("select count(*) from {table}")).await;
    assert_eq!(c.rows[0][0].display(), "1001");

    // Errors.
    let mut c = Collect::default();
    let err = s
        .execute("select * from no_such_table_here", &mut c)
        .await
        .unwrap_err();
    assert!(
        err.message.to_lowercase().contains("no_such_table_here"),
        "{err}"
    );
    if e == Engine::Postgres {
        assert_eq!(err.position, Some(14));
        assert_eq!(err.code.as_deref(), Some("42P01"));
    }

    // Schema browser.
    let schemas = s.introspect(&Scope::Schemas).await.unwrap();
    assert!(!schemas.is_empty());
    let schema = match e {
        Engine::Postgres => "public".to_string(),
        Engine::Mysql => cfg.database.clone(),
        Engine::Sqlite => "main".to_string(),
    };
    assert!(schemas.iter().any(|o| o.name == schema), "{schemas:?}");
    let tables = s
        .introspect(&Scope::Objects {
            schema: schema.clone(),
            kind: ObjectKind::Table,
        })
        .await
        .unwrap();
    assert!(tables.iter().any(|o| o.name == table), "{tables:?}");
    let cols = s
        .introspect(&Scope::Columns {
            schema: schema.clone(),
            table: table.into(),
        })
        .await
        .unwrap();
    let names: Vec<&str> = cols.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["id", "name", "price", "note", "raw", "parent_id"]);
    assert!(cols[0].badges.contains(&"PK".to_string()), "{:?}", cols[0]);
    assert!(cols[1].badges.contains(&"NOT NULL".to_string()));
    assert!(
        cols[5]
            .badges
            .iter()
            .any(|b| b.starts_with("FK → ") && b.contains(&parent)),
        "{:?}",
        cols[5]
    );
    let idx = s
        .introspect(&Scope::Indexes {
            schema: schema.clone(),
            table: table.into(),
        })
        .await
        .unwrap();
    assert!(
        idx.iter()
            .any(|i| i.name == format!("{table}_name") && i.detail.contains("name")),
        "{idx:?}"
    );
    let cat = s.catalog().await.unwrap();
    assert!(
        cat.tables
            .iter()
            .any(|(_, t, cols)| t == table && cols.len() == 6),
        "{cat:?}"
    );

    // Cancel a long statement from another task.
    let slow = match e {
        Engine::Postgres => "select pg_sleep(20)".to_string(),
        Engine::Mysql => "select sleep(20)".to_string(),
        Engine::Sqlite => {
            "with recursive r(i) as (select 1 union all select i + 1 from r) select count(*) from r"
                .to_string()
        }
    };
    let cancel = s.canceller();
    let t = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(400));
        cancel();
    });
    let started = Instant::now();
    let mut c = Collect::default();
    let _ = s.execute(&slow, &mut c).await;
    t.join().unwrap();
    assert!(started.elapsed() < Duration::from_secs(10), "cancelled");
    let c = run(&mut s, "select 2").await;
    assert_eq!(c.rows[0][0].display(), "2");

    run(&mut s, &format!("drop table {table}")).await;
    run(&mut s, &format!("drop table {parent}")).await;

    // Read-only sessions refuse writes on the server side.
    let mut ro = cfg.clone();
    ro.read_only = true;
    let mut r = connect(&ro).await.unwrap();
    let mut c = Collect::default();
    let err = r.execute("create table hs_ro_probe (x int)", &mut c).await;
    assert!(err.is_err(), "read-only session accepted DDL");
}

#[tokio::test(flavor = "multi_thread")]
async fn sqlite_driver() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.db");
    // SQLite enforces nothing about FKs by default but reports them.
    let cfg = ConnectConfig {
        engine: Engine::Sqlite,
        path: path.to_string_lossy().into(),
        ..Default::default()
    };
    exercise(cfg, "hs_items").await;
}

#[tokio::test(flavor = "multi_thread")]
async fn postgres_driver() {
    let Some(cfg) = server("HS_PG", Engine::Postgres) else {
        eprintln!("HS_PG not set; skipping");
        return;
    };
    exercise(cfg, "hs_items").await;
}

#[tokio::test(flavor = "multi_thread")]
async fn mysql_driver() {
    let Some(cfg) = server("HS_MYSQL", Engine::Mysql) else {
        eprintln!("HS_MYSQL not set; skipping");
        return;
    };
    exercise(cfg, "hs_items").await;
}

/// With `HS_PG_TLS=1` the server has SSL on (a self-signed certificate):
/// `require` encrypts without checking it, `verify-full` refuses it.
#[tokio::test(flavor = "multi_thread")]
async fn postgres_tls_modes() {
    let Some(mut cfg) = server("HS_PG", Engine::Postgres) else {
        return;
    };
    if std::env::var("HS_PG_TLS").is_err() {
        eprintln!("HS_PG_TLS not set; skipping");
        return;
    }
    cfg.ssl = "require".into();
    let mut s = connect(&cfg).await.expect("require connects");
    let c = run(
        &mut s,
        "select ssl from pg_stat_ssl where pid = pg_backend_pid()",
    )
    .await;
    assert_eq!(c.rows[0][0].display(), "t");
    cfg.ssl = "verify-full".into();
    let err = connect(&cfg).await.err().expect("self-signed is refused");
    assert!(!err.message.is_empty());
}
