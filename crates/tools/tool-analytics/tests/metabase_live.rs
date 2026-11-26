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
use tool_analytics::profile::MetabaseConfig;
use tool_analytics::provider::metabase::Metabase;
use tool_analytics::provider::{BiProvider, Kind, Support};

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
