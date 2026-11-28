//! The Power BI Report Server provider against a server, skipped unless
//! `HS_POWERBIRS` names one (docs/tools/analytics.md §8.5). Everything here
//! reads.
//!
//! - `HS_POWERBIRS=<url>|<user>|<password>` signs in with HTTP Basic
//!   (`DOMAIN\user` for a domain account); the url is the web portal root,
//!   without `/reports`
//!
//! Report Server is a licensed Windows product, so the bench runs a mock of
//! its REST API v2.0 (`powerbirs-mock`, port 8480; credentials in
//! `bench/.env`) and this test is written to the mock's data: the folders
//! Sales and Operations, two Power BI reports, two paginated reports, two
//! KPIs, two data sources (one with a password in its connection string) and
//! two refresh plans. Against the owner's own server only the shape holds:
//! set the variable and read what fails.

use std::time::Duration;

use hs_core::service_auth::Auth;
use hs_core::service_http::Tls;
use tool_analytics::profile::PowerBiConfig;
use tool_analytics::provider::powerbi::PowerBi;
use tool_analytics::provider::{BiProvider, Kind, Support, ViewFile};

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("runtime")
}

fn provider(url: &str, user: &str, password: &str) -> PowerBi {
    PowerBi::new(PowerBiConfig {
        url: url.into(),
        auth: Auth::Password {
            user: user.into(),
            password: password.into(),
        },
        tls: Tls::default(),
        timeout: Duration::from_secs(30),
    })
    .expect("client")
}

fn parts() -> Option<Vec<String>> {
    let v = std::env::var("HS_POWERBIRS").ok()?;
    let p: Vec<String> = v.splitn(3, '|').map(str::to_string).collect();
    (p.len() == 3).then_some(p)
}

fn names(nodes: &[tool_analytics::provider::Node]) -> Vec<&str> {
    nodes.iter().map(|n| n.name.as_str()).collect()
}

#[test]
fn the_server_is_walked_from_folders_to_kpis_and_plans() {
    let Some(v) = parts() else {
        return;
    };
    runtime().block_on(async {
        let p = provider(&v[0], &v[1], &v[2]);
        let info = p.connect().await.expect("connect");
        assert!(
            info.product.contains("Power BI Report Server"),
            "{}",
            info.product
        );
        assert_eq!(info.support, Support::Supported, "{}", info.version_text);
        assert_eq!(info.signed_in_as, v[1]);
        let caps = p.capabilities();
        assert!(caps.browse && caps.views && caps.kpis && caps.extracts);
        assert!(!caps.native_sql && !caps.tables && !caps.view_filters);
        assert_eq!(caps.view_files, [ViewFile::Image, ViewFile::Csv]);

        // The tree from the paths: folders first, each level by name.
        let root = p.children("root").await.expect("root");
        assert_eq!(names(&root), ["Operations", "Sales"]);
        assert!(root.iter().all(|n| n.kind == Kind::Collection));
        let sales = root.iter().find(|n| n.name == "Sales").expect("Sales");
        let inside = p.children(&sales.id).await.expect("Sales");
        assert_eq!(
            names(&inside),
            [
                "Customer Churn",
                "Regional Sales",
                "Revenue vs Goal",
                "Sales DW",
                "Targets"
            ]
        );
        let kinds: Vec<Kind> = inside.iter().map(|n| n.kind).collect();
        assert_eq!(
            kinds,
            [
                Kind::PowerBi,
                Kind::PowerBi,
                Kind::Kpi,
                Kind::DataSource,
                Kind::DataSource
            ]
        );
        // A data source never shows its password.
        let dw = inside
            .iter()
            .find(|n| n.name == "Sales DW")
            .expect("Sales DW");
        assert!(
            dw.description.contains("Password=********"),
            "{}",
            dw.description
        );
        assert!(!dw.description.contains("hunter2"));
        let ops = root
            .iter()
            .find(|n| n.name == "Operations")
            .expect("Operations");
        let ops_items = p.children(&ops.id).await.expect("Operations");
        assert_eq!(
            names(&ops_items),
            ["Backlog by Warehouse", "Open Backlog", "Order Fulfilment"]
        );

        // Search by name, narrowed to a kind.
        let found = p.search("backlog", &[], 0).await.expect("search");
        assert_eq!(found.total, 2);
        let kpis = p.search("backlog", &[Kind::Kpi], 0).await.expect("kpis");
        assert_eq!(names(&kpis.nodes), ["Open Backlog"]);
        assert!(
            p.item_url(Kind::Report, &ops_items[0].id)
                .contains("/reports/report/Operations/Backlog%20by%20Warehouse")
        );
        assert!(
            p.item_url(Kind::PowerBi, &inside[0].id)
                .contains("/reports/powerbi/Sales/Customer%20Churn")
        );

        // A paginated report: details, its picture, its data; a .pbix has none.
        let report = &ops_items[0];
        let facts = p.view(&report.id).await.expect("details");
        assert_eq!(facts.name, "Backlog by Warehouse");
        let png = p
            .view_file(&report.id, ViewFile::Image, 800, &[])
            .await
            .expect("picture");
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        let t = p.view_data(&report.id, &[], 100).await.expect("data");
        assert_eq!(
            t.columns
                .iter()
                .map(|c| c.name.as_str())
                .collect::<Vec<_>>(),
            ["Warehouse", "Open orders", "Oldest (days)"]
        );
        assert_eq!(t.rows.len(), 3);
        assert!(t.columns[1].kind.is_number());
        assert!(
            p.view_file(&report.id, ViewFile::Pdf, 0, &[])
                .await
                .is_err()
        );
        let e = p
            .view_file(&inside[0].id, ViewFile::Image, 800, &[])
            .await
            .expect_err("a .pbix has no picture");
        assert!(e.contains("could not render"), "{e}");

        // KPIs: value, goal, status, trend.
        let revenue = inside.iter().find(|n| n.kind == Kind::Kpi).expect("a KPI");
        let k = p.kpi(&revenue.id).await.expect("kpi");
        assert_eq!((k.value, k.goal, k.status), (1_284_000.0, 1_500_000.0, 0));
        assert_eq!(k.trend.len(), 4);
        let backlog = p.kpi(&kpis.nodes[0].id).await.expect("kpi");
        assert_eq!(backlog.status, -1, "over its goal of 100");

        // The refresh plans, one failed.
        let plans = p.extracts().await.expect("plans");
        assert_eq!(plans.len(), 2);
        assert!(plans.iter().all(|t| !t.last_run.is_empty()));
        assert_eq!(plans.iter().filter(|t| t.failed == 1).count(), 1);
        assert!(plans.iter().any(|t| t.target == "/Sales/Regional Sales"));
    });
}

#[test]
fn a_wrong_password_is_said_in_words() {
    let Some(v) = parts() else {
        return;
    };
    runtime().block_on(async {
        let e = provider(&v[0], &v[1], "not-the-password")
            .connect()
            .await
            .expect_err("refused");
        assert!(e.contains("refused"), "{e}");
    });
}
