//! The Superset provider against a real Superset, skipped unless
//! `HS_SUPERSET` names one (docs/tools/analytics.md §8.5). Everything here
//! reads: no test saves a chart or changes the server.
//!
//! - `HS_SUPERSET=<url>|<provider>|<user>|<password>` signs in with
//!   `/security/login` (provider `db` or `ldap`)
//! - `HS_SUPERSET_TOKEN=<url>|<access token>` the same by a pasted token (it
//!   lasts about 15 minutes by default: fetch one just before the run)
//!
//! The bench's Superset 4.1 (profile `bi`; `seed/seed-superset.sh`) holds the
//! database `Bench Sales` (a SQLite file with the `sales` table of 400 rows),
//! five charts (table, line, bar, pie, big number) and the dashboard
//! `HyperStack Overview` with a Region select filter. Against another Superset
//! only the shape holds: set the variable and read what fails.

use std::time::Duration;

use hs_core::service_auth::Auth;
use hs_core::service_http::Tls;
use tool_analytics::profile::SupersetConfig;
use tool_analytics::provider::superset::Superset;
use tool_analytics::provider::{BiProvider, Content, ExportFormat, Kind};

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("runtime")
}

fn provider(url: &str, auth: Auth, provider: &str) -> Superset {
    Superset::new(SupersetConfig {
        url: url.into(),
        auth,
        provider: provider.into(),
        tls: Tls::default(),
        timeout: Duration::from_secs(60),
    })
    .expect("client")
}

fn parts(var: &str, n: usize) -> Option<Vec<String>> {
    let v = std::env::var(var).ok()?;
    let p: Vec<String> = v.splitn(n, '|').map(str::to_string).collect();
    (p.len() == n).then_some(p)
}

async fn chart_named(p: &Superset, name: &str) -> tool_analytics::provider::Card {
    let charts = p.children("charts").await.expect("charts");
    let id = charts
        .iter()
        .find(|n| n.name == name)
        .unwrap_or_else(|| panic!("the chart {name}"))
        .id
        .clone();
    p.card(&id).await.expect("card")
}

/// Sign in, then the lists, a chart of each kind, the databases, SQL Lab and
/// the dashboard with its filter.
async fn walk(p: &Superset) {
    let info = p.connect().await.expect("connect");
    assert_eq!(info.product, "Apache Superset");
    assert!(!info.signed_in_as.is_empty());
    let caps = p.capabilities();
    assert!(caps.browse && caps.native_sql && caps.tables);
    assert_eq!(caps.search_kinds, [Kind::Dashboard, Kind::Question]);

    // The two lists the tree shows.
    let root = p.children("root").await.expect("root");
    assert_eq!(
        root.iter().map(|n| n.name.as_str()).collect::<Vec<_>>(),
        ["Dashboards", "Charts"]
    );
    let boards = p.children("dashboards").await.expect("dashboards");
    let overview = boards
        .iter()
        .find(|n| n.name == "HyperStack Overview")
        .expect("the demo dashboard");
    assert_eq!(overview.kind, Kind::Dashboard);
    let charts = p.children("charts").await.expect("charts");
    assert!(charts.len() >= 5 && charts.iter().all(|n| n.kind == Kind::Question));
    assert!(
        p.item_url(Kind::Question, &charts[0].id)
            .contains("/explore/?slice_id=")
    );

    // Search by name, narrowed to a kind.
    let found = p.search("revenue", &[], 0).await.expect("search");
    assert!(found.nodes.iter().any(|n| n.name == "Revenue by month"));
    let only = p
        .search("overview", &[Kind::Dashboard], 0)
        .await
        .expect("dashboards only");
    assert!(!only.nodes.is_empty() && only.nodes.iter().all(|n| n.kind == Kind::Dashboard));

    // A chart of each kind, drawn from its saved query context.
    let line = chart_named(p, "Revenue by month").await;
    assert_eq!(line.display, "line");
    assert!(line.viz.get("query_context").is_some());
    let t = p.run(&line, &[], 1000).await.expect("line");
    assert_eq!(t.rows.len(), 12, "twelve months");
    assert_eq!(
        t.columns
            .iter()
            .map(|c| c.name.as_str())
            .collect::<Vec<_>>(),
        ["month", "Revenue"]
    );
    assert!(t.columns[1].kind.is_number());
    assert!(
        t.sql
            .as_deref()
            .is_some_and(|q| q.to_lowercase().contains("select"))
    );
    let pie = chart_named(p, "Orders by region").await;
    assert_eq!(pie.display, "pie");
    let t = p.run(&pie, &[], 1000).await.expect("pie");
    assert_eq!(t.rows.len(), 4, "four regions");
    let total: f64 = t.rows.iter().filter_map(|r| r[1].as_f64()).sum();
    assert_eq!(total, 400.0, "all 400 orders");
    let big = chart_named(p, "Orders in total").await;
    assert_eq!(big.display, "scalar");
    let t = p.run(&big, &[], 10).await.expect("number");
    assert_eq!((t.rows.len(), t.rows[0][0].as_f64()), (1, Some(400.0)));
    let bar = chart_named(p, "Revenue by category").await;
    assert_eq!(bar.display, "bar");
    let table = chart_named(p, "Sales table").await;
    assert_eq!(table.display, "table");
    let t = p.run(&table, &[], 50).await.expect("table");
    assert_eq!(
        (t.rows.len(), t.truncated),
        (50, true),
        "cut at the run limit"
    );
    let csv = p.export(&bar, &[], ExportFormat::Csv).await.expect("csv");
    assert!(String::from_utf8_lossy(&csv).starts_with("category,Revenue"));
    assert!(p.export(&bar, &[], ExportFormat::Xlsx).await.is_err());

    // Databases, their datasets with columns, and the address Open in Database
    // matches on (none for a file database).
    let dbs = p.databases().await.expect("databases");
    let bench = dbs
        .iter()
        .find(|d| d.name == "Bench Sales")
        .expect("Bench Sales");
    assert_eq!(bench.engine, "sqlite");
    assert!(bench.native, "exposed in SQL Lab");
    assert!(
        bench.host.is_empty() && bench.dbname.contains("bench.db"),
        "{bench:?}"
    );
    let tables = p.tables(&bench.id).await.expect("tables");
    let sales = tables.iter().find(|t| t.name == "sales").expect("sales");
    let cols: Vec<&str> = sales.fields.iter().map(|f| f.name.as_str()).collect();
    assert!(
        cols.contains(&"region") && cols.contains(&"amount"),
        "{cols:?}"
    );
    assert_eq!(
        line.database, bench.id,
        "the chart's database, for Open in Database"
    );

    // SQL Lab reads, and refuses a write before it is sent.
    let rows = p
        .run_sql(
            &bench.id,
            "SELECT region, COUNT(*) AS n FROM sales GROUP BY region ORDER BY region",
            100,
        )
        .await
        .expect("SQL Lab");
    assert_eq!(rows.rows.len(), 4);
    let e = p
        .run_sql(&bench.id, "DELETE FROM sales", 100)
        .await
        .expect_err("refused");
    assert!(e.contains("only reads"), "{e}");

    // The dashboard: its grid, and its Region filter reaching the charts.
    let d = p.dashboard(&overview.id).await.expect("dashboard");
    assert_eq!(d.name, "HyperStack Overview");
    assert_eq!(d.cards.len(), 5);
    let first = d.cards.iter().filter(|c| c.row == 0).count();
    assert_eq!(first, 2, "the big number and the line share the first row");
    assert!(d.cards.iter().all(|c| c.col + c.size_x <= 24));
    assert_eq!(d.params.len(), 1);
    assert_eq!(d.params[0].name, "Region");
    let values = p
        .filter_values(&overview.id, &d.params[0].id)
        .await
        .expect("values");
    assert!(
        values.contains(&"East".to_string()) && values.len() == 4,
        "{values:?}"
    );
    let tile = d
        .cards
        .iter()
        .find(
            |c| matches!(&c.content, Content::Card { card, .. } if card.name == "Orders in total"),
        )
        .expect("the number tile");
    let all = p.run_dashcard(&d, tile, &[], 10).await.expect("unfiltered");
    let east = p
        .run_dashcard(&d, tile, &[(d.params[0].id.clone(), "East".into())], 10)
        .await
        .expect("filtered");
    let (all, east) = (
        all.rows[0][0].as_f64().unwrap(),
        east.rows[0][0].as_f64().unwrap(),
    );
    assert_eq!(all, 400.0);
    assert!(
        east > 0.0 && east < all,
        "the filter reached the chart: {east} of {all}"
    );

    p.disconnect().await;
    assert!(p.children("dashboards").await.is_err(), "the token is gone");
}

#[test]
fn a_password_walks_the_server() {
    let Some(v) = parts("HS_SUPERSET", 4) else {
        return;
    };
    runtime().block_on(async {
        let auth = Auth::Password {
            user: v[2].clone(),
            password: v[3].clone(),
        };
        walk(&provider(&v[0], auth, &v[1])).await;
    });
}

#[test]
fn a_pasted_access_token_walks_the_server() {
    let Some(v) = parts("HS_SUPERSET_TOKEN", 2) else {
        return;
    };
    runtime().block_on(async {
        let auth = Auth::Token {
            user: String::new(),
            token: v[1].clone(),
        };
        walk(&provider(&v[0], auth, "db")).await;
    });
}

#[test]
fn a_wrong_password_is_said_in_words() {
    let Some(v) = parts("HS_SUPERSET", 4) else {
        return;
    };
    runtime().block_on(async {
        let auth = Auth::Password {
            user: v[2].clone(),
            password: "not-the-password".into(),
        };
        let e = provider(&v[0], auth, &v[1])
            .connect()
            .await
            .expect_err("refused");
        assert!(e.contains("refused the sign-in"), "{e}");
    });
}
