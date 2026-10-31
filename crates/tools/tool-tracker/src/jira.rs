//! Jira through REST v3 (Cloud) or v2 (Data Center)
//! (docs/tools/tracker.md §4). The query is JQL. Cloud keeps text as
//! Atlassian Document Format, Data Center as wiki markup; both are shown
//! and edited as Markdown through `markup.rs`.

use async_trait::async_trait;
use reqwest::Method;
use serde_json::{Map, Value, json};

use crate::driver::{
    Caps, Comment, CreateForm, Detail, Driver, Edit, Found, Item, MAX_COMMENTS, NewItem, PAGE,
    Result, ServerInfo, Transition, iso_ms, q, time_ms,
};
use crate::markup::{Lite, adf_to_md, lite_to_md, md_to_adf, md_to_lite};
use crate::profile::ConnectConfig;
use hs_core::service_http::{Http, s, seg};

const FIELDS: &str = "summary,status,assignee,priority,updated,duedate";

pub struct Jira {
    http: Http,
    signed_in: bool,
    web: String,
    cloud: bool,
    project: String,
    info: ServerInfo,
}

fn display(v: &Value, k: &str) -> String {
    v.get(k)
        .filter(|x| !x.is_null())
        .map(|x| {
            let d = s(x, "displayName");
            if d.is_empty() { s(x, "name") } else { d }
        })
        .unwrap_or_default()
}

impl Jira {
    pub async fn connect(cfg: &ConnectConfig) -> Result<Jira> {
        let api = if cfg.cloud { "3" } else { "2" };
        let http = cfg.http(&format!("{}/rest/api/{api}", cfg.url))?;
        let v = http.get_json("/serverInfo").await?;
        Ok(Jira {
            http,
            signed_in: cfg.auth != hs_core::service_auth::Auth::None,
            web: cfg.url.clone(),
            cloud: cfg.cloud,
            project: cfg.project.clone(),
            info: ServerInfo {
                product: "Jira".into(),
                version: s(&v, "version"),
            },
        })
    }

    /// A text field (description, comment body) as Markdown.
    fn text(&self, v: Option<&Value>) -> (String, Option<String>) {
        match v {
            Some(Value::String(t)) => lite_to_md(t, Lite::JiraWiki),
            Some(doc @ Value::Object(_)) => adf_to_md(doc),
            _ => (String::new(), None),
        }
    }

    /// Markdown as the text field this deployment takes.
    fn body(&self, md: &str) -> Value {
        if self.cloud {
            md_to_adf(md)
        } else {
            json!(md_to_lite(md, Lite::JiraWiki))
        }
    }

    fn row(&self, i: &Value) -> Item {
        let key = s(i, "key");
        let f = i.get("fields").cloned().unwrap_or_default();
        Item {
            url: format!("{}/browse/{key}", self.web),
            key,
            summary: s(&f, "summary"),
            status: f.get("status").map(|x| s(x, "name")).unwrap_or_default(),
            assignee: display(&f, "assignee"),
            priority: f.get("priority").map(|x| s(x, "name")).unwrap_or_default(),
            updated_ms: iso_ms(&s(&f, "updated")),
            due: s(&f, "duedate"),
        }
    }

    /// Cloud names people by account id, Data Center by user name.
    async fn person(&self, who: &str) -> Result<Value> {
        let who = who.trim();
        if who.is_empty() {
            return Ok(Value::Null);
        }
        if !self.cloud {
            return Ok(json!({ "name": who }));
        }
        let v = self
            .http
            .get_json(&format!("/user/search?query={}", q(who)))
            .await?;
        let id = v
            .as_array()
            .and_then(|a| a.first())
            .map(|u| s(u, "accountId"))
            .filter(|id| !id.is_empty())
            .ok_or_else(|| format!("no Jira user matches “{who}”"))?;
        Ok(json!({ "accountId": id }))
    }
}

#[async_trait]
impl Driver for Jira {
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
        if self.project.is_empty() {
            "assignee = currentUser() AND statusCategory != Done ORDER BY updated DESC".into()
        } else {
            format!(
                "project = {} AND statusCategory != Done ORDER BY updated DESC",
                self.project
            )
        }
    }

    fn mine_query(&self) -> Option<String> {
        self.signed_in
            .then(|| "assignee = currentUser() AND statusCategory != Done".into())
    }

    async fn search(&mut self, query: &str, from: Option<&str>) -> Result<Found> {
        // Cloud retired the offset search for one that pages by token.
        let v = if self.cloud {
            let mut path = format!(
                "/search/jql?jql={}&maxResults={PAGE}&fields={FIELDS}",
                q(query)
            );
            if let Some(t) = from {
                path.push_str(&format!("&nextPageToken={}", q(t)));
            }
            self.http.get_json(&path).await?
        } else {
            let at: u64 = from.and_then(|p| p.parse().ok()).unwrap_or(0);
            self.http
                .get_json(&format!(
                    "/search?jql={}&startAt={at}&maxResults={PAGE}&fields={FIELDS}",
                    q(query)
                ))
                .await?
        };
        let items: Vec<Item> = v
            .get("issues")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|i| self.row(i))
            .collect();
        let total = v.get("total").and_then(Value::as_u64);
        let next = if self.cloud {
            Some(s(&v, "nextPageToken")).filter(|t| !t.is_empty())
        } else {
            let at = v.get("startAt").and_then(Value::as_u64).unwrap_or(0);
            let seen = at + items.len() as u64;
            total.filter(|t| seen < *t).map(|_| seen.to_string())
        };
        Ok(Found { items, next, total })
    }

    async fn item(&mut self, key: &str) -> Result<Detail> {
        let i = self
            .http
            .get_json(&format!(
                "/issue/{}?fields={FIELDS},description,labels,duedate,issuetype,reporter,created,attachment,issuelinks,comment",
                seg(key)
            ))
            .await?;
        let f = i.get("fields").cloned().unwrap_or_default();
        let (description, lossy) = self.text(f.get("description"));
        let comments = f
            .pointer("/comment/comments")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .take(MAX_COMMENTS)
            .map(|c| Comment {
                author: display(c, "author"),
                created_ms: iso_ms(&s(c, "created")),
                body: self.text(c.get("body")).0,
            })
            .collect();
        let mut fields = vec![
            (
                "type".to_string(),
                f.get("issuetype").map(|t| s(t, "name")).unwrap_or_default(),
            ),
            ("reporter".into(), display(&f, "reporter")),
        ];
        if let Some(t) = iso_ms(&s(&f, "created")) {
            fields.push(("created".into(), time_ms(t)));
        }
        let attachments = f
            .get("attachment")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|a| (s(a, "filename"), s(a, "content")))
            .collect();
        let links = f
            .get("issuelinks")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|l| {
                let ty = l.get("type")?;
                if let Some(o) = l.get("outwardIssue") {
                    Some((s(ty, "outward"), s(o, "key")))
                } else {
                    l.get("inwardIssue").map(|o| (s(ty, "inward"), s(o, "key")))
                }
            })
            .collect();
        let labels = f
            .get("labels")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|l| l.as_str().map(String::from))
            .collect();
        Ok(Detail {
            item: self.row(&i),
            fields,
            description,
            lossy,
            labels,
            due: s(&f, "duedate"),
            comments,
            comments_note: None,
            attachments,
            links,
        })
    }

    async fn transitions(&mut self, key: &str) -> Result<Vec<Transition>> {
        let v = self
            .http
            .get_json(&format!("/issue/{}/transitions", seg(key)))
            .await?;
        Ok(v.get("transitions")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|t| Transition {
                id: s(t, "id"),
                name: s(t, "name"),
            })
            .collect())
    }

    async fn transition(&mut self, key: &str, to: &Transition) -> Result<String> {
        self.http
            .send(
                self.http
                    .request(Method::POST, &format!("/issue/{}/transitions", seg(key)))
                    .json(&json!({ "transition": { "id": to.id } })),
            )
            .await?;
        Ok(format!("{key}: {}", to.name))
    }

    async fn edit(&mut self, key: &str, e: &Edit) -> Result<String> {
        let mut f = Map::new();
        if let Some(v) = &e.summary {
            f.insert("summary".into(), json!(v));
        }
        if let Some(v) = &e.description {
            f.insert("description".into(), self.body(v));
        }
        if let Some(v) = &e.assignee {
            f.insert("assignee".into(), self.person(v).await?);
        }
        if let Some(v) = &e.priority {
            f.insert("priority".into(), json!({ "name": v }));
        }
        if let Some(v) = &e.labels {
            f.insert("labels".into(), json!(v));
        }
        if let Some(v) = &e.due {
            f.insert(
                "duedate".into(),
                if v.is_empty() { Value::Null } else { json!(v) },
            );
        }
        self.http
            .send(
                self.http
                    .request(Method::PUT, &format!("/issue/{}", seg(key)))
                    .json(&json!({ "fields": f })),
            )
            .await?;
        Ok(format!("{key} saved"))
    }

    async fn comment(&mut self, key: &str, body: &str) -> Result<String> {
        self.http
            .json(
                self.http
                    .request(Method::POST, &format!("/issue/{}/comment", seg(key)))
                    .json(&json!({ "body": self.body(body) })),
            )
            .await?;
        Ok(format!("Comment added to {key}"))
    }

    async fn fields(&mut self, project: &str) -> Result<CreateForm> {
        let project = if project.is_empty() {
            self.project.clone()
        } else {
            project.to_string()
        };
        let mut kinds = Vec::new();
        if !project.is_empty() {
            let v = self
                .http
                .get_json(&format!("/issue/createmeta/{}/issuetypes", seg(&project)))
                .await?;
            // Cloud says `issueTypes`, Data Center `values`.
            kinds = v
                .get("issueTypes")
                .or_else(|| v.get("values"))
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter(|t| t.get("subtask") != Some(&Value::Bool(true)))
                .map(|t| s(t, "name"))
                .collect();
        }
        let priorities = self
            .http
            .get_json("/priority")
            .await
            .ok()
            .and_then(|v| v.as_array().cloned())
            .unwrap_or_default()
            .iter()
            .map(|p| s(p, "name"))
            .collect();
        Ok(CreateForm { kinds, priorities })
    }

    async fn create(&mut self, it: &NewItem) -> Result<String> {
        let project = if it.project.is_empty() {
            self.project.clone()
        } else {
            it.project.clone()
        };
        if project.is_empty() {
            return Err("a new Jira issue needs a project key".into());
        }
        let mut f = Map::new();
        f.insert("project".into(), json!({ "key": project }));
        f.insert(
            "issuetype".into(),
            json!({ "name": if it.kind.is_empty() { "Task" } else { &it.kind } }),
        );
        f.insert("summary".into(), json!(it.summary));
        if !it.description.trim().is_empty() {
            f.insert("description".into(), self.body(&it.description));
        }
        if !it.priority.is_empty() {
            f.insert("priority".into(), json!({ "name": it.priority }));
        }
        if !it.labels.is_empty() {
            f.insert("labels".into(), json!(it.labels));
        }
        let v = self
            .http
            .json(
                self.http
                    .request(Method::POST, "/issue")
                    .json(&json!({ "fields": f })),
            )
            .await?;
        Ok(s(&v, "key"))
    }
}
