//! The Database tab's phase-H schema tools, driven through the `Tool` trait
//! with real services, a real SQLite database and a real connection profile.
//!
//! The four features are dialogs and a cross-tool hand-over, which unit
//! tests in `tool-db` cannot reach: they need a connected session, the
//! snapshot round trip and the shell's action queue. kittest runs the frames
//! headlessly, so the dialogs are opened, read and clicked here rather than
//! only by hand (`docs/GOTCHAS.md`, "Headless and GUI Testing").

use std::sync::Arc;
use std::time::Duration;

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use hs_core::Services;
use hs_core::connections::ConnectionProfile;
use hs_core::paths::DataDir;
use hs_core::runtime::NoopWaker;
use hs_core::secrets::KdfParams;
use hs_core::tool::{Action, GRAPHICS_MIME, OpenRequest, Tool, ToolContext, ToolFactory};

/// Creates a SQLite database with two related tables and returns its path.
fn make_db(path: &std::path::Path, with_note: bool) {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        let cfg = tool_db::driver::ConnectConfig {
            engine: tool_db::driver::Engine::Sqlite,
            path: path.to_string_lossy().into(),
            ..Default::default()
        };
        let mut s = tool_db::driver::connect(&cfg).await.expect("sqlite opens");
        let note = if with_note { ", note TEXT" } else { "" };
        for sql in [
            "CREATE TABLE customers (id INTEGER PRIMARY KEY AUTOINCREMENT, email TEXT NOT NULL)"
                .to_string(),
            format!(
                "CREATE TABLE orders (id INTEGER PRIMARY KEY AUTOINCREMENT, \
                 customer_id INTEGER NOT NULL REFERENCES customers(id){note})"
            ),
        ] {
            let mut c = tool_db::driver::Collect::default();
            s.execute(&sql, &mut c)
                .await
                .unwrap_or_else(|e| panic!("{sql}: {e}"));
        }
    });
}

/// Runs statements against an existing SQLite file.
fn seed(path: &std::path::Path, statements: &[&str]) {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        let cfg = tool_db::driver::ConnectConfig {
            engine: tool_db::driver::Engine::Sqlite,
            path: path.to_string_lossy().into(),
            ..Default::default()
        };
        let mut s = tool_db::driver::connect(&cfg).await.expect("sqlite opens");
        for sql in statements {
            let mut c = tool_db::driver::Collect::default();
            s.execute(sql, &mut c)
                .await
                .unwrap_or_else(|e| panic!("{sql}: {e}"));
        }
    });
}

/// A saved `db` profile pointing at a SQLite file.
fn profile(name: &str, path: &std::path::Path) -> ConnectionProfile {
    let mut db = toml::Table::new();
    db.insert("engine".into(), "sqlite".into());
    db.insert("path".into(), path.to_string_lossy().to_string().into());
    let mut fields = toml::Table::new();
    fields.insert("db".into(), toml::Value::Table(db));
    ConnectionProfile {
        name: name.into(),
        kind: "db".into(),
        fields,
        ..Default::default()
    }
}

struct Tab<'s> {
    tool: Box<dyn Tool>,
    services: &'s Services,
    actions: Vec<Action>,
    /// A palette command to run on the next frame.
    command: Option<String>,
    fonts_ready: bool,
}

fn harness<'s>(services: &'s Services, conn_id: String) -> Harness<'s, Tab<'s>> {
    harness_with_sql(services, conn_id, "")
}

/// As [`harness`], with the query editor already holding `sql`, which is how
/// a restored tab arrives and what `db.compare_data` reads its table from.
fn harness_with_sql<'s>(
    services: &'s Services,
    conn_id: String,
    sql: &str,
) -> Harness<'s, Tab<'s>> {
    let factory = tool_db::DbFactory;
    let restore = (!sql.is_empty()).then(|| serde_json::json!({ "sql": sql }));
    let mut tool = factory.create(services, restore);
    let mut actions = Vec::new();
    let mut cx = ToolContext {
        services,
        tab_id: 1,
        env_id: None,
        actions: &mut actions,
        is_home: false,
    };
    // Opening the connection is what starts the session.
    assert!(tool.open(OpenRequest::Connection { id: conn_id }, &mut cx));

    Harness::new_ui_state(
        |ui, st: &mut Tab| {
            if !st.fonts_ready {
                // The editor and the icons draw with the named families the
                // app installs at startup; a fresh context has none.
                hs_ui::icons::install_fonts(ui.ctx());
                st.fonts_ready = true;
                return;
            }
            ui.set_width(900.0);
            ui.set_height(640.0);
            let services = st.services;
            let mut actions = std::mem::take(&mut st.actions);
            let mut cx = ToolContext {
                services,
                tab_id: 1,
                env_id: None,
                actions: &mut actions,
                is_home: false,
            };
            if let Some(c) = st.command.take() {
                st.tool.run_command(&c, &mut cx);
            }
            st.tool.ui(ui, &mut cx);
            st.actions = actions;
        },
        Tab {
            tool,
            services,
            actions,
            command: None,
            fonts_ready: false,
        },
    )
}

/// Runs frames until `done` is satisfied, giving the session's own task time
/// to answer between them. The work is real I/O on another thread, so the
/// test waits for it rather than assuming one frame is enough.
fn pump(
    h: &mut Harness<'_, Tab<'_>>,
    what: &str,
    mut done: impl FnMut(&mut Harness<'_, Tab<'_>>) -> bool,
) {
    for _ in 0..200 {
        h.run();
        if done(h) {
            return;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    panic!("timed out waiting for {what}");
}

fn boot(dir: &std::path::Path) -> crate::bootstrap::Boot {
    let paths = DataDir::resolve(Some(&dir.join("data"))).unwrap();
    crate::bootstrap::services(paths, Arc::new(NoopWaker), KdfParams::FAST).unwrap()
}

#[test]
fn the_er_diagram_reaches_the_graphics_tool() {
    let tmp = tempfile::tempdir().unwrap();
    let db = tmp.path().join("shop.db");
    make_db(&db, true);
    let boot = boot(tmp.path());
    let s = &boot.services;
    let p = profile("shop", &db);
    let id = p.id.clone();
    s.connections.write().upsert(p).unwrap();

    let mut h = harness(s, id);
    // Wait for the schema tree's first answer, so the session is up.
    pump(&mut h, "the connection", |h| {
        h.state().tool.current_connection().is_some()
    });
    h.state_mut().command = Some("db.erd".into());

    pump(&mut h, "the diagram hand-over", |h| {
        h.state()
            .actions
            .iter()
            .any(|a| matches!(a, Action::OpenTool { factory_id, .. } if factory_id == "graphics"))
    });

    // The hand-over carries a whole ERD, not a path or a blank document.
    let body = h
        .state()
        .actions
        .iter()
        .find_map(|a| match a {
            Action::OpenTool {
                factory_id,
                request: Some(OpenRequest::Text { mime, body }),
            } if factory_id == "graphics" && mime == GRAPHICS_MIME => Some(body.clone()),
            _ => None,
        })
        .expect("the ER diagram goes out as a graphics Text request");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    let doc: hs_diagram::DiagramDoc =
        serde_json::from_value(v["open_diagram"].clone()).expect("an open_diagram document");
    let mut names: Vec<&str> = doc.nodes.iter().map(|n| n.label.as_str()).collect();
    names.sort();
    assert_eq!(names, ["customers", "orders"]);
    assert_eq!(doc.edges.len(), 1, "the foreign key is one edge");
    // The key column carries its badge, which is what makes it an ERD
    // rather than a box of text.
    let orders = doc.nodes.iter().find(|n| n.label == "orders").unwrap();
    let cols = &orders.erd.as_ref().unwrap().columns;
    assert_eq!(cols[0].name, "id");
    assert_eq!(cols[0].key, hs_diagram::model::ColumnKey::Primary);
    assert_eq!(cols[1].key, hs_diagram::model::ColumnKey::Foreign);
}

#[test]
fn the_ddl_dialog_opens_with_the_schemas_statements() {
    let tmp = tempfile::tempdir().unwrap();
    let db = tmp.path().join("shop.db");
    make_db(&db, true);
    let boot = boot(tmp.path());
    let s = &boot.services;
    let p = profile("shop", &db);
    let id = p.id.clone();
    s.connections.write().upsert(p).unwrap();

    let mut h = harness(s, id);
    pump(&mut h, "the connection", |h| {
        h.state().tool.current_connection().is_some()
    });
    h.state_mut().command = Some("db.ddl".into());
    pump(&mut h, "the DDL dialog", |h| {
        h.query_by_label("Generated DDL").is_some()
    });

    // Its controls are all there and named as the locale file says.
    for label in [
        "Dialect",
        "IF NOT EXISTS",
        "DROP First",
        "Indexes",
        "Foreign Keys",
        "Open in Editor",
    ] {
        assert!(
            h.query_by_label(label).is_some(),
            "the DDL dialog should offer {label}"
        );
    }
    // Toggling an option re-generates without panicking, and the dialog
    // stays up.
    h.get_by_label("DROP First").click();
    h.run();
    assert!(h.query_by_label("Generated DDL").is_some());

    // Open in Editor puts the script in the query editor.
    h.get_by_label("Open in Editor").click();
    h.run();
    let sql = h.state().tool.save_state()["sql"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    assert!(sql.contains("CREATE TABLE orders"), "{sql}");
    assert!(sql.contains("CREATE TABLE customers"), "{sql}");
    assert!(sql.contains("AUTOINCREMENT"), "{sql}");
    // DROP First was ticked before the script was taken.
    assert!(sql.contains("DROP TABLE IF EXISTS"), "{sql}");
    // SQLite has no ALTER TABLE ADD CONSTRAINT, so the key is inline.
    assert!(sql.contains("FOREIGN KEY (customer_id)"), "{sql}");
    assert!(!sql.contains("ADD CONSTRAINT"), "{sql}");
}

#[test]
fn a_schema_compare_reports_the_column_the_target_lacks() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("source.db");
    let target = tmp.path().join("target.db");
    // The target is the same schema without `orders.note`.
    make_db(&source, true);
    make_db(&target, false);
    let boot = boot(tmp.path());
    let s = &boot.services;
    let src = profile("source", &source);
    let src_id = src.id.clone();
    let tgt = profile("target", &target);
    {
        let mut c = s.connections.write();
        c.upsert(src).unwrap();
        c.upsert(tgt).unwrap();
    }

    let mut h = harness(s, src_id);
    pump(&mut h, "the connection", |h| {
        h.state().tool.current_connection().is_some()
    });
    h.state_mut().command = Some("db.compare_schema".into());
    pump(&mut h, "the compare picker", |h| {
        h.query_by_label("Compare Against").is_some()
    });

    // The combo carries its text in a child node rather than its own
    // label, so it is reached by role.
    h.get_by_role(egui::accesskit::Role::ComboBox).click();
    h.run();
    // The other saved profile is offered; this tab's own is not, being the
    // source side of the compare.
    assert!(
        h.query_by_label("source").is_none(),
        "the tab's own connection is the source, so it is not a target"
    );
    h.get_by_label("target").click();
    h.run();
    h.get_by_label("Compare").click();

    // Generate Migration is only drawn once the compare came back with
    // differences, so waiting for it proves the whole round trip.
    pump(&mut h, "the compare result", |h| {
        h.query_by_label("Generate Migration").is_some()
    });
    // The difference is reported from the target's point of view.
    assert!(h.query_by_label("orders.note").is_some());

    // The migration is additive SQL for that column.
    h.get_by_label("Generate Migration").click();
    h.run();
    h.get_by_label("Open in Editor").click();
    h.run();
    let sql = h.state().tool.save_state()["sql"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    assert!(sql.contains("ADD COLUMN note"), "{sql}");
    // Nothing destructive is ever live SQL.
    for line in sql.lines().filter(|l| l.contains("DROP")) {
        assert!(line.trim_start().starts_with("--"), "{line}");
    }
}

#[test]
fn a_data_compare_reports_the_rows_that_differ() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("source.db");
    let target = tmp.path().join("target.db");
    make_db(&source, true);
    make_db(&target, true);
    // Same keys 1 and 2 on both sides: row 1 differs, row 2 matches, and
    // row 3 is only on the source.
    seed(
        &source,
        &["INSERT INTO customers (id, email) VALUES (1, 'a@x'), (2, 'b@x'), (3, 'c@x')"],
    );
    seed(
        &target,
        &["INSERT INTO customers (id, email) VALUES (1, 'CHANGED'), (2, 'b@x')"],
    );

    let boot = boot(tmp.path());
    let s = &boot.services;
    let src = profile("source", &source);
    let src_id = src.id.clone();
    let tgt = profile("target", &target);
    {
        let mut c = s.connections.write();
        c.upsert(src).unwrap();
        c.upsert(tgt).unwrap();
    }

    // `db.compare_data` takes its table from the statement in the editor,
    // the same way Export and the editable grid do.
    let mut h = harness_with_sql(s, src_id, "SELECT * FROM customers");
    pump(&mut h, "the connection", |h| {
        h.state().tool.current_connection().is_some()
    });
    h.state_mut().command = Some("db.compare_data".into());
    pump(&mut h, "the data compare picker", |h| {
        h.query_by_label("Row Limit").is_some()
    });
    h.get_by_role(egui::accesskit::Role::ComboBox).click();
    h.run();
    h.get_by_label("target").click();
    h.run();
    h.get_by_label("Compare").click();

    pump(&mut h, "the data compare result", |h| {
        h.query_by_label("Generate Row Changes").is_some()
    });
    h.get_by_label("Generate Row Changes").click();
    h.run();
    h.get_by_label("Open in Editor").click();
    h.run();
    let sql = h.state().tool.save_state()["sql"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    // Row 3 is only on the source, so the target needs it.
    assert!(sql.contains("INSERT INTO customers"), "{sql}");
    assert!(sql.contains("'c@x'"), "{sql}");
    // Row 1 differs, and only the column that differs is set.
    assert!(
        sql.contains("UPDATE customers SET email = 'a@x' WHERE id = '1'"),
        "{sql}"
    );
    // Row 2 matches on both sides, so it is not mentioned at all.
    assert!(
        !sql.contains("'b@x'"),
        "a matching row needs no statement: {sql}"
    );
    // Nothing destructive is ever live SQL.
    for line in sql.lines().filter(|l| l.contains("DELETE")) {
        assert!(line.trim_start().starts_with("--"), "{line}");
    }
}
