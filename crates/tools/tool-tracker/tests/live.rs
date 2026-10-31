//! Driver tests against real trackers, each skipped unless its variable
//! names one:
//!
//! - `HS_REDMINE=host:port:api-key` - writes; needs a public project
//!   `demo` with the default trackers and statuses
//! - `HS_FORGEJO_ISSUES=host:port:owner/repo:token` - writes
//! - `HS_GITHUB_ISSUES_REPO=owner/name` - a public repository, read only
//! - `HS_GITLAB_ISSUES_PROJECT=group/name` - a public project on
//!   gitlab.com, read only
//!
//! Writes go only to the local bench servers (owner, 2026-10-31).

use std::time::Duration;

use hs_core::service_auth::{Auth, TokenAs};
use tool_tracker::driver::{self, Edit, NewItem};
use tool_tracker::profile::ConnectConfig;

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("runtime")
}

fn config(kind: &str, url: &str, auth: Auth, token_as: TokenAs) -> ConnectConfig {
    ConnectConfig {
        kind: kind.into(),
        url: url.into(),
        repo: String::new(),
        project: String::new(),
        cloud: false,
        textile: false,
        auth,
        token_as,
        read_only: false,
        timeout: Duration::from_secs(20),
    }
}

/// A name no earlier run has used.
fn unique(what: &str) -> String {
    format!("{what} {}", hs_core::new_id())
}

#[test]
fn redmine_files_an_issue_moves_it_edits_it_and_answers_it() {
    let Ok(spec) = std::env::var("HS_REDMINE") else {
        eprintln!("HS_REDMINE not set; skipping");
        return;
    };
    let parts: Vec<&str> = spec.splitn(3, ':').collect();
    let [host, port, key] = parts[..] else {
        panic!("expected host:port:api-key");
    };
    runtime().block_on(async {
        let mut cfg = config(
            "redmine",
            &format!("http://{host}:{port}"),
            Auth::ApiKey {
                header: "X-Redmine-API-Key".into(),
                key: key.into(),
            },
            TokenAs::Bearer,
        );
        cfg.project = "demo".into();
        let mut d = driver::connect(&cfg).await.expect("connect");
        let form = d.fields("").await.expect("fields");
        assert!(form.kinds.contains(&"Bug".to_string()), "{form:?}");
        assert!(form.priorities.contains(&"High".to_string()), "{form:?}");

        let subject = unique("Disk full on");
        let key = d
            .create(&NewItem {
                kind: "Bug".into(),
                summary: subject.clone(),
                description: "The **disk** is full.".into(),
                priority: "High".into(),
                ..Default::default()
            })
            .await
            .expect("create");
        assert!(key.starts_with('#'), "{key}");

        let found = d
            .search(&format!("status:open {}", &subject[10..]), None)
            .await
            .expect("search");
        assert_eq!(found.items.len(), 1, "{found:?}");
        assert_eq!(found.items[0].key, key);
        assert_eq!(found.items[0].priority, "High");

        let to = d
            .transitions(&key)
            .await
            .expect("transitions")
            .into_iter()
            .find(|t| t.name == "In Progress")
            .expect("In Progress offered");
        d.transition(&key, &to).await.expect("transition");
        d.edit(
            &key,
            &Edit {
                summary: Some(format!("{subject} (edited)")),
                assignee: Some("me".into()),
                due: Some("2026-12-31".into()),
                ..Default::default()
            },
        )
        .await
        .expect("edit");
        d.comment(&key, "Freed *10 GB*.").await.expect("comment");

        let item = d.item(&key).await.expect("item");
        assert_eq!(item.item.status, "In Progress");
        assert!(item.item.summary.ends_with("(edited)"));
        assert_eq!(item.due, "2026-12-31");
        assert_eq!(item.description, "The **disk** is full.");
        assert_eq!(
            item.comments.last().map(|c| c.body.as_str()),
            Some("Freed *10 GB*.")
        );
        assert!(!item.item.assignee.is_empty(), "assigned to me");

        // Assigned to the key's user, so it counts on the dashboard.
        let mine = d.mine_query().expect("signed in");
        let found = d.search(&mine, None).await.expect("mine");
        assert!(found.items.iter().any(|i| i.key == key));

        // A write on a read-only connection never reaches the server.
        cfg.read_only = true;
        let mut ro = driver::connect(&cfg).await.expect("connect read-only");
        assert!(
            ro.comment(&key, "nope")
                .await
                .unwrap_err()
                .contains("read-only")
        );
        let again = d.item(&key).await.expect("item");
        assert_eq!(again.comments.len(), item.comments.len());
    });
}

#[test]
fn forgejo_files_an_issue_comments_closes_and_reopens_it() {
    let Ok(spec) = std::env::var("HS_FORGEJO_ISSUES") else {
        eprintln!("HS_FORGEJO_ISSUES not set; skipping");
        return;
    };
    let parts: Vec<&str> = spec.splitn(4, ':').collect();
    let [host, port, repo, token] = parts[..] else {
        panic!("expected host:port:owner/repo:token");
    };
    runtime().block_on(async {
        let mut cfg = config(
            "forgejo-issues",
            &format!("http://{host}:{port}"),
            Auth::Token {
                user: String::new(),
                token: token.into(),
            },
            TokenAs::TokenPrefix,
        );
        cfg.repo = repo.into();
        let mut d = driver::connect(&cfg).await.expect("connect");
        assert!(!d.info().version.is_empty());
        let title = unique("Flaky test");
        let key = d
            .create(&NewItem {
                summary: title.clone(),
                description: "Fails one run in ten.".into(),
                ..Default::default()
            })
            .await
            .expect("create");
        d.comment(&key, "Seen again on main.")
            .await
            .expect("comment");
        d.edit(
            &key,
            &Edit {
                due: Some("2026-12-31".into()),
                ..Default::default()
            },
        )
        .await
        .expect("edit");
        let item = d.item(&key).await.expect("item");
        assert_eq!(item.item.summary, title);
        assert_eq!(item.comments[0].body, "Seen again on main.");
        assert_eq!(item.due, "2026-12-31");

        let close = d.transitions(&key).await.expect("transitions");
        assert_eq!(close.len(), 1);
        d.transition(&key, &close[0]).await.expect("close");
        let closed = d.search("is:closed", None).await.expect("closed");
        assert!(closed.items.iter().any(|i| i.key == key));
        let open = d.transitions(&key).await.expect("transitions");
        assert_eq!(open[0].id, "open");
        d.transition(&key, &open[0]).await.expect("reopen");
        let found = d.search(&title, None).await.expect("search");
        assert_eq!(found.items.len(), 1, "{found:?}");
        assert_eq!(found.items[0].status, "open");
        // The token's user, so "assigned to me" can be counted.
        assert!(d.mine_query().is_some());
    });
}

#[test]
fn github_reads_a_public_repositorys_issues() {
    let Ok(repo) = std::env::var("HS_GITHUB_ISSUES_REPO") else {
        eprintln!("HS_GITHUB_ISSUES_REPO not set; skipping");
        return;
    };
    runtime().block_on(async {
        let mut cfg = config(
            "github-issues",
            "https://api.github.com",
            Auth::None,
            TokenAs::Bearer,
        );
        cfg.repo = repo;
        let mut d = driver::connect(&cfg).await.expect("connect");
        let found = d.search("is:closed", None).await.expect("search");
        assert!(!found.items.is_empty());
        assert!(found.total.is_some());
        let first = &found.items[0];
        assert!(first.key.starts_with('#'));
        let item = d.item(&first.key).await.expect("item");
        assert_eq!(item.item.key, first.key);
        assert!(item.item.url.starts_with("https://github.com/"));
        // Anonymous: nobody to be "me".
        assert!(d.mine_query().is_none());
    });
}

#[test]
fn gitlab_reads_a_public_projects_issues() {
    let Ok(project) = std::env::var("HS_GITLAB_ISSUES_PROJECT") else {
        eprintln!("HS_GITLAB_ISSUES_PROJECT not set; skipping");
        return;
    };
    runtime().block_on(async {
        let mut cfg = config(
            "gitlab-issues",
            "https://gitlab.com",
            Auth::None,
            TokenAs::Bearer,
        );
        cfg.project = project;
        let mut d = driver::connect(&cfg).await.expect("connect");
        let found = d.search("is:open", None).await.expect("search");
        assert!(!found.items.is_empty());
        assert!(found.next.is_some(), "a busy project has a second page");
        let more = d
            .search("is:open", found.next.as_deref())
            .await
            .expect("more");
        assert_ne!(more.items[0].key, found.items[0].key);
        // gitlab.com keeps notes from anonymous callers; the issue still opens.
        let item = d.item(&found.items[0].key).await.expect("item");
        assert_eq!(item.item.status, "open");
        assert!(item.comments_note.is_some());
    });
}
