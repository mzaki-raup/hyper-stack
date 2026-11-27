//! The Metabase provider against a real server, skipped unless
//! `HS_METABASE` names one (docs/tools/analytics.md §8.5):
//!
//! - `HS_METABASE=<url>|<api-key>` - the bench's Metabase (profile `bi`,
//!   `seed/seed-metabase.sh` saves the key in `bench/.state/metabase-api-key`)
//! - `HS_METABASE_USER=<email>|<password>` - optional; the same walk by a
//!   session
//!
//! Everything here reads. The seeded collection `HyperStack Demo` holds nine
//! questions and the dashboard `HyperStack Overview`.

use std::time::Duration;

use hs_core::service_auth::Auth;
use hs_core::service_http::Tls;
use tool_analytics::chart::{self, Plan, SeriesKind};
use tool_analytics::profile::MetabaseConfig;
use tool_analytics::provider::metabase::Metabase;
use tool_analytics::provider::{BiProvider, Content, ExportFormat, Kind, Support};

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("runtime")
}

fn provider(url: &str, auth: Auth) -> Metabase {
    Metabase::new(MetabaseConfig {
        url: url.into(),
        auth,
        tls: Tls::default(),
        timeout: Duration::from_secs(30),
    })
    .expect("client")
}

/// Sign in, then walk the tree and search it.
async fn walk(p: &Metabase, personal: bool) {
    let info = p.connect().await.expect("connect");
    assert_eq!(info.product, "Metabase");
    assert_eq!(info.support, Support::Supported, "{}", info.version_text);
    assert!(!info.signed_in_as.is_empty());

    let root = p.children("root").await.expect("root");
    let demo = root
        .iter()
        .find(|n| n.kind == Kind::Collection && n.name == "HyperStack Demo")
        .expect("the seeded collection is at the root");
    // Collections come first, then the rest, each by name.
    let first_other = root.iter().position(|n| n.kind != Kind::Collection);
    if let Some(i) = first_other {
        assert!(root[..i].iter().all(|n| n.kind == Kind::Collection));
    }
    // An API key's user has no personal collection; a person has one.
    assert_eq!(
        root.iter().any(|n| n.name.contains("Personal Collection")),
        personal,
        "the account's own collection is listed for a person only"
    );

    let inside = p.children(&demo.id).await.expect("the demo collection");
    let questions = inside.iter().filter(|n| n.kind == Kind::Question).count();
    assert!(questions >= 9, "nine seeded questions, found {questions}");
    assert!(
        inside
            .iter()
            .any(|n| n.kind == Kind::Dashboard && n.name == "HyperStack Overview")
    );

    // Search: by name, then narrowed to one kind.
    let found = p.search("orders", &[], 0).await.expect("search");
    assert!(found.total >= 3, "{} results", found.total);
    assert!(found.nodes.iter().any(|n| n.kind == Kind::Question));
    let boards = p
        .search("overview", &[Kind::Dashboard], 0)
        .await
        .expect("dashboards only");
    assert!(!boards.nodes.is_empty());
    assert!(boards.nodes.iter().all(|n| n.kind == Kind::Dashboard));
    assert!(
        boards
            .nodes
            .iter()
            .any(|n| n.collection == "HyperStack Demo"),
        "a result names its collection"
    );
    let nothing = p
        .search("zzz-no-such-thing-zzz", &[], 0)
        .await
        .expect("an empty search is not an error");
    assert!(nothing.nodes.is_empty());

    assert!(p.item_url(Kind::Dashboard, "9").ends_with("/dashboard/9"));
    p.disconnect().await;
}

#[test]
fn an_api_key_walks_the_tree_and_searches() {
    let Ok(spec) = std::env::var("HS_METABASE") else {
        return;
    };
    let (url, key) = spec.split_once('|').expect("HS_METABASE=<url>|<api-key>");
    let p = provider(
        url,
        Auth::ApiKey {
            header: tool_analytics::profile::API_KEY_HEADER.into(),
            key: key.into(),
        },
    );
    runtime().block_on(walk(&p, false));
}

#[test]
fn a_session_walks_the_tree_and_searches() {
    let (Ok(spec), Ok(login)) = (
        std::env::var("HS_METABASE"),
        std::env::var("HS_METABASE_USER"),
    ) else {
        return;
    };
    let (url, _) = spec.split_once('|').expect("HS_METABASE=<url>|<api-key>");
    let (user, password) = login
        .split_once('|')
        .expect("HS_METABASE_USER=<email>|<password>");
    let p = provider(
        url,
        Auth::Password {
            user: user.into(),
            password: password.into(),
        },
    );
    runtime().block_on(walk(&p, true));
}

#[test]
fn a_wrong_key_is_refused_in_words() {
    let Ok(spec) = std::env::var("HS_METABASE") else {
        return;
    };
    let (url, _) = spec.split_once('|').expect("HS_METABASE=<url>|<api-key>");
    let p = provider(
        url,
        Auth::ApiKey {
            header: tool_analytics::profile::API_KEY_HEADER.into(),
            key: "mb_not-a-key".into(),
        },
    );
    let err = runtime().block_on(p.connect()).expect_err("a bad key");
    assert!(!err.is_empty());
}

/// Every seeded question runs, and each kind of chart gets the plan it
/// should; the native question takes its parameter; the export is a file.
async fn questions(p: &Metabase) {
    p.connect().await.expect("connect");
    let root = p.children("root").await.expect("root");
    let demo = root
        .iter()
        .find(|n| n.name == "HyperStack Demo")
        .expect("the seeded collection");
    let inside = p.children(&demo.id).await.expect("the demo collection");
    let id_of = |name: &str| {
        inside
            .iter()
            .find(|n| n.name == name)
            .unwrap_or_else(|| panic!("{name} is seeded"))
            .id
            .clone()
    };
    let run = |name: &'static str| {
        let id = id_of(name);
        async move {
            let card = p.card(&id).await.expect("the card");
            let table = p.run(&card, &[], 2000).await.expect("a result");
            let plan = chart::plan(&card, &table);
            (card, table, plan)
        }
    };

    let (card, table, plan) = run("Orders table").await;
    assert_eq!(card.display, "table");
    assert_eq!(plan, Plan::Table(chart::Reason::IsTable));
    assert!(table.rows.len() > 10 && table.columns.len() > 3);
    assert!(
        table
            .sql
            .as_deref()
            .is_some_and(|q| q.to_lowercase().contains("select"))
    );

    let (_, table, plan) = run("Orders per month").await;
    let Plan::Series(sp) = plan else {
        panic!("a line is a series");
    };
    assert_eq!(sp.series[0].kind, SeriesKind::Line);
    assert_eq!(sp.series[0].points.len(), table.rows.len());
    assert!(
        sp.x_labels
            .as_ref()
            .is_some_and(|l| l.len() == table.rows.len())
    );

    let (_, _, plan) = run("Orders per month (area)").await;
    assert!(matches!(&plan, Plan::Series(sp) if sp.series[0].kind == SeriesKind::Area));
    let (_, _, plan) = run("Orders by category").await;
    assert!(
        matches!(&plan, Plan::Series(sp) if sp.series[0].kind == SeriesKind::Bar && !sp.horizontal)
    );
    let (_, _, plan) = run("Orders by category (row)").await;
    assert!(matches!(&plan, Plan::Series(sp) if sp.horizontal));
    let (_, _, plan) = run("Share of orders by category").await;
    let Plan::Pie(slices) = plan else {
        panic!("a pie");
    };
    assert!((slices.iter().map(|s| s.percent).sum::<f64>() - 100.0).abs() < 1e-6);
    let (_, _, plan) = run("Orders in total").await;
    let Plan::Scalar(sc) = plan else {
        panic!("a scalar");
    };
    assert!(
        sc.value.parse::<i64>().is_ok_and(|n| n > 1000),
        "{}",
        sc.value
    );
    let (_, _, plan) = run("Revenue per month").await;
    assert!(matches!(&plan, Plan::Series(sp) if sp.series.len() == 2));

    // The native question: a parameter with a default, and a bigger minimum
    // leaves fewer rows.
    let (card, default_run, _) = run("Big orders (native)").await;
    assert_eq!(card.params.len(), 1);
    assert_eq!(card.params[0].default, "100");
    assert!(
        card.native_sql
            .as_deref()
            .is_some_and(|q| q.contains("min_total"))
    );
    let id = card.params[0].id.clone();
    let strict = p
        .run(&card, &[(id.clone(), "400".into())], 2000)
        .await
        .expect("with a parameter");
    assert!(
        strict.rows.len() < default_run.rows.len(),
        "{} vs {}",
        strict.rows.len(),
        default_run.rows.len()
    );
    let err = p
        .run(&card, &[(id, "lots".into())], 2000)
        .await
        .expect_err("a word is not a number");
    assert!(err.contains("number"), "{err}");

    // A limit cuts and says so.
    let card = p.card(&id_of("Orders table")).await.expect("the card");
    let cut = p.run(&card, &[], 5).await.expect("a short run");
    assert_eq!(cut.rows.len(), 5);
    assert!(cut.truncated);

    // The export is the server's own file.
    let csv = p
        .export(&card, &[], ExportFormat::Csv)
        .await
        .expect("a CSV");
    let text = String::from_utf8_lossy(&csv);
    assert!(text.lines().count() > 10, "{} lines", text.lines().count());
    p.disconnect().await;
}

#[test]
fn questions_run_and_draw_by_api_key() {
    let Ok(spec) = std::env::var("HS_METABASE") else {
        return;
    };
    let (url, key) = spec.split_once('|').expect("HS_METABASE=<url>|<api-key>");
    let p = provider(
        url,
        Auth::ApiKey {
            header: tool_analytics::profile::API_KEY_HEADER.into(),
            key: key.into(),
        },
    );
    runtime().block_on(questions(&p));
}

/// The seeded dashboard: two tabs, a Category filter, tiles that run, and a
/// filter that narrows the tiles it is mapped to and no others.
async fn dashboards(p: &Metabase) {
    p.connect().await.expect("connect");
    let root = p.children("root").await.expect("root");
    let demo = root
        .iter()
        .find(|n| n.name == "HyperStack Demo")
        .expect("the seeded collection");
    let inside = p.children(&demo.id).await.expect("the demo collection");
    let board = inside
        .iter()
        .find(|n| n.kind == Kind::Dashboard && n.name == "HyperStack Overview")
        .expect("the seeded dashboard");
    let d = p.dashboard(&board.id).await.expect("the dashboard");
    assert_eq!(
        d.tabs.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(),
        ["Overview", "Detail"]
    );
    assert_eq!(d.params.len(), 1);
    assert_eq!(d.params[0].name, "Category");
    assert_eq!(d.cards.len(), 5);
    assert!(
        d.cards.iter().all(|c| c.tab.is_some()),
        "every tile is on a tab"
    );

    let values = p
        .filter_values(&d.id, &d.params[0].id)
        .await
        .expect("the filter's values");
    assert!(values.iter().any(|v| v == "Gadget"), "{values:?}");

    // Every tile runs.
    for c in &d.cards {
        let Content::Card { card, .. } = &c.content else {
            continue;
        };
        let t = p.run_dashcard(&d, c, &[], 2000).await.expect("a tile");
        assert!(!t.columns.is_empty(), "{} has columns", card.name);
    }

    // The category filter reaches the bar chart (mapped) and not the line.
    let by_name = |name: &str| {
        d.cards
            .iter()
            .find(|c| matches!(&c.content, Content::Card { card, .. } if card.name == name))
            .unwrap_or_else(|| panic!("{name} is on the dashboard"))
    };
    let filter = [(d.params[0].id.clone(), "Gadget".to_string())];
    let bar = by_name("Orders by category");
    let all = p
        .run_dashcard(&d, bar, &[], 2000)
        .await
        .expect("unfiltered");
    let one = p
        .run_dashcard(&d, bar, &filter, 2000)
        .await
        .expect("filtered");
    assert_eq!(all.rows.len(), 4);
    assert_eq!(one.rows.len(), 1, "{:?}", one.rows);
    assert_eq!(one.rows[0][0], "Gadget");
    let line = by_name("Orders per month");
    let plain = p.run_dashcard(&d, line, &[], 2000).await.expect("line");
    let same = p
        .run_dashcard(&d, line, &filter, 2000)
        .await
        .expect("line with filter");
    assert_eq!(plain.rows, same.rows, "an unmapped tile ignores the filter");
    p.disconnect().await;
}

#[test]
fn a_dashboard_runs_its_tiles_and_its_filter_narrows_them() {
    let Ok(spec) = std::env::var("HS_METABASE") else {
        return;
    };
    let (url, key) = spec.split_once('|').expect("HS_METABASE=<url>|<api-key>");
    let p = provider(
        url,
        Auth::ApiKey {
            header: tool_analytics::profile::API_KEY_HEADER.into(),
            key: key.into(),
        },
    );
    runtime().block_on(dashboards(&p));
}

/// The server's databases, a database's tables, and ad-hoc SQL: a result, a
/// refusal in the server's words, and the row limit.
async fn sql(p: &Metabase) {
    p.connect().await.expect("connect");
    let dbs = p.databases().await.expect("the databases");
    let sample = dbs
        .iter()
        .find(|d| d.name == "Sample Database")
        .expect("the built-in sample database");
    assert!(sample.native, "an admin may run SQL on it");
    assert_eq!(sample.engine, "h2");

    let tables = p.tables(&sample.id).await.expect("its tables");
    let orders = tables
        .iter()
        .find(|t| t.name == "ORDERS")
        .expect("the ORDERS table");
    assert!(orders.fields.iter().any(|f| f.name == "TOTAL"));
    assert!(tables.len() >= 4, "{} tables", tables.len());

    let count = p
        .run_sql(&sample.id, "SELECT COUNT(*) AS n FROM ORDERS", 2000)
        .await
        .expect("a count");
    assert_eq!(count.rows.len(), 1);
    assert_eq!(count.columns[0].name.to_lowercase(), "n");
    assert!(
        count.rows[0][0].as_i64().is_some_and(|n| n > 1000),
        "{:?}",
        count.rows
    );

    // A wrong statement comes back as the server's sentence, not an empty table.
    let bad = p
        .run_sql(&sample.id, "SELECT * FROM NO_SUCH_TABLE", 2000)
        .await
        .expect_err("a table that does not exist");
    assert!(bad.to_uppercase().contains("NO_SUCH_TABLE"), "{bad}");
    // Nothing to run is said before anything is sent.
    assert!(p.run_sql(&sample.id, "  ", 10).await.is_err());

    let cut = p
        .run_sql(&sample.id, "SELECT ID FROM ORDERS", 7)
        .await
        .expect("a short read");
    assert_eq!(cut.rows.len(), 7);
    assert!(cut.truncated);
    p.disconnect().await;
}

#[test]
fn databases_tables_and_ad_hoc_sql_run_on_the_sample_database() {
    let Ok(spec) = std::env::var("HS_METABASE") else {
        return;
    };
    let (url, key) = spec.split_once('|').expect("HS_METABASE=<url>|<api-key>");
    let p = provider(
        url,
        Auth::ApiKey {
            header: tool_analytics::profile::API_KEY_HEADER.into(),
            key: key.into(),
        },
    );
    runtime().block_on(sql(&p));
}
