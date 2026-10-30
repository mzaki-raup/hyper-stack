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
            // The key is declared at table level, not inline on the column:
            // MySQL parses a column-level REFERENCES and silently ignores
            // it, so an inline one creates no constraint there and the FK
            // badge below never appears.
            "create table {table} ({int_pk}, name varchar(40) not null, price decimal(10,2), \
             note text, raw {blob}, parent_id int, \
             foreign key (parent_id) references {parent}(id))"
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

/// Structured introspection against a real SQLite database (depth programme
/// phase H). The queries that feed the ER diagram, the DDL generator and
/// both compares are the part no unit test can cover, so they run here
/// against a file the test creates.
#[tokio::test(flavor = "multi_thread")]
async fn sqlite_table_schema() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = ConnectConfig {
        engine: Engine::Sqlite,
        path: dir.path().join("h.db").to_string_lossy().into(),
        ..Default::default()
    };
    let mut s = connect(&cfg).await.expect("sqlite opens");
    run(
        &mut s,
        "CREATE TABLE customers (id INTEGER PRIMARY KEY AUTOINCREMENT, email TEXT NOT NULL)",
    )
    .await;
    run(
        &mut s,
        "CREATE TABLE orders (\
           id INTEGER PRIMARY KEY AUTOINCREMENT, \
           customer_id INTEGER NOT NULL REFERENCES customers(id) ON DELETE CASCADE, \
           note TEXT DEFAULT 'none')",
    )
    .await;
    run(
        &mut s,
        "CREATE UNIQUE INDEX customers_email ON customers(email)",
    )
    .await;
    run(
        &mut s,
        "CREATE INDEX orders_customer ON orders(customer_id)",
    )
    .await;
    // A composite key, to check the key keeps its order.
    run(
        &mut s,
        "CREATE TABLE lines (order_id INTEGER, line INTEGER, qty INTEGER, PRIMARY KEY (order_id, line))",
    )
    .await;

    let orders = s.table_schema("main", "orders").await.expect("orders");
    assert_eq!(orders.primary_key, ["id"]);
    let id = orders.column("id").expect("id column");
    assert!(id.auto_increment, "AUTOINCREMENT was not seen: {id:?}");
    // SQLite reports notnull = 0 for a rowid alias; a key column is still
    // recorded as NOT NULL, so it matches what Postgres and MySQL report.
    assert!(!id.nullable, "a primary key column must not be nullable");
    let note = orders.column("note").expect("note column");
    assert!(note.nullable);
    assert_eq!(note.default.as_deref(), Some("'none'"));
    assert!(orders.column("customer_id").is_some_and(|c| !c.nullable));

    assert_eq!(orders.foreign_keys.len(), 1);
    let fk = &orders.foreign_keys[0];
    assert_eq!(fk.columns, ["customer_id"]);
    assert_eq!(fk.ref_table, "customers");
    assert_eq!(fk.ref_columns, ["id"]);
    assert_eq!(fk.on_delete.as_deref(), Some("CASCADE"));
    // SQLite reports NO ACTION for an unspecified update rule.
    assert_eq!(fk.on_update, None);

    let named = orders
        .indexes
        .iter()
        .find(|i| i.name == "orders_customer")
        .expect("the named index is listed");
    assert_eq!(named.columns, ["customer_id"]);
    assert!(!named.unique);
    assert!(!named.primary);

    let customers = s
        .table_schema("main", "customers")
        .await
        .expect("customers");
    let uq = customers
        .indexes
        .iter()
        .find(|i| i.name == "customers_email")
        .expect("the unique index is listed");
    assert!(uq.unique);
    assert_eq!(uq.columns, ["email"]);

    let lines = s.table_schema("main", "lines").await.expect("lines");
    assert_eq!(lines.primary_key, ["order_id", "line"]);
    // A composite key is not an AUTOINCREMENT rowid alias.
    assert!(lines.columns.iter().all(|c| !c.auto_increment));

    // The whole schema, as the ER diagram and the DDL generator read it.
    let (snap, skipped) = tool_db::schema::read(s.as_mut(), "main", 100)
        .await
        .expect("snapshot");
    assert_eq!(skipped, 0);
    let names: Vec<&str> = snap.tables.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, ["customers", "lines", "orders"], "sorted by name");

    // The ERD sees the one relationship, and the generated DDL re-reads.
    let doc = tool_db::erd::build(&snap);
    assert_eq!(doc.nodes.len(), 3);
    assert_eq!(doc.edges.len(), 1);
    let sql = tool_db::ddlgen::script(&snap, Engine::Sqlite, &Default::default());
    assert!(sql.contains("PRIMARY KEY (order_id, line)"), "{sql}");
    assert!(sql.contains("AUTOINCREMENT"), "{sql}");

    // A schema compared with itself has nothing to report.
    assert!(tool_db::compare::schemas(&snap, &snap).is_empty());

    // The cap is honoured and reported.
    let (small, left) = tool_db::schema::read(s.as_mut(), "main", 2)
        .await
        .expect("capped snapshot");
    assert_eq!(small.tables.len(), 2);
    assert_eq!(left, 1);
}

/// What `sqlite_table_schema` checks, against a live PostgreSQL or MySQL
/// server: the structured introspection behind the ER diagram, the DDL
/// generator and both compares (depth programme phase H).
///
/// `schema` is where the tables are made - `public` on PostgreSQL, the
/// connection's own database on MySQL, which has no schemas of its own.
async fn exercise_table_schema(cfg: ConnectConfig, schema: &str) {
    let mut s = connect(&cfg).await.expect("connects");
    let engine = cfg.engine;
    // Drop in dependency order, so a re-run after a failure starts clean.
    for t in ["hs_h_lines", "hs_h_orders", "hs_h_customers"] {
        run(&mut s, &format!("DROP TABLE IF EXISTS {t}")).await;
    }
    let serial = match engine {
        Engine::Postgres => "serial PRIMARY KEY",
        _ => "integer NOT NULL AUTO_INCREMENT PRIMARY KEY",
    };
    run(
        &mut s,
        &format!("CREATE TABLE hs_h_customers (id {serial}, email varchar(64) NOT NULL)"),
    )
    .await;
    run(
        &mut s,
        &format!(
            "CREATE TABLE hs_h_orders (\
               id {serial}, \
               customer_id integer NOT NULL, \
               note varchar(32) DEFAULT 'none', \
               CONSTRAINT hs_h_orders_customer FOREIGN KEY (customer_id) \
                 REFERENCES hs_h_customers (id) ON DELETE CASCADE)"
        ),
    )
    .await;
    run(
        &mut s,
        "CREATE TABLE hs_h_lines (order_id integer NOT NULL, line integer NOT NULL, \
         qty integer, PRIMARY KEY (order_id, line))",
    )
    .await;
    run(
        &mut s,
        "CREATE UNIQUE INDEX hs_h_customers_email ON hs_h_customers (email)",
    )
    .await;
    run(
        &mut s,
        "CREATE INDEX hs_h_orders_customer_ix ON hs_h_orders (customer_id)",
    )
    .await;

    let orders = s
        .table_schema(schema, "hs_h_orders")
        .await
        .expect("orders structure");
    assert_eq!(orders.primary_key, ["id"], "{orders:?}");
    let id = orders.column("id").expect("id");
    assert!(
        id.auto_increment,
        "a {engine:?} generated key should read as auto-increment: {id:?}"
    );
    assert!(!id.nullable);
    // A serial's nextval() default belongs to its sequence, so it is not
    // repeated as a column default.
    assert_eq!(id.default, None, "{id:?}");
    let note = orders.column("note").expect("note");
    assert!(note.nullable);
    // Both servers report the default as a SQL expression, string quotes
    // included - that is what makes the generated DDL re-runnable.
    assert!(
        note.default.as_deref().is_some_and(|d| d.contains("none")),
        "the string default should survive as a literal: {note:?}"
    );
    assert!(
        orders
            .column("customer_id")
            .is_some_and(|c| !c.nullable && !c.data_type.is_empty())
    );

    assert_eq!(orders.foreign_keys.len(), 1, "{:?}", orders.foreign_keys);
    let fk = &orders.foreign_keys[0];
    assert_eq!(fk.columns, ["customer_id"]);
    assert_eq!(fk.ref_table, "hs_h_customers");
    assert_eq!(fk.ref_columns, ["id"]);
    assert_eq!(fk.on_delete.as_deref(), Some("CASCADE"), "{fk:?}");
    assert_eq!(fk.on_update, None, "an unspecified rule is the default");

    let ix = orders
        .indexes
        .iter()
        .find(|i| i.name == "hs_h_orders_customer_ix")
        .unwrap_or_else(|| panic!("named index missing: {:?}", orders.indexes));
    assert_eq!(ix.columns, ["customer_id"]);
    assert!(!ix.unique);
    assert!(!ix.primary);

    let customers = s
        .table_schema(schema, "hs_h_customers")
        .await
        .expect("customers structure");
    let uq = customers
        .indexes
        .iter()
        .find(|i| i.name == "hs_h_customers_email")
        .unwrap_or_else(|| panic!("unique index missing: {:?}", customers.indexes));
    assert!(uq.unique);
    assert_eq!(uq.columns, ["email"]);
    // The key's own index is reported and flagged, so a compare can skip it.
    assert!(
        customers.indexes.iter().any(|i| i.primary),
        "the primary key's index should be listed: {:?}",
        customers.indexes
    );

    // A composite key keeps the order it was declared in - the case that
    // needs WITH ORDINALITY on PostgreSQL and SEQ_IN_INDEX on MySQL.
    let lines = s
        .table_schema(schema, "hs_h_lines")
        .await
        .expect("lines structure");
    assert_eq!(lines.primary_key, ["order_id", "line"], "{lines:?}");

    // The whole schema, then the three things built on it.
    let (snap, _) = tool_db::schema::read(s.as_mut(), schema, 500)
        .await
        .expect("snapshot");
    for t in ["hs_h_customers", "hs_h_lines", "hs_h_orders"] {
        assert!(snap.table(t).is_some(), "{t} missing from the snapshot");
    }
    let ours: Vec<&str> = snap
        .tables
        .iter()
        .map(|t| t.name.as_str())
        .filter(|n| n.starts_with("hs_h_"))
        .collect();
    assert_eq!(
        ours,
        ["hs_h_customers", "hs_h_lines", "hs_h_orders"],
        "sorted by name"
    );

    // The ERD finds the one relationship between our tables.
    let doc = tool_db::erd::build(&snap);
    assert!(
        doc.edges.iter().any(|e| e.from.contains("orders")),
        "the foreign key should be an edge: {:?}",
        doc.edges
    );
    let svg = tool_db::erd::to_svg(&snap);
    assert!(svg.starts_with("<svg") && svg.contains("hs_h_orders"));

    // Generated DDL round-trips in the server's own dialect.
    let sql = tool_db::ddlgen::script(&snap, engine, &Default::default());
    assert!(sql.contains("hs_h_orders"), "{sql}");
    assert!(
        sql.contains("PRIMARY KEY (order_id, line)") || sql.contains("PRIMARY KEY (`order_id`"),
        "the composite key should be written out: {sql}"
    );
    // Postgres and MySQL both add their keys afterwards.
    assert!(sql.contains("ADD CONSTRAINT"), "{sql}");

    // A schema compared with itself reports nothing.
    assert!(tool_db::compare::schemas(&snap, &snap).is_empty());

    for t in ["hs_h_lines", "hs_h_orders", "hs_h_customers"] {
        run(&mut s, &format!("DROP TABLE IF EXISTS {t}")).await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn postgres_table_schema() {
    let Some(cfg) = server("HS_PG", Engine::Postgres) else {
        eprintln!("HS_PG not set; skipping");
        return;
    };
    exercise_table_schema(cfg, "public").await;
}

#[tokio::test(flavor = "multi_thread")]
async fn mysql_table_schema() {
    let Some(cfg) = server("HS_MYSQL", Engine::Mysql) else {
        eprintln!("HS_MYSQL not set; skipping");
        return;
    };
    // MySQL has no schemas: the connection's database is the schema.
    let schema = cfg.database.clone();
    exercise_table_schema(cfg, &schema).await;
}
