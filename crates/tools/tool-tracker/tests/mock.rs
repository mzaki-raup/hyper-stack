//! Drivers against a mock HTTP server, for the trackers that cannot run on
//! the bench (Jira and Confluence need an Atlassian licence key) and for
//! the writes the owner keeps off GitHub and GitLab (2026-10-31).
//!
//! The responses are written from each API's reference documentation,
//! trimmed to the fields the drivers read; they were not captured from a
//! live server. What these tests prove is that the drivers call the
//! documented endpoints with the documented bodies and read the documented
//! answers - not that a given server version still answers that way.

use std::sync::Arc;
use std::time::Duration;

use hs_core::service_auth::{Auth, TokenAs};
use parking_lot::Mutex;
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tool_tracker::driver::{self, Edit, NewItem, Transition};
use tool_tracker::profile::ConnectConfig;

/// One canned answer: method, path prefix (query included where it
/// matters), status, body.
type Route = (&'static str, &'static str, u16, String);

/// A request the server saw: method, path, Authorization header, body.
#[derive(Debug, Clone)]
struct Seen {
    method: String,
    path: String,
    auth: String,
    body: String,
}

impl Seen {
    fn json(&self) -> Value {
        serde_json::from_str(&self.body).unwrap_or(Value::Null)
    }
}

async fn serve(routes: Vec<Route>) -> (String, Arc<Mutex<Vec<Seen>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                return;
            };
            let routes = routes.clone();
            let log = log.clone();
            tokio::spawn(async move {
                let mut buf = Vec::new();
                let mut chunk = [0u8; 4096];
                // Headers, then as much body as Content-Length says.
                let (head_end, length) = loop {
                    let n = sock.read(&mut chunk).await.unwrap_or(0);
                    if n == 0 {
                        return;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                    if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                        let head = String::from_utf8_lossy(&buf[..i]).to_lowercase();
                        let length = head
                            .lines()
                            .find_map(|l| l.strip_prefix("content-length:"))
                            .and_then(|v| v.trim().parse::<usize>().ok())
                            .unwrap_or(0);
                        break (i + 4, length);
                    }
                };
                while buf.len() < head_end + length {
                    let n = sock.read(&mut chunk).await.unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                }
                let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
                let body = String::from_utf8_lossy(&buf[head_end..]).to_string();
                let mut first = head.lines().next().unwrap_or("").split(' ');
                let method = first.next().unwrap_or("").to_string();
                let path = first.next().unwrap_or("").to_string();
                let auth = head
                    .lines()
                    .find_map(|l| {
                        let (k, v) = l.split_once(':')?;
                        (k.eq_ignore_ascii_case("authorization")
                            || k.eq_ignore_ascii_case("private-token")
                            || k.eq_ignore_ascii_case("x-redmine-api-key"))
                        .then(|| v.trim().to_string())
                    })
                    .unwrap_or_default();
                log.lock().push(Seen {
                    method: method.clone(),
                    path: path.clone(),
                    auth,
                    body,
                });
                let (status, reply) = routes
                    .iter()
                    .filter(|(m, p, _, _)| *m == method && path.starts_with(p))
                    // The longest prefix is the most specific route.
                    .max_by_key(|(_, p, _, _)| p.len())
                    .map(|(_, _, s, b)| (*s, b.clone()))
                    .unwrap_or((404, r#"{"message":"no such route"}"#.into()));
                let resp = format!(
                    "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{reply}",
                    reply.len()
                );
                let _ = sock.write_all(resp.as_bytes()).await;
            });
        }
    });
    (format!("http://{addr}"), seen)
}

fn config(kind: &str, url: &str, cloud: bool, auth: Auth, token_as: TokenAs) -> ConnectConfig {
    ConnectConfig {
        kind: kind.into(),
        url: url.into(),
        repo: String::new(),
        project: String::new(),
        cloud,
        textile: false,
        auth,
        token_as,
        read_only: false,
        timeout: Duration::from_secs(5),
    }
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("runtime")
}

fn find<'a>(seen: &'a [Seen], method: &str, path: &str) -> &'a Seen {
    seen.iter()
        .find(|s| s.method == method && s.path.starts_with(path))
        .unwrap_or_else(|| panic!("no {method} {path} in {seen:#?}"))
}

const ADF_DESC: &str = r#"{"type":"doc","version":1,"content":[{"type":"paragraph","content":[{"type":"text","text":"The "},{"type":"text","text":"disk","marks":[{"type":"strong"}]},{"type":"text","text":" is full."}]}]}"#;

#[test]
fn jira_cloud_searches_by_token_reads_adf_and_writes_it_back() {
    runtime().block_on(async {
        let issue = format!(
            r#"{{"key":"OPS-1","fields":{{"summary":"Disk full","status":{{"name":"To Do"}},"assignee":{{"displayName":"Ada"}},"priority":{{"name":"High"}},"updated":"2026-10-31T10:00:00.000+0000","duedate":"2026-11-02","issuetype":{{"name":"Bug"}},"reporter":{{"displayName":"Bob"}},"labels":["ops"],"description":{ADF_DESC},"comment":{{"comments":[{{"author":{{"displayName":"Bob"}},"created":"2026-10-31T11:00:00.000+0000","body":{ADF_DESC}}}]}},"issuelinks":[{{"type":{{"outward":"blocks","inward":"is blocked by"}},"outwardIssue":{{"key":"OPS-9"}}}}],"attachment":[{{"filename":"df.txt","content":"https://x/df.txt"}}]}}}}"#
        );
        let (url, seen) = serve(vec![
            ("GET", "/rest/api/3/serverInfo", 200, r#"{"version":"1001.0.0","deploymentType":"Cloud"}"#.into()),
            ("GET", "/rest/api/3/search/jql", 200, format!(r#"{{"issues":[{issue}],"nextPageToken":"tok2","isLast":false}}"#)),
            ("GET", "/rest/api/3/issue/OPS-1?", 200, issue.clone()),
            ("GET", "/rest/api/3/issue/OPS-1/transitions", 200, r#"{"transitions":[{"id":"31","name":"Done"}]}"#.into()),
            ("POST", "/rest/api/3/issue/OPS-1/transitions", 204, String::new()),
            ("PUT", "/rest/api/3/issue/OPS-1", 204, String::new()),
            ("POST", "/rest/api/3/issue/OPS-1/comment", 201, r#"{"id":"10"}"#.into()),
            ("GET", "/rest/api/3/user/search", 200, r#"[{"accountId":"acc-ada"}]"#.into()),
            ("GET", "/rest/api/3/issue/createmeta/OPS/issuetypes", 200, r#"{"issueTypes":[{"name":"Bug","subtask":false},{"name":"Sub-task","subtask":true}]}"#.into()),
            ("GET", "/rest/api/3/priority", 200, r#"[{"name":"High"},{"name":"Low"}]"#.into()),
            ("POST", "/rest/api/3/issue", 201, r#"{"key":"OPS-2"}"#.into()),
        ])
        .await;
        let mut cfg = config(
            "jira",
            &url,
            true,
            Auth::Token {
                user: "ada@example.com".into(),
                token: "tok".into(),
            },
            TokenAs::Basic,
        );
        cfg.project = "OPS".into();
        let mut d = driver::connect(&cfg).await.expect("connect");
        assert_eq!(d.info().version, "1001.0.0");
        assert!(d.default_query().starts_with("project = OPS"));

        let found = d.search("project = OPS", None).await.expect("search");
        assert_eq!(found.items[0].key, "OPS-1");
        assert_eq!(found.items[0].due, "2026-11-02");
        assert_eq!(found.next.as_deref(), Some("tok2"));
        d.search("project = OPS", Some("tok2")).await.expect("more");

        let item = d.item("OPS-1").await.expect("item");
        assert_eq!(item.description, "The **disk** is full.");
        assert_eq!(item.lossy, None);
        assert_eq!(item.comments[0].body, "The **disk** is full.");
        assert_eq!(item.links, vec![("blocks".to_string(), "OPS-9".to_string())]);
        assert_eq!(item.labels, vec!["ops"]);

        let t = d.transitions("OPS-1").await.expect("transitions");
        d.transition("OPS-1", &t[0]).await.expect("transition");
        d.edit(
            "OPS-1",
            &Edit {
                description: Some("Now *fine*.".into()),
                assignee: Some("ada".into()),
                ..Default::default()
            },
        )
        .await
        .expect("edit");
        d.comment("OPS-1", "On it").await.expect("comment");
        let form = d.fields("").await.expect("fields");
        assert_eq!(form.kinds, vec!["Bug"]);
        assert_eq!(form.priorities, vec!["High", "Low"]);
        let key = d
            .create(&NewItem {
                kind: "Bug".into(),
                summary: "New".into(),
                description: "Body".into(),
                ..Default::default()
            })
            .await
            .expect("create");
        assert_eq!(key, "OPS-2");

        let seen = seen.lock().clone();
        // Email and token as basic: base64("ada@example.com:tok").
        assert_eq!(
            find(&seen, "GET", "/rest/api/3/serverInfo").auth,
            "Basic YWRhQGV4YW1wbGUuY29tOnRvaw=="
        );
        assert!(seen.iter().any(|s| s.path.contains("nextPageToken=tok2")));
        assert_eq!(
            find(&seen, "POST", "/rest/api/3/issue/OPS-1/transitions").json()["transition"]["id"],
            "31"
        );
        let put = find(&seen, "PUT", "/rest/api/3/issue/OPS-1").json();
        assert_eq!(put["fields"]["description"]["type"], "doc");
        assert_eq!(put["fields"]["description"]["content"][0]["content"][1]["marks"][0]["type"], "em");
        assert_eq!(put["fields"]["assignee"]["accountId"], "acc-ada");
        assert!(put["fields"].get("summary").is_none(), "only what changed is sent");
        assert_eq!(
            find(&seen, "POST", "/rest/api/3/issue/OPS-1/comment").json()["body"]["type"],
            "doc"
        );
        let created = seen
            .iter()
            .find(|s| s.method == "POST" && s.path == "/rest/api/3/issue")
            .expect("create")
            .json();
        assert_eq!(created["fields"]["project"]["key"], "OPS");
        assert_eq!(created["fields"]["issuetype"]["name"], "Bug");
    });
}

#[test]
fn jira_data_center_pages_by_offset_and_speaks_wiki_markup() {
    runtime().block_on(async {
        let issue = r#"{"key":"OPS-1","fields":{"summary":"Disk full","status":{"name":"Open"},"assignee":{"name":"ada","displayName":"Ada"},"updated":"2026-10-31T10:00:00.000+0000","description":"h2. Steps\n* run {{df -h}}","comment":{"comments":[]}}}"#;
        let (url, seen) = serve(vec![
            ("GET", "/rest/api/2/serverInfo", 200, r#"{"version":"9.12.0"}"#.into()),
            ("GET", "/rest/api/2/search", 200, format!(r#"{{"startAt":0,"total":150,"issues":[{issue}]}}"#)),
            ("GET", "/rest/api/2/issue/OPS-1?", 200, issue.into()),
            ("PUT", "/rest/api/2/issue/OPS-1", 204, String::new()),
        ])
        .await;
        let cfg = config(
            "jira",
            &url,
            false,
            Auth::Token {
                user: String::new(),
                token: "pat".into(),
            },
            TokenAs::Bearer,
        );
        let mut d = driver::connect(&cfg).await.expect("connect");
        let found = d.search("assignee = currentUser()", None).await.expect("search");
        assert_eq!(found.total, Some(150));
        assert_eq!(found.next.as_deref(), Some("1"));
        let item = d.item("OPS-1").await.expect("item");
        assert_eq!(item.description, "## Steps\n- run `df -h`");
        d.edit(
            "OPS-1",
            &Edit {
                description: Some("Use **sudo**.".into()),
                assignee: Some("bob".into()),
                ..Default::default()
            },
        )
        .await
        .expect("edit");
        let seen = seen.lock().clone();
        assert_eq!(find(&seen, "GET", "/rest/api/2/serverInfo").auth, "Bearer pat");
        let put = find(&seen, "PUT", "/rest/api/2/issue/OPS-1").json();
        assert_eq!(put["fields"]["description"], "Use *sudo*.");
        assert_eq!(put["fields"]["assignee"]["name"], "bob");
    });
}

#[test]
fn confluence_cloud_browses_reads_and_publishes_with_the_version_bumped() {
    runtime().block_on(async {
        let (url, seen) = serve(vec![
            ("GET", "/wiki/rest/api/space?limit=1", 200, r#"{"results":[]}"#.into()),
            ("GET", "/wiki/api/v2/spaces?", 200, r#"{"results":[{"id":"65","key":"DOCS","name":"Docs"}]}"#.into()),
            ("GET", "/wiki/api/v2/spaces/65/pages", 200, r#"{"results":[{"id":"1","title":"Home","parentId":null},{"id":"2","title":"Runbook","parentId":"1"}]}"#.into()),
            ("GET", "/wiki/api/v2/pages/2?", 200, r#"{"id":"2","title":"Runbook","version":{"number":4},"body":{"storage":{"value":"<h1>Runbook</h1><p>Restart the <strong>api</strong>.</p>"}},"_links":{"webui":"/spaces/DOCS/pages/2"}}"#.into()),
            ("GET", "/wiki/api/v2/pages/2/footer-comments", 200, r#"{"results":[{"version":{"createdAt":"2026-10-31T10:00:00.000Z"},"body":{"storage":{"value":"<p>Done</p>"}}}]}"#.into()),
            ("GET", "/wiki/api/v2/pages/3?", 200, r#"{"id":"3","title":"Dashboard","version":{"number":1},"body":{"storage":{"value":"<ac:structured-macro ac:name=\"jira\"/>"}}}"#.into()),
            ("GET", "/wiki/api/v2/pages/3/footer-comments", 200, r#"{"results":[]}"#.into()),
            ("PUT", "/wiki/api/v2/pages/2", 200, "{}".into()),
            ("POST", "/wiki/api/v2/footer-comments", 201, "{}".into()),
            ("GET", "/wiki/rest/api/content/search", 200, r#"{"results":[{"id":"2","title":"Runbook","space":{"key":"DOCS"},"version":{"when":"2026-10-31T10:00:00.000Z","by":{"displayName":"Ada"}},"_links":{"webui":"/x"}}],"size":1,"_links":{}}"#.into()),
        ])
        .await;
        let cfg = config(
            "confluence",
            &url,
            true,
            Auth::Token {
                user: "ada@example.com".into(),
                token: "tok".into(),
            },
            TokenAs::Basic,
        );
        let mut d = driver::connect(&cfg).await.expect("connect");
        assert!(d.caps().pages);
        let spaces = d.spaces().await.expect("spaces");
        assert_eq!(spaces[0].key, "DOCS");
        let pages = d.pages(&spaces[0]).await.expect("pages");
        assert_eq!(pages[1].parent, "1");
        let page = d.page("2").await.expect("page");
        assert_eq!(page.body, "# Runbook\n\nRestart the **api**.");
        assert_eq!(page.read_only, None);
        assert_eq!(page.comments[0].body, "Done");
        assert!(page.url.ends_with("/wiki/spaces/DOCS/pages/2"));
        let said = d
            .publish(&page, "# Runbook\n\nRestart the **api** twice.")
            .await
            .expect("publish");
        assert!(said.contains("version 5"), "{said}");
        d.comment("2", "Checked").await.expect("comment");
        let found = d.search("space = DOCS", None).await.expect("search");
        assert_eq!(found.items[0].summary, "Runbook");
        assert_eq!(found.next, None);

        // A page with a macro is shown, never published over.
        let dash = d.page("3").await.expect("page");
        assert!(dash.read_only.is_some());
        assert!(d.publish(&dash, "x").await.unwrap_err().contains("refused"));

        let seen = seen.lock().clone();
        let put = find(&seen, "PUT", "/wiki/api/v2/pages/2").json();
        assert_eq!(put["version"]["number"], 5);
        assert_eq!(put["body"]["representation"], "storage");
        assert_eq!(
            put["body"]["value"],
            "<h1>Runbook</h1><p>Restart the <strong>api</strong> twice.</p>"
        );
        assert_eq!(find(&seen, "POST", "/wiki/api/v2/footer-comments").json()["pageId"], "2");
        assert!(!seen.iter().any(|s| s.method == "PUT" && s.path.contains("/pages/3")));
    });
}

#[test]
fn confluence_data_center_uses_v1_with_ancestors_for_the_tree() {
    runtime().block_on(async {
        let (url, seen) = serve(vec![
            ("GET", "/rest/api/space?limit=1", 200, r#"{"results":[]}"#.into()),
            ("GET", "/rest/api/content?spaceKey=DOCS", 200, r#"{"results":[{"id":"2","title":"Runbook","ancestors":[{"id":"1"}]}]}"#.into()),
            ("GET", "/rest/api/content/2?", 200, r#"{"id":"2","title":"Runbook","version":{"number":7},"body":{"storage":{"value":"<p>Hi</p>"}},"_links":{"webui":"/display/DOCS/Runbook"}}"#.into()),
            ("GET", "/rest/api/content/2/child/comment", 200, r#"{"results":[{"version":{"when":"2026-10-31T10:00:00.000Z","by":{"displayName":"Bob"}},"body":{"storage":{"value":"<p>Ok</p>"}}}]}"#.into()),
            ("PUT", "/rest/api/content/2", 200, "{}".into()),
        ])
        .await;
        let cfg = config("confluence", &url, false, Auth::None, TokenAs::Bearer);
        let mut d = driver::connect(&cfg).await.expect("connect");
        let space = tool_tracker::driver::Space {
            id: "98".into(),
            key: "DOCS".into(),
            name: "Docs".into(),
        };
        let pages = d.pages(&space).await.expect("pages");
        assert_eq!(pages[0].parent, "1");
        let page = d.page("2").await.expect("page");
        assert_eq!(page.comments[0].author, "Bob");
        d.publish(&page, "Hello").await.expect("publish");
        let seen = seen.lock().clone();
        let put = find(&seen, "PUT", "/rest/api/content/2").json();
        assert_eq!(put["type"], "page");
        assert_eq!(put["version"]["number"], 8);
        assert_eq!(put["body"]["storage"]["value"], "<p>Hello</p>");
    });
}

#[test]
fn github_issue_writes_send_the_documented_bodies() {
    runtime().block_on(async {
        let (url, seen) = serve(vec![
            ("GET", "/repos/o/r", 200, r#"{"has_issues":true}"#.into()),
            ("GET", "/search/issues", 200, r#"{"total_count":1,"items":[{"number":5,"title":"Bug","state":"open","updated_at":"2026-10-31T10:00:00Z","html_url":"https://github.com/o/r/issues/5"}]}"#.into()),
            ("PATCH", "/repos/o/r/issues/5", 200, "{}".into()),
            ("POST", "/repos/o/r/issues/5/comments", 201, "{}".into()),
            ("POST", "/repos/o/r/issues", 201, r#"{"number":6}"#.into()),
        ])
        .await;
        let mut cfg = config(
            "github-issues",
            &url,
            false,
            Auth::Token {
                user: String::new(),
                token: "ghp".into(),
            },
            TokenAs::Bearer,
        );
        cfg.repo = "o/r".into();
        let mut d = driver::connect(&cfg).await.expect("connect");
        assert_eq!(d.mine_query().as_deref(), Some("is:open assignee:@me"));
        let found = d.search("is:open label:bug", None).await.expect("search");
        assert_eq!(found.items[0].key, "#5");
        d.transition(
            "#5",
            &Transition {
                id: "not_planned".into(),
                name: "Closed (Not Planned)".into(),
            },
        )
        .await
        .expect("close");
        d.edit(
            "#5",
            &Edit {
                labels: Some(vec!["bug".into(), "ui".into()]),
                ..Default::default()
            },
        )
        .await
        .expect("edit");
        d.comment("#5", "Fixed in main").await.expect("comment");
        let key = d
            .create(&NewItem {
                summary: "New".into(),
                labels: vec!["bug".into()],
                ..Default::default()
            })
            .await
            .expect("create");
        assert_eq!(key, "#6");
        let seen = seen.lock().clone();
        let q = &find(&seen, "GET", "/search/issues").path;
        assert!(q.contains("repo%3Ao%2Fr") && q.contains("is%3Aissue"), "{q}");
        let patches: Vec<Value> = seen
            .iter()
            .filter(|s| s.method == "PATCH")
            .map(Seen::json)
            .collect();
        assert_eq!(patches[0]["state_reason"], "not_planned");
        assert_eq!(patches[1]["labels"], serde_json::json!(["bug", "ui"]));
        assert_eq!(find(&seen, "PATCH", "/repos/o/r/issues/5").auth, "Bearer ghp");
    });
}

#[test]
fn gitlab_issue_writes_send_the_documented_bodies() {
    runtime().block_on(async {
        let (url, seen) = serve(vec![
            (
                "GET",
                "/api/v4/projects/g%2Fp",
                200,
                r#"{"issues_enabled":true}"#.into(),
            ),
            (
                "GET",
                "/api/v4/version",
                401,
                r#"{"message":"401 Unauthorized"}"#.into(),
            ),
            ("GET", "/api/v4/user", 200, r#"{"username":"me"}"#.into()),
            (
                "GET",
                "/api/v4/users?username=ada",
                200,
                r#"[{"id":42}]"#.into(),
            ),
            ("PUT", "/api/v4/projects/g%2Fp/issues/7", 200, "{}".into()),
            (
                "POST",
                "/api/v4/projects/g%2Fp/issues/7/notes",
                201,
                "{}".into(),
            ),
            (
                "POST",
                "/api/v4/projects/g%2Fp/issues",
                201,
                r#"{"iid":8}"#.into(),
            ),
        ])
        .await;
        let mut cfg = config(
            "gitlab-issues",
            &url,
            false,
            Auth::ApiKey {
                header: "PRIVATE-TOKEN".into(),
                key: "glpat".into(),
            },
            TokenAs::Bearer,
        );
        cfg.project = "g/p".into();
        let mut d = driver::connect(&cfg).await.expect("connect");
        assert_eq!(d.mine_query().as_deref(), Some("is:open assignee:me"));
        d.transition(
            "#7",
            &Transition {
                id: "closed".into(),
                name: "Closed".into(),
            },
        )
        .await
        .expect("close");
        d.edit(
            "#7",
            &Edit {
                assignee: Some("ada".into()),
                due: Some("2026-11-30".into()),
                ..Default::default()
            },
        )
        .await
        .expect("edit");
        d.comment("#7", "Noted").await.expect("note");
        assert_eq!(
            d.create(&NewItem {
                summary: "New".into(),
                labels: vec!["a".into(), "b".into()],
                ..Default::default()
            })
            .await
            .expect("create"),
            "#8"
        );
        let seen = seen.lock().clone();
        let puts: Vec<Value> = seen
            .iter()
            .filter(|s| s.method == "PUT")
            .map(Seen::json)
            .collect();
        assert_eq!(puts[0]["state_event"], "close");
        assert_eq!(puts[1]["assignee_ids"], serde_json::json!([42]));
        assert_eq!(puts[1]["due_date"], "2026-11-30");
        let created = seen
            .iter()
            .find(|s| s.method == "POST" && s.path == "/api/v4/projects/g%2Fp/issues")
            .expect("create");
        assert_eq!(created.json()["labels"], "a,b");
        assert_eq!(created.auth, "glpat");
    });
}

#[test]
fn a_read_only_connection_sends_no_write_at_all() {
    runtime().block_on(async {
        let (url, seen) = serve(vec![(
            "GET",
            "/repos/o/r",
            200,
            r#"{"has_issues":true}"#.into(),
        )])
        .await;
        let mut cfg = config("github-issues", &url, false, Auth::None, TokenAs::Bearer);
        cfg.repo = "o/r".into();
        cfg.read_only = true;
        let mut d = driver::connect(&cfg).await.expect("connect");
        assert!(d.comment("#1", "x").await.is_err());
        assert!(d.create(&NewItem::default()).await.is_err());
        assert!(!seen.lock().iter().any(|s| s.method != "GET"));
    });
}
