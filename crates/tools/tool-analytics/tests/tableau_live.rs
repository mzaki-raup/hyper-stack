//! The Tableau provider against a Server, skipped unless `HS_TABLEAU` names
//! one (docs/tools/analytics.md §8.5). Everything here reads.
//!
//! - `HS_TABLEAU=<url>|<site>|<user>|<password>` signs in by password; the
//!   site is empty for the default one
//! - `HS_TABLEAU_PAT=<url>|<site>|<token name>|<token secret>` the same by a
//!   Personal Access Token
//!
//! Tableau Server is licensed, so the bench runs a mock of its REST API
//! (`tableau-mock`, port 8280, on the `chore/bench-kit` branch; credentials
//! in `bench/.env`) and this test is written to the mock's data: the
//! projects Sales and Operations, the workbook Regional Sales with its views
//! Overview and By Month, the data source Orders (extract), two extract
//! tasks and two jobs. Against the owner's own Server only the shape holds:
//! set the variable and read what fails.

use std::time::Duration;

use hs_core::service_auth::Auth;
use hs_core::service_http::Tls;
use tool_analytics::profile::TableauConfig;
use tool_analytics::provider::tableau::Tableau;
use tool_analytics::provider::{BiProvider, Kind, Support, ViewFile};

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("runtime")
}

/// `<url>|<site>|<a>|<b>` from the variable, if it is set.
fn parts(var: &str) -> Option<(String, String, String, String)> {
    let v = std::env::var(var).ok()?;
    let mut it = v.splitn(4, '|').map(str::to_string);
    Some((it.next()?, it.next()?, it.next()?, it.next()?))
}

fn provider(url: &str, site: &str, auth: Auth) -> Tableau {
    Tableau::new(TableauConfig {
        url: url.into(),
        site: site.into(),
        auth,
        tls: Tls::default(),
        timeout: Duration::from_secs(30),
    })
    .expect("client")
}

/// Sign in, walk projects → workbooks → views, search, read a view's picture
/// and data, then the extract tasks and jobs.
async fn walk(p: &Tableau) {
    let info = p.connect().await.expect("connect");
    assert_eq!(info.product, "Tableau Server");
    assert_eq!(info.support, Support::Supported, "{}", info.version_text);
    assert!(!info.signed_in_as.is_empty());

    let root = p.children("root").await.expect("projects");
    assert!(root.iter().all(|n| n.kind == Kind::Collection));
    let sales = root
        .iter()
        .find(|n| n.name == "Sales")
        .expect("the project Sales");
    let inside = p.children(&sales.id).await.expect("a project's items");
    let wb = inside
        .iter()
        .find(|n| n.kind == Kind::Workbook && n.name == "Regional Sales")
        .expect("the workbook Regional Sales is in Sales");
    assert!(
        inside.iter().all(|n| n.collection == "Sales"),
        "a project holds only its own items"
    );
    assert!(inside.iter().any(|n| n.kind == Kind::DataSource));
    let ops = root
        .iter()
        .find(|n| n.name == "Operations")
        .expect("Operations");
    let ops_items = p.children(&ops.id).await.expect("Operations' items");
    assert!(!ops_items.iter().any(|n| n.name == "Regional Sales"));

    let views = p.children(&wb.id).await.expect("a workbook's views");
    let names: Vec<&str> = views.iter().map(|n| n.name.as_str()).collect();
    assert_eq!(names, ["By Month", "Overview"], "views, each by name");
    assert!(views.iter().all(|n| n.kind == Kind::View));
    let overview = views
        .iter()
        .find(|n| n.name == "Overview")
        .expect("Overview");
    assert!(
        p.item_url(Kind::View, &overview.id)
            .contains("/views/RegionalSales/Overview"),
        "Open Interactive leads to the view: {}",
        p.item_url(Kind::View, &overview.id)
    );

    // Search by name, narrowed to a kind.
    let found = p.search("month", &[], 0).await.expect("search");
    assert!(
        found
            .nodes
            .iter()
            .any(|n| n.kind == Kind::View && n.name == "By Month")
    );
    let only = p
        .search("regional", &[Kind::Workbook], 0)
        .await
        .expect("workbooks only");
    assert!(!only.nodes.is_empty() && only.nodes.iter().all(|n| n.kind == Kind::Workbook));

    // A view's picture is a PNG, its data a table.
    let png = p
        .view_file(&overview.id, ViewFile::Image, 800, &[])
        .await
        .expect("picture");
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    let table = p.view_data(&overview.id, &[], 100).await.expect("data");
    assert_eq!(
        table
            .columns
            .iter()
            .map(|c| c.name.as_str())
            .collect::<Vec<_>>(),
        ["Region", "Sales", "Profit"]
    );
    assert_eq!(table.rows.len(), 4);
    assert!(table.columns[1].kind.is_number());
    let cut = p.view_data(&overview.id, &[], 2).await.expect("data");
    assert_eq!((cut.rows.len(), cut.truncated), (2, true));

    // Extract refresh tasks and jobs.
    let tasks = p.extracts().await.expect("tasks");
    assert_eq!(tasks.len(), 2);
    assert!(
        tasks
            .iter()
            .any(|t| t.target == "Orders (extract)" && t.target_kind == "Data Source")
    );
    assert!(
        tasks
            .iter()
            .any(|t| t.target == "Regional Sales" && t.failed == 2)
    );
    let jobs = p.jobs().await.expect("jobs");
    assert_eq!(jobs.len(), 2);
    assert_eq!(jobs[0].ok, Some(false), "newest first, the failed one");
    assert!(jobs[0].notes.contains("Could not connect"));

    p.disconnect().await;
    // The token is gone: the next read is refused.
    assert!(p.children("root").await.is_err());
}

#[test]
fn a_password_walks_the_server() {
    let Some((url, site, user, password)) = parts("HS_TABLEAU") else {
        return;
    };
    runtime().block_on(async {
        walk(&provider(&url, &site, Auth::Password { user, password })).await;
    });
}

#[test]
fn a_personal_access_token_walks_the_server() {
    let Some((url, site, name, secret)) = parts("HS_TABLEAU_PAT") else {
        return;
    };
    runtime().block_on(async {
        walk(&provider(
            &url,
            &site,
            Auth::ApiKey {
                header: name,
                key: secret,
            },
        ))
        .await;
    });
}

#[test]
fn a_wrong_password_and_an_unknown_site_are_said_in_words() {
    let Some((url, site, user, _)) = parts("HS_TABLEAU") else {
        return;
    };
    runtime().block_on(async {
        let bad = provider(
            &url,
            &site,
            Auth::Password {
                user,
                password: "not-the-password".into(),
            },
        );
        let e = bad.connect().await.expect_err("refused");
        assert!(
            e.contains("refused the sign-in") && e.contains("401"),
            "{e}"
        );
        let nowhere = provider(
            &url,
            "no-such-site",
            Auth::Password {
                user: "x".into(),
                password: "y".into(),
            },
        );
        assert!(nowhere.connect().await.is_err());
    });
}
