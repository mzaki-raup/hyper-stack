//! GitHub Issues through the REST API (docs/tools/tracker.md §4). A
//! connection is one repository; the query is GitHub's search syntax,
//! scoped to the repository and to issues.

use async_trait::async_trait;
use reqwest::Method;
use serde_json::{Map, Value, json};

use crate::driver::{
    Caps, Comment, CreateForm, Detail, Driver, Edit, Found, Item, MAX_COMMENTS, NewItem, PAGE,
    Result, ServerInfo, Transition, iso_ms, q,
};
use crate::profile::ConnectConfig;
use hs_core::service_http::{Http, s};

pub struct GitHub {
    http: Http,
    repo: String,
    signed_in: bool,
    info: ServerInfo,
}

/// "#42" or "42" to 42.
pub fn number(key: &str) -> Result<u64> {
    key.trim()
        .trim_start_matches('#')
        .parse()
        .map_err(|_| format!("“{key}” is not an issue number"))
}

/// Names from a list of objects: assignees' logins, labels' names.
pub fn names(v: Option<&Value>, k: &str) -> Vec<String> {
    v.and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|x| s(x, k))
        .filter(|n| !n.is_empty())
        .collect()
}

/// "a, b" to ["a", "b"].
pub fn split_list(s: &str) -> Vec<String> {
    s.split(',')
        .map(str::trim)
        .filter(|x| !x.is_empty())
        .map(String::from)
        .collect()
}

/// A forge issue (GitHub and Forgejo share the shape) as a row.
pub fn forge_item(i: &Value) -> Item {
    let assignees = names(i.get("assignees"), "login");
    Item {
        key: format!("#{}", s(i, "number")),
        summary: s(i, "title"),
        status: s(i, "state"),
        assignee: assignees.join(", "),
        priority: String::new(),
        updated_ms: iso_ms(&s(i, "updated_at")),
        due: s(i, "due_date").chars().take(10).collect(),
        url: s(i, "html_url"),
    }
}

pub fn forge_comment(c: &Value) -> Comment {
    Comment {
        author: c.get("user").map(|u| s(u, "login")).unwrap_or_default(),
        created_ms: iso_ms(&s(c, "created_at")),
        body: s(c, "body"),
    }
}

/// The fields every forge issue shows beside its description.
pub fn forge_detail(i: &Value, comments: Vec<Comment>) -> Detail {
    let mut fields = vec![(
        "author".to_string(),
        i.get("user").map(|u| s(u, "login")).unwrap_or_default(),
    )];
    if let Some(t) = iso_ms(&s(i, "created_at")) {
        fields.push(("created".into(), crate::driver::time_ms(t)));
    }
    if let Some(m) = i.get("milestone").filter(|m| !m.is_null()) {
        fields.push(("milestone".into(), s(m, "title")));
    }
    Detail {
        item: forge_item(i),
        fields,
        description: s(i, "body"),
        lossy: None,
        labels: names(i.get("labels"), "name"),
        due: s(i, "due_date").chars().take(10).collect(),
        comments,
        comments_note: None,
        attachments: Vec::new(),
        links: Vec::new(),
    }
}

/// Open → close (done or not planned); closed → reopen.
pub fn forge_transitions(state: &str, not_planned: bool) -> Vec<Transition> {
    let t = |id: &str, name: &str| Transition {
        id: id.into(),
        name: name.into(),
    };
    if state == "open" {
        let mut v = vec![t("closed", "Closed")];
        if not_planned {
            v.push(t("not_planned", "Closed (Not Planned)"));
        }
        v
    } else {
        vec![t("open", "Open")]
    }
}

/// The body of an edit, in the forges' shared field names.
pub fn forge_edit(e: &Edit) -> Map<String, Value> {
    let mut body = Map::new();
    if let Some(v) = &e.summary {
        body.insert("title".into(), json!(v));
    }
    if let Some(v) = &e.description {
        body.insert("body".into(), json!(v));
    }
    if let Some(v) = &e.assignee {
        body.insert("assignees".into(), json!(split_list(v)));
    }
    body
}

impl GitHub {
    pub async fn connect(cfg: &ConnectConfig) -> Result<GitHub> {
        let http = cfg
            .http(&cfg.url)?
            .with_header("Accept", "application/vnd.github+json")
            .with_header("X-GitHub-Api-Version", "2022-11-28");
        let r = http.get_json(&format!("/repos/{}", cfg.repo)).await?;
        if r.get("has_issues") == Some(&Value::Bool(false)) {
            return Err(format!("{} has issues turned off", cfg.repo));
        }
        Ok(GitHub {
            http,
            repo: cfg.repo.clone(),
            signed_in: cfg.auth != hs_core::service_auth::Auth::None,
            info: ServerInfo {
                product: "GitHub Issues".into(),
                version: String::new(),
            },
        })
    }

    async fn comments(&self, n: u64) -> Result<Vec<Comment>> {
        let mut out = Vec::new();
        for page in 1..=MAX_COMMENTS.div_ceil(PAGE) {
            let v = self
                .http
                .get_json(&format!(
                    "/repos/{}/issues/{n}/comments?per_page={PAGE}&page={page}",
                    self.repo
                ))
                .await?;
            let list = v.as_array().cloned().unwrap_or_default();
            let done = list.len() < PAGE;
            out.extend(list.iter().map(forge_comment));
            if done {
                break;
            }
        }
        out.truncate(MAX_COMMENTS);
        Ok(out)
    }

    async fn patch(&self, n: u64, body: Value) -> Result<Value> {
        self.http
            .json(
                self.http
                    .request(Method::PATCH, &format!("/repos/{}/issues/{n}", self.repo))
                    .json(&body),
            )
            .await
    }
}

#[async_trait]
impl Driver for GitHub {
    fn info(&self) -> &ServerInfo {
        &self.info
    }

    fn caps(&self) -> Caps {
        Caps {
            transition: true,
            edit: true,
            comment: true,
            create: true,
            pages: false,
        }
    }

    fn default_query(&self) -> String {
        "is:open sort:updated-desc".into()
    }

    fn mine_query(&self) -> Option<String> {
        self.signed_in.then(|| "is:open assignee:@me".into())
    }

    async fn search(&mut self, query: &str, from: Option<&str>) -> Result<Found> {
        let page: u64 = from.and_then(|p| p.parse().ok()).unwrap_or(1);
        // Search covers pull requests too; the tab lists issues.
        let mut full = format!("repo:{} {query}", self.repo);
        if !query.contains("is:issue") && !query.contains("is:pr") && !query.contains("type:") {
            full.push_str(" is:issue");
        }
        let v = self
            .http
            .get_json(&format!(
                "/search/issues?q={}&per_page={PAGE}&page={page}",
                q(&full)
            ))
            .await?;
        let items: Vec<Item> = v
            .get("items")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(forge_item)
            .collect();
        let total = v.get("total_count").and_then(Value::as_u64);
        // GitHub stops at the 1000th result.
        let seen = page * PAGE as u64;
        let next = (total.is_some_and(|t| seen < t) && seen < 1000).then(|| (page + 1).to_string());
        Ok(Found { items, next, total })
    }

    async fn item(&mut self, key: &str) -> Result<Detail> {
        let n = number(key)?;
        let i = self
            .http
            .get_json(&format!("/repos/{}/issues/{n}", self.repo))
            .await?;
        let comments = self.comments(n).await?;
        Ok(forge_detail(&i, comments))
    }

    async fn transitions(&mut self, key: &str) -> Result<Vec<Transition>> {
        let n = number(key)?;
        let i = self
            .http
            .get_json(&format!("/repos/{}/issues/{n}", self.repo))
            .await?;
        Ok(forge_transitions(&s(&i, "state"), true))
    }

    async fn transition(&mut self, key: &str, to: &Transition) -> Result<String> {
        let n = number(key)?;
        let body = match to.id.as_str() {
            "not_planned" => json!({ "state": "closed", "state_reason": "not_planned" }),
            "closed" => json!({ "state": "closed", "state_reason": "completed" }),
            _ => json!({ "state": "open" }),
        };
        self.patch(n, body).await?;
        Ok(format!("#{n} is now {}", to.name))
    }

    async fn edit(&mut self, key: &str, e: &Edit) -> Result<String> {
        let n = number(key)?;
        let mut body = forge_edit(e);
        if let Some(v) = &e.labels {
            body.insert("labels".into(), json!(v));
        }
        if e.due.is_some() {
            return Err("GitHub issues have no due date".into());
        }
        self.patch(n, Value::Object(body)).await?;
        Ok(format!("#{n} saved"))
    }

    async fn comment(&mut self, key: &str, body: &str) -> Result<String> {
        let n = number(key)?;
        self.http
            .json(
                self.http
                    .request(
                        Method::POST,
                        &format!("/repos/{}/issues/{n}/comments", self.repo),
                    )
                    .json(&json!({ "body": body })),
            )
            .await?;
        Ok(format!("Comment added to #{n}"))
    }

    async fn fields(&mut self, _project: &str) -> Result<CreateForm> {
        Ok(CreateForm::default())
    }

    async fn create(&mut self, item: &NewItem) -> Result<String> {
        let v = self
            .http
            .json(
                self.http
                    .request(Method::POST, &format!("/repos/{}/issues", self.repo))
                    .json(&json!({
                        "title": item.summary,
                        "body": item.description,
                        "labels": item.labels,
                    })),
            )
            .await?;
        Ok(format!("#{}", s(&v, "number")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_issue_row_reads_the_forge_shape() {
        let i = json!({
            "number": 42, "title": "Crash on start", "state": "open",
            "assignees": [{ "login": "ada" }, { "login": "bob" }],
            "updated_at": "2026-10-31T10:00:00Z",
            "html_url": "https://github.com/o/r/issues/42"
        });
        let it = forge_item(&i);
        assert_eq!(it.key, "#42");
        assert_eq!(it.assignee, "ada, bob");
        assert_eq!(it.updated_ms, Some(1_793_440_800_000));
        assert_eq!(number("#42"), Ok(42));
        assert!(number("OPS-1").is_err());
    }

    #[test]
    fn an_open_issue_can_close_two_ways_and_a_closed_one_reopen() {
        assert_eq!(forge_transitions("open", true).len(), 2);
        assert_eq!(forge_transitions("open", false).len(), 1);
        assert_eq!(forge_transitions("closed", true)[0].id, "open");
    }

    #[test]
    fn an_edit_sends_only_what_changed() {
        let e = Edit {
            summary: Some("New".into()),
            assignee: Some("ada, bob".into()),
            ..Default::default()
        };
        let b = forge_edit(&e);
        assert_eq!(b.len(), 2);
        assert_eq!(b["assignees"], json!(["ada", "bob"]));
    }
}
