//! The SAS Viya provider against a server, skipped unless `HS_SASVIYA` names
//! one (docs/tools/analytics.md §8.5). Everything here reads.
//!
//! - `HS_SASVIYA=<url>|<client id>|<client secret>|<user>|<password>` signs in
//!   through SAS Logon's password grant (the secret is empty for `sas.cli`)
//! - `HS_SASVIYA_TOKEN=<url>|<access token>` the same by a pasted token
//!
//! Viya is licensed, so the bench runs a mock of its REST services
//! (`sasviya-mock`, port 8380; credentials in `bench/.env`) and this test is
//! written to the mock's data: the folders Public > Sales and My Folder, the
//! reports Revenue by Region and Customer Churn, the CAS libraries Public and
//! Samples with the tables ORDERS, CUSTOMERS and CARS, and two jobs. Against
//! the owner's own Viya only the shape holds: set the variable and read what
//! fails.

use std::time::Duration;

use hs_core::service_auth::Auth;
use hs_core::service_http::Tls;
use tool_analytics::profile::SasViyaConfig;
use tool_analytics::provider::sasviya::SasViya;
use tool_analytics::provider::{BiProvider, Kind, ViewFile};

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("runtime")
}

fn provider(url: &str, auth: Auth, client: &str, secret: &str) -> SasViya {
    SasViya::new(SasViyaConfig {
        url: url.into(),
        auth,
        client_id: client.into(),
        client_secret: secret.into(),
        tls: Tls::default(),
        timeout: Duration::from_secs(30),
    })
    .expect("client")
}

/// Sign in, then folders → reports, search, a picture, CAS and jobs.
async fn walk(p: &SasViya) {
    let info = p.connect().await.expect("connect");
    assert_eq!(info.product, "SAS Viya");
    assert!(!info.signed_in_as.is_empty());
    assert!(
        info.version_text.contains("Reports") && info.version_text.contains("CAS"),
        "{}",
        info.version_text
    );
    let caps = p.capabilities();
    assert!(caps.browse && caps.views && caps.tables && caps.jobs);
    assert!(!caps.native_sql && !caps.view_data);
    assert_eq!(caps.view_files, [ViewFile::Image]);
    assert_eq!(caps.search_kinds, [Kind::Report]);

    // Folders first, each by name; the account's own folder is among them.
    let root = p.children("root").await.expect("root");
    assert!(root.iter().all(|n| n.kind == Kind::Collection));
    let public = root.iter().find(|n| n.name == "Public").expect("Public");
    assert!(root.iter().any(|n| n.name == "My Folder"));
    assert_eq!(
        root.iter().filter(|n| n.name == "My Folder").count(),
        1,
        "listed once"
    );
    let sales = p
        .children(&public.id)
        .await
        .expect("Public")
        .into_iter()
        .find(|n| n.name == "Sales")
        .expect("Sales");
    let reports = p.children(&sales.id).await.expect("Sales' members");
    assert_eq!(
        reports.iter().map(|n| n.name.as_str()).collect::<Vec<_>>(),
        ["Customer Churn", "Revenue by Region"]
    );
    assert!(reports.iter().all(|n| n.kind == Kind::Report));
    let revenue = reports
        .iter()
        .find(|n| n.name == "Revenue by Region")
        .expect("the report");
    assert!(
        p.item_url(Kind::Report, &revenue.id)
            .contains("/SASVisualAnalyticsViewer/"),
        "Open Interactive leads to Visual Analytics"
    );

    // Search by name.
    let found = p.search("revenue", &[], 0).await.expect("search");
    assert_eq!(found.total, 1);
    assert_eq!(found.nodes[0].name, "Revenue by Region");
    assert!(p.search("no such report", &[], 0).await.unwrap().nodes.is_empty());

    // A report's picture: a job polled until it completes, then a PNG.
    let facts = p.view(&revenue.id).await.expect("details");
    assert_eq!(facts.name, "Revenue by Region");
    let png = p
        .view_file(&revenue.id, ViewFile::Image, 800, &[])
        .await
        .expect("picture");
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    assert!(p.view_file(&revenue.id, ViewFile::Csv, 0, &[]).await.is_err());

    // CAS: libraries, tables with their columns, a row preview.
    let libs = p.databases().await.expect("caslibs");
    assert_eq!(
        libs.iter().map(|d| d.name.as_str()).collect::<Vec<_>>(),
        ["Public", "Samples"]
    );
    assert!(libs.iter().all(|d| !d.native && d.engine == "cas"));
    let tables = p.tables("Public").await.expect("tables");
    let orders = tables.iter().find(|t| t.name == "ORDERS").expect("ORDERS");
    assert_eq!(
        orders.fields.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(),
        ["OrderID", "Region", "Amount"]
    );
    let rows = p.preview("Public", "ORDERS", 100).await.expect("rows");
    assert_eq!(rows.columns.len(), 3);
    assert_eq!(rows.rows.len(), 4);
    assert!(!rows.truncated);
    let cut = p.preview("Public", "ORDERS", 2).await.expect("rows");
    assert_eq!((cut.rows.len(), cut.truncated), (2, true));

    // Jobs, newest first.
    let jobs = p.jobs().await.expect("jobs");
    assert_eq!(jobs.len(), 2);
    assert_eq!(jobs[0].ok, Some(false), "the failed one is the newest");
    assert!(jobs[0].notes.contains("CUSTOMERS_SCORE"));

    p.disconnect().await;
    assert!(p.children("root").await.is_err(), "the token is gone");
}

fn parts(var: &str, n: usize) -> Option<Vec<String>> {
    let v = std::env::var(var).ok()?;
    let p: Vec<String> = v.splitn(n, '|').map(str::to_string).collect();
    (p.len() == n).then_some(p)
}

#[test]
fn a_password_through_the_oauth_client_walks_the_server() {
    let Some(p) = parts("HS_SASVIYA", 5) else {
        return;
    };
    runtime().block_on(async {
        let auth = Auth::Password {
            user: p[3].clone(),
            password: p[4].clone(),
        };
        walk(&provider(&p[0], auth, &p[1], &p[2])).await;
    });
}

#[test]
fn a_pasted_access_token_walks_the_server() {
    let Some(p) = parts("HS_SASVIYA_TOKEN", 2) else {
        return;
    };
    runtime().block_on(async {
        let auth = Auth::Token {
            user: String::new(),
            token: p[1].clone(),
        };
        walk(&provider(&p[0], auth, "", "")).await;
    });
}

#[test]
fn a_wrong_password_and_a_wrong_client_are_said_in_words() {
    let Some(p) = parts("HS_SASVIYA", 5) else {
        return;
    };
    runtime().block_on(async {
        let bad_pw = Auth::Password {
            user: p[3].clone(),
            password: "not-the-password".into(),
        };
        let e = provider(&p[0], bad_pw, &p[1], &p[2])
            .connect()
            .await
            .expect_err("refused");
        assert!(e.contains("refused the sign-in"), "{e}");
        let ok_pw = Auth::Password {
            user: p[3].clone(),
            password: p[4].clone(),
        };
        let e = provider(&p[0], ok_pw, "no.such.client", "")
            .connect()
            .await
            .expect_err("refused");
        assert!(e.contains("refused the sign-in"), "{e}");
    });
}
