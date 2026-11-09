//! The browse columns of S3, Redis and NoSQL (CR-0091) and the NoSQL
//! document column (CR-0093), driven through the `Tool` trait with real
//! services and a real profile on a throwaway data folder.
//!
//! The columns are egui resizable panels, which no unit test in the tool
//! crates reaches: what is asserted here is what the user sees - a label
//! that lives only in a column is there, gone after the toggle and back
//! after the second one; dragging a column's edge moves the next column's
//! label by the drag and it stays there.
//!
//! The bucket column needs no server (an unreachable endpoint still draws
//! it). The rest need one, so each is skipped unless its variable is set,
//! as the live tests in the tool crates are: `HS_S3_ENDPOINT`
//! (`http://127.0.0.1:9000`, bucket `HS_S3_BUCKET`, default `hyperstack`),
//! `HS_REDIS` (`127.0.0.1:6381`) and `HS_MONGO` (`127.0.0.1:27017`).

use std::sync::Arc;
use std::time::Duration;

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use hs_core::Services;
use hs_core::connections::ConnectionProfile;
use hs_core::paths::DataDir;
use hs_core::runtime::NoopWaker;
use hs_core::secrets::KdfParams;
use hs_core::tool::{Action, Tool, ToolContext, ToolFactory};
use serde_json::{Value, json};

struct Tab<'s> {
    tool: Box<dyn Tool>,
    services: &'s Services,
    /// A palette command to run on the next frame.
    command: Option<String>,
    fonts_ready: bool,
}

fn boot(dir: &std::path::Path) -> crate::bootstrap::Boot {
    let paths = DataDir::resolve(Some(&dir.join("data"))).unwrap();
    crate::bootstrap::services(paths, Arc::new(NoopWaker), KdfParams::FAST).unwrap()
}

/// A saved profile of `kind` whose fields are the table `fields` under the
/// kind's own key (`[s3]`, `[redis]`, `[mongodb]`), as the forms save them.
fn profile(kind: &str, section: &str, fields: toml::Table) -> ConnectionProfile {
    let mut all = toml::Table::new();
    all.insert(section.into(), toml::Value::Table(fields));
    ConnectionProfile {
        name: format!("{kind} columns"),
        kind: kind.into(),
        fields: all,
        ..Default::default()
    }
}

/// The tab restored from `state` (which names the connection, so it
/// connects as a restored tab does), in a fixed-size window.
fn harness<'s>(
    services: &'s Services,
    factory: &dyn ToolFactory,
    state: Value,
) -> Harness<'s, Tab<'s>> {
    let tool = factory.create(services, Some(state));
    Harness::builder()
        .with_size(egui::vec2(1400.0, 800.0))
        .build_ui_state(
            |ui, st: &mut Tab| {
                if !st.fonts_ready {
                    // The tools draw with the named families the app
                    // installs at startup; a fresh context has none.
                    hs_ui::icons::install_fonts(ui.ctx());
                    st.fonts_ready = true;
                    return;
                }
                let services = st.services;
                let mut actions: Vec<Action> = Vec::new();
                let mut cx = ToolContext {
                    services,
                    tab_id: 1,
                    env_id: None,
                    actions: &mut actions,
                    is_home: false,
                };
                if let Some(c) = st.command.take() {
                    assert!(st.tool.run_command(&c, &mut cx), "{c} is a command");
                }
                st.tool.ui(ui, &mut cx);
            },
            Tab {
                tool,
                services,
                command: None,
                fonts_ready: false,
            },
        )
}

/// Runs frames until `done`, giving the tool's runtime tasks time to answer
/// between them: the work is real I/O on another thread.
fn pump(
    h: &mut Harness<'_, Tab<'_>>,
    what: &str,
    mut done: impl FnMut(&Harness<'_, Tab<'_>>) -> bool,
) {
    for _ in 0..400 {
        h.run_steps(2);
        if done(h) {
            return;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    panic!("timed out waiting for {what}");
}

fn command(h: &mut Harness<'_, Tab<'_>>, id: &str) {
    h.state_mut().command = Some(id.into());
    h.run_steps(3);
}

/// How many widgets are labelled exactly `text`.
fn count(h: &Harness<'_, Tab<'_>>, text: &str) -> usize {
    h.query_all_by_label(text).count()
}

/// The rect of the first widget whose label contains `text`.
fn rect_of(h: &Harness<'_, Tab<'_>>, text: &str) -> egui::Rect {
    h.query_all_by_label_contains(text)
        .next()
        .unwrap_or_else(|| panic!("nothing labelled {text:?} is drawn"))
        .rect()
}

/// The x of a panel's resize edge, found as the user finds it: walking the
/// pointer left from `from_x` until the cursor turns into a resize arrow.
fn edge_left_of(h: &mut Harness<'_, Tab<'_>>, from_x: f32, y: f32) -> f32 {
    let mut x = from_x;
    while x > (from_x - 260.0).max(1.0) {
        h.hover_at(egui::pos2(x, y));
        h.step();
        if matches!(
            h.output().platform_output.cursor_icon,
            egui::CursorIcon::ResizeHorizontal
                | egui::CursorIcon::ResizeEast
                | egui::CursorIcon::ResizeWest
        ) {
            return x;
        }
        x -= 1.0;
    }
    panic!("no resize edge within 260 px left of x={from_x}");
}

/// Presses at `from`, moves in a few steps to `to` and lets go there.
fn drag(h: &mut Harness<'_, Tab<'_>>, from: egui::Pos2, to: egui::Pos2) {
    h.hover_at(from);
    h.step();
    h.drag_at(from);
    h.step();
    for i in 1..=6 {
        h.hover_at(from.lerp(to, i as f32 / 6.0));
        h.step();
    }
    // Let go with the pointer still there, as a real mouse does. kittest's
    // `drop_at` also sends `PointerGone` in the same frame, so the panel
    // sees a release with no pointer position and keeps its old size.
    h.event(egui::Event::PointerButton {
        pos: to,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    h.run_steps(3);
    h.remove_cursor();
    h.run_steps(2);
}

/// Drags the edge found left of `probe` (a label of the column the edge
/// belongs to, or of the column beside it) by `dx`, and checks that the
/// label `watch` moved by about `dx` and is still there 30 frames later.
fn drag_edge_and_watch(
    h: &mut Harness<'_, Tab<'_>>,
    probe: &str,
    watch: &str,
    dx: f32,
    what: &str,
) {
    let p = rect_of(h, probe);
    let edge = edge_left_of(h, p.left() - 1.0, p.center().y);
    let before = rect_of(h, watch).left();
    drag(
        h,
        egui::pos2(edge, p.center().y),
        egui::pos2(edge + dx, p.center().y),
    );
    let after = rect_of(h, watch).left();
    assert!(
        ((after - before) - dx).abs() < 12.0,
        "{what}: dragging the edge by {dx} px moved {watch:?} from x={before} to x={after}"
    );
    h.run_steps(30);
    let later = rect_of(h, watch).left();
    assert!(
        (later - after).abs() < 1.0,
        "{what}: the column did not keep its width: {watch:?} went from x={after} to x={later}"
    );
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap()
}

/// An S3 profile for `endpoint` with the test keys every mock accepts. The
/// keys are the S3 mocks' public test values, not a credential.
fn s3_profile(endpoint: &str) -> ConnectionProfile {
    let mut s3 = toml::Table::new();
    for (k, v) in [
        ("provider", "other"),
        ("endpoint", endpoint),
        ("region", "us-east-1"),
        ("access_key", "testing"),
        ("secret_key", "testing"),
    ] {
        s3.insert(k.into(), v.into());
    }
    s3.insert("path_style".into(), true.into());
    profile("s3", "s3", s3)
}

/// The Database schema column, against a SQLite file: its edge drags and
/// the width holds, frame after frame and across a hide and show from
/// `db.schema_column` (BUG-0054). The query tab's title, right of the
/// column, is what moves.
#[test]
fn the_database_schema_column_resizes_and_keeps_its_width() {
    let tmp = tempfile::tempdir().unwrap();
    let boot = boot(tmp.path());
    let s = &boot.services;
    let mut db = toml::Table::new();
    db.insert("engine".into(), "sqlite".into());
    db.insert(
        "path".into(),
        tmp.path()
            .join("cols.db")
            .to_string_lossy()
            .to_string()
            .into(),
    );
    let p = profile("db", "db", db);
    let id = p.id.clone();
    s.connections.write().upsert(p).unwrap();
    let mut h = harness(s, &tool_db::DbFactory, json!({}));
    {
        let st = h.state_mut();
        let mut actions = Vec::new();
        let mut cx = ToolContext {
            services: st.services,
            tab_id: 1,
            env_id: None,
            actions: &mut actions,
            is_home: false,
        };
        assert!(
            st.tool
                .open(hs_core::tool::OpenRequest::Connection { id }, &mut cx)
        );
    }
    let tab = "Untitled";
    pump(&mut h, "the schema column", |h| {
        h.state().tool.current_connection().is_some()
            && h.query_all_by_label_contains(tab).next().is_some()
    });
    h.run_steps(10);
    let start = rect_of(&h, tab).left();
    h.run_steps(30);
    assert!(
        (rect_of(&h, tab).left() - start).abs() < 1.0,
        "the column keeps its width with nothing touched: x={start} became x={}",
        rect_of(&h, tab).left()
    );
    drag_edge_and_watch(&mut h, tab, tab, 120.0, "the schema column");
    let dragged = rect_of(&h, tab).left();
    command(&mut h, "db.schema_column");
    assert!(
        rect_of(&h, tab).left() < dragged - 150.0,
        "the toggle hides the column"
    );
    command(&mut h, "db.schema_column");
    h.run_steps(10);
    let back = rect_of(&h, tab).left();
    assert!(
        (back - dragged).abs() < 1.0,
        "the column comes back at the width it was given: x={back}, was x={dragged}"
    );
}

/// The bucket column hides from `s3.buckets` and comes back. No server:
/// against an endpoint nothing listens on, the column still draws its
/// heading (and the listing's loading or failed state) beside the pane's
/// "pick one" hint. Its drag is tested against a live server
/// (`the_s3_columns_hide_and_resize`): in this state the column's content
/// is narrower than the column, and egui's `Panel` sizes itself to its
/// content, so it sits at its minimum and a drag cannot widen it.
#[test]
fn the_s3_bucket_column_hides_and_shows() {
    let tmp = tempfile::tempdir().unwrap();
    let boot = boot(tmp.path());
    let s = &boot.services;
    let p = s3_profile("http://127.0.0.1:1");
    let id = p.id.clone();
    s.connections.write().upsert(p).unwrap();
    let mut h = harness(s, &tool_s3::S3Factory, json!({ "conn_id": id }));
    let hint = "Pick one on the left";
    pump(&mut h, "the bucket column", |h| {
        count(h, "Buckets") == 1 && h.query_by_label_contains(hint).is_some()
    });
    let shown = rect_of(&h, hint).left();
    assert!(
        rect_of(&h, "Buckets").right() < shown,
        "the bucket column is left of the pane"
    );

    command(&mut h, "s3.buckets");
    assert_eq!(count(&h, "Buckets"), 0, "the toggle hides the column");
    let hidden = rect_of(&h, hint).left();
    assert!(
        hidden < shown - 150.0,
        "the pane takes the column's room: hint at x={hidden}, was x={shown}"
    );
    command(&mut h, "s3.buckets");
    assert_eq!(count(&h, "Buckets"), 1, "the second toggle shows it again");
    assert!((rect_of(&h, hint).left() - shown).abs() < 1.0);
}

/// The leftmost widget whose label contains `text`: a table row's name
/// when the preview column shows the same name further right.
fn leftmost(h: &Harness<'_, Tab<'_>>, text: &str) -> egui::Rect {
    h.query_all_by_label_contains(text)
        .map(|n| n.rect())
        .min_by(|a, b| a.left().total_cmp(&b.left()))
        .unwrap_or_else(|| panic!("nothing labelled {text:?} is drawn"))
}

/// With a live S3 server: the bucket column's edge drags; the object
/// list's Preview and Details columns show beside the list, the preview
/// hides from `s3.preview` and comes back, and both columns' edges drag.
/// Writes one object under a unique prefix and deletes it afterwards.
#[test]
fn the_s3_columns_hide_and_resize() {
    let Ok(endpoint) = std::env::var("HS_S3_ENDPOINT") else {
        eprintln!("HS_S3_ENDPOINT not set; skipping");
        return;
    };
    let bucket = std::env::var("HS_S3_BUCKET").unwrap_or_else(|_| "hyperstack".into());
    let prefix = format!("hstest-cr0091-{}/", hs_core::new_id().to_lowercase());
    let key = format!("{prefix}columns-note.txt");
    let client = hs_core::objstore::s3::S3Client::new(hs_core::objstore::s3::S3Config {
        endpoint: endpoint.clone(),
        region: "us-east-1".into(),
        path_style: true,
        credentials: hs_core::objstore::sigv4::Credentials {
            access_key: "testing".into(),
            secret_key: "testing".into(),
            session_token: None,
        },
    })
    .unwrap();
    let rt = runtime();
    rt.block_on(client.put_bytes(&bucket, &key, b"hello columns\n".to_vec(), "text/plain"))
        .expect("the test object is written");

    let tmp = tempfile::tempdir().unwrap();
    let boot = boot(tmp.path());
    let s = &boot.services;
    let p = s3_profile(&endpoint);
    let id = p.id.clone();
    s.connections.write().upsert(p).unwrap();
    let mut h = harness(
        s,
        &tool_s3::S3Factory,
        json!({ "conn_id": id, "bucket": bucket, "prefix": prefix }),
    );
    pump(&mut h, "the object list", |h| {
        h.query_by_label_contains("columns-note.txt").is_some()
            && h.query_by_label_contains("items loaded").is_some()
            && h.query_by_label_contains("Loading").is_none()
    });
    // The bucket column, with its buckets listed: the object list's status
    // line moves with its edge.
    let status = "items loaded";
    drag_edge_and_watch(&mut h, status, status, 100.0, "S3 bucket column wider");
    drag_edge_and_watch(&mut h, status, status, -50.0, "S3 bucket column narrower");

    h.get_by_label_contains("columns-note.txt").click();
    pump(&mut h, "the preview column", |h| {
        h.query_by_label_contains("hello columns").is_some()
    });
    let row = leftmost(&h, "columns-note.txt");
    let preview = rect_of(&h, "Preview");
    assert!(
        preview.left() > row.right() && preview.top() < row.bottom(),
        "the preview is a column right of the list, not below it: preview {preview:?}, row {row:?}"
    );

    command(&mut h, "s3.preview");
    assert!(
        h.query_by_label_contains("hello columns").is_none(),
        "the toggle hides the preview"
    );
    command(&mut h, "s3.preview");
    pump(&mut h, "the preview again", |h| {
        h.query_by_label_contains("hello columns").is_some()
    });

    command(&mut h, "s3.details");
    pump(&mut h, "the details column", |h| count(h, "Details") == 1);
    // Let the details finish loading before measuring: the column is laid
    // out to its content, which changes when the metadata lands.
    pump(&mut h, "the details", |h| {
        h.query_by_label_contains("Loading").is_none()
    });
    std::thread::sleep(Duration::from_millis(300));
    h.run_steps(30);
    let details = rect_of(&h, "Details");
    assert!(
        details.left() > rect_of(&h, "Preview").left(),
        "Details is the right-most column"
    );

    // The preview's left edge: the preview grows, the list gives way.
    drag_edge_and_watch(&mut h, "Preview", "Preview", -100.0, "S3 preview column");
    // The details' left edge: both columns right of the list move with it.
    let preview_before = rect_of(&h, "Preview").left();
    drag_edge_and_watch(&mut h, "Details", "Details", -80.0, "S3 details column");
    let preview_after = rect_of(&h, "Preview").left();
    assert!(
        ((preview_after - preview_before) + 80.0).abs() < 12.0,
        "the preview moved with the details edge: x={preview_before} -> x={preview_after}"
    );

    drop(h);
    let _ = rt.block_on(client.delete(&bucket, &key));
}

/// With a live Redis: the key list hides from `redis.key_column` and comes
/// back, and its edge drags. The filter matches nothing, so the list shows
/// its empty sentence, which lives only in that column; nothing is written.
#[test]
fn the_redis_key_list_hides_shows_and_resizes() {
    let Ok(spec) = std::env::var("HS_REDIS") else {
        eprintln!("HS_REDIS not set; skipping");
        return;
    };
    let (host, port) = spec.split_once(':').unwrap_or((spec.as_str(), "6379"));
    let mut redis = toml::Table::new();
    redis.insert("host".into(), host.into());
    redis.insert("port".into(), port.parse::<i64>().unwrap().into());
    let tmp = tempfile::tempdir().unwrap();
    let boot = boot(tmp.path());
    let s = &boot.services;
    let p = profile("redis", "redis", redis);
    let id = p.id.clone();
    s.connections.write().upsert(p).unwrap();
    let pattern = format!("hstest:cr0091:{}:*", hs_core::new_id());
    let mut h = harness(
        s,
        &tool_redis::RedisFactory,
        json!({ "conn_id": id, "pattern": pattern }),
    );
    let empty = "No keys matched";
    let value = "Pick a key in the list";
    pump(&mut h, "the key list", |h| {
        h.query_by_label_contains(empty).is_some() && h.query_by_label_contains(value).is_some()
    });
    let shown = rect_of(&h, value).left();
    assert!(
        rect_of(&h, empty).left() < shown,
        "the list is left of the value"
    );

    command(&mut h, "redis.key_column");
    assert!(
        h.query_by_label_contains(empty).is_none(),
        "the toggle hides the key list"
    );
    let hidden = rect_of(&h, value).left();
    assert!(
        hidden < shown - 150.0,
        "the value pane takes the list's room: x={hidden}, was x={shown}"
    );
    command(&mut h, "redis.key_column");
    assert!(
        h.query_by_label_contains(empty).is_some(),
        "the second toggle shows it again"
    );

    drag_edge_and_watch(&mut h, value, value, 120.0, "Redis key list wider");
    drag_edge_and_watch(&mut h, value, value, -80.0, "Redis key list narrower");
}

/// A test document in `hstest.cr0093` on the live MongoDB, removed again by
/// [`mongo_seed`]`(false)`.
fn mongo_seed(host: &str, port: u16, insert: bool) {
    use tool_nosql::store::DocStore;
    let cfg = tool_nosql::mongo::ConnectConfig {
        host: host.into(),
        port,
        auth_source: "admin".into(),
        connect_timeout: Duration::from_secs(5),
        ..Default::default()
    };
    runtime().block_on(async {
        let mut m = tool_nosql::mongo::MongoStore::connect(&cfg)
            .await
            .expect("mongo connects");
        let c = tool_nosql::store::ContainerRef::new("hstest", "cr0093");
        let _ = m.delete_many(&c, r#"{"cr0093": true}"#, false).await;
        if insert {
            let doc = tool_nosql::store::Document::new(
                json!({ "_id": "cr0093-doc", "cr0093": true, "title": "right column" }),
                "cr0093-doc",
                None,
            );
            m.put(&c, &doc, tool_nosql::store::PutMode::Insert)
                .await
                .expect("the test document is written");
        }
    });
}

/// After a result row is clicked: the document's id heads a column right of
/// the row and level with it, and the editor's Save and Delete sit right of
/// the results, not below them; that column's edge drags.
fn the_clicked_document_opens_on_the_right(h: &mut Harness<'_, Tab<'_>>, what: &str) {
    pump(h, "the result row", |h| count(h, "cr0093-doc") == 1);
    let row = rect_of(h, "cr0093-doc");
    h.get_by_label("cr0093-doc").click();
    pump(h, "the document column", |h| {
        count(h, "cr0093-doc") == 2 && h.query_by_label_contains("Save").is_some()
    });
    let title = h
        .query_all_by_label("cr0093-doc")
        .map(|n| n.rect())
        .max_by(|a, b| a.left().total_cmp(&b.left()))
        .unwrap();
    assert!(
        title.left() > row.right(),
        "{what}: the document's title (x={}) is right of its row (right edge x={})",
        title.left(),
        row.right()
    );
    assert!(
        title.top() < row.bottom(),
        "{what}: the document column starts level with the results (y={}), not below the row (y={})",
        title.top(),
        row.bottom()
    );
    for button in ["Save", "Delete"] {
        let b = rect_of(h, button);
        assert!(
            b.left() > row.right(),
            "{what}: {button} (x={}) is right of the results (x={})",
            b.left(),
            row.right()
        );
    }
    drag_edge_and_watch(h, "Save", "Save", -120.0, what);
}

/// With a live MongoDB: the database column hides from `nosql.db_column`
/// and comes back, and its edge drags (CR-0091); a clicked Find result and
/// a clicked Aggregate result open their JSON in a resizable column right
/// of the results (CR-0093). Writes one document in `hstest.cr0093` and
/// deletes it afterwards.
#[test]
fn the_nosql_columns_hide_resize_and_hold_the_open_document() {
    let Ok(spec) = std::env::var("HS_MONGO") else {
        eprintln!("HS_MONGO not set; skipping");
        return;
    };
    let (host, port) = spec.split_once(':').expect("HS_MONGO is host:port");
    let port: u16 = port.parse().unwrap();
    mongo_seed(host, port, true);

    let tmp = tempfile::tempdir().unwrap();
    let boot = boot(tmp.path());
    let s = &boot.services;
    let mut mongo = toml::Table::new();
    mongo.insert("host".into(), host.into());
    mongo.insert("port".into(), i64::from(port).into());
    let p = profile("mongodb", "mongodb", mongo);
    let id = p.id.clone();
    s.connections.write().upsert(p).unwrap();
    let state = json!({ "conn_id": id, "database": "hstest", "container": "cr0093" });
    let mut h = harness(s, &tool_nosql::NoSqlFactory, state.clone());
    let pane = "hstest.cr0093";
    pump(&mut h, "the database column", |h| {
        count(h, "Containers") == 1 && count(h, pane) == 1
    });
    let shown = rect_of(&h, pane).left();
    command(&mut h, "nosql.db_column");
    assert_eq!(count(&h, "Containers"), 0, "the toggle hides the column");
    let hidden = rect_of(&h, pane).left();
    assert!(
        hidden < shown - 150.0,
        "the pane takes the column's room: x={hidden}, was x={shown}"
    );
    command(&mut h, "nosql.db_column");
    assert_eq!(
        count(&h, "Containers"),
        1,
        "the second toggle shows it again"
    );
    drag_edge_and_watch(&mut h, pane, pane, 100.0, "NoSQL database column wider");

    // Find.
    command(&mut h, "nosql.run");
    the_clicked_document_opens_on_the_right(&mut h, "Find");
    drop(h);

    // Aggregate: the same column for a pipeline's result.
    let mut state = state;
    state["tab"] = json!("aggregate");
    state["pipeline"] = json!(r#"[{"$match": {"cr0093": true}}]"#);
    let mut h = harness(s, &tool_nosql::NoSqlFactory, state);
    pump(&mut h, "the Aggregate tab", |h| {
        count(h, "Run") == 1 && count(h, pane) == 1
    });
    h.get_by_label("Run").click();
    the_clicked_document_opens_on_the_right(&mut h, "Aggregate");
    drop(h);

    mongo_seed(host, port, false);
}
