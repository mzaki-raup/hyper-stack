//! GitLab issues through the REST API v4 (docs/tools/tracker.md §4). A
//! connection is one project. The query bar takes the forges' words
//! (`is:open`, `label:bug`, `assignee:ada`) and searches the rest.

use async_trait::async_trait;
use reqwest::Method;
use serde_json::{Map, Value, json};

use crate::driver::{
    Caps, Comment, CreateForm, Detail, Driver, Edit, Found, Item, MAX_COMMENTS, NewItem, PAGE,
    Result, ServerInfo, Transition, iso_ms, q, time_ms,
};
use crate::forgejo::parse_query;
use crate::github::{names, number};
use crate::profile::ConnectConfig;
use hs_core::service_http::{Http, s, seg};

pub struct GitLab {
    http: Http,
    project: String,
    /// The signed-in user's name, for the dashboard's "assigned to me".
    me: Option<String>,
    info: ServerInfo,
}

fn item(i: &Value) -> Item {
    Item {
        key: format!("#{}", s(i, "iid")),
        summary: s(i, "title"),
        // GitLab says "opened"; the tab says what the forges say.
        status: match s(i, "state").as_str() {
            "opened" => "open".into(),
            other => other.into(),
        },
        assignee: names(i.get("assignees"), "username").join(", "),
        priority: String::new(),
        updated_ms: iso_ms(&s(i, "updated_at")),
        due: s(i, "due_date"),
        url: s(i, "web_url"),
    }
}

impl GitLab {
    pub async fn connect(cfg: &ConnectConfig) -> Result<GitLab> {
        let http = cfg.http(&format!("{}/api/v4", cfg.url))?;
        let project = seg(&cfg.project);
        let p = http.get_json(&format!("/projects/{project}")).await?;
        if p.get("issues_enabled") == Some(&Value::Bool(false)) {
            return Err(format!("{} has issues turned off", cfg.project));
        }
        let version = match http.get_json("/version").await {
            Ok(v) => s(&v, "version"),
            // Anonymous callers may not read the version; that is no failure.
            Err(_) => String::new(),
        };
        let me = match &cfg.auth {
            hs_core::service_auth::Auth::None => None,
            _ => http
                .get_json("/user")
                .await
                .ok()
                .map(|u| s(&u, "username"))
                .filter(|u| !u.is_empty()),
        };
        Ok(GitLab {
            http,
            project,
            me,
            info: ServerInfo {
                product: "GitLab Issues".into(),
                version,
            },
        })
    }

    async fn put(&self, n: u64, body: Value) -> Result<Value> {
        self.http
            .json(
                self.http
                    .request(
                        Method::PUT,
                        &format!("/projects/{}/issues/{n}", self.project),
                    )
                    .json(&body),
            )
            .await
    }

    /// Usernames to the ids GitLab's edit takes.
    async fn user_ids(&self, list: &str) -> Result<Vec<u64>> {
        let mut ids = Vec::new();
        for u in crate::github::split_list(list) {
            let v = self
                .http
                .get_json(&format!("/users?username={}", q(u.trim_start_matches('@'))))
                .await?;
            let id = v
                .as_array()
                .and_then(|a| a.first())
                .and_then(|x| x.get("id"))
                .and_then(Value::as_u64)
                .ok_or_else(|| format!("no GitLab user “{u}”"))?;
            ids.push(id);
        }
        Ok(ids)
    }
}

#[async_trait]
impl Driver for GitLab {
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
        "is:open".into()
    }

    fn mine_query(&self) -> Option<String> {
        self.me.as_ref().map(|u| format!("is:open assignee:{u}"))
    }

    async fn search(&mut self, query: &str, from: Option<&str>) -> Result<Found> {
        let page: u64 = from.and_then(|p| p.parse().ok()).unwrap_or(1);
        let fq = parse_query(query);
        let state = match fq.state.as_str() {
            "open" => "opened",
            other => other,
        };
        let mut path = format!(
            "/projects/{}/issues?state={state}&order_by=updated_at&per_page={PAGE}&page={page}",
            self.project
        );
        if !fq.text.is_empty() {
            path.push_str(&format!("&search={}", q(&fq.text)));
        }
        if !fq.labels.is_empty() {
            path.push_str(&format!("&labels={}", q(&fq.labels.join(","))));
        }
        if !fq.assignee.is_empty() {
            path.push_str(&format!("&assignee_username={}", q(&fq.assignee)));
        }
        let resp = self
            .http
            .send(self.http.request(Method::GET, &path))
            .await?;
        let header = |k: &str| {
            resp.headers()
                .get(k)
                .and_then(|h| h.to_str().ok())
                .map(str::to_string)
                .filter(|h| !h.is_empty())
        };
        let next = header("x-next-page");
        let total = header("x-total").and_then(|t| t.parse().ok());
        let v: Value = resp
            .json()
            .await
            .map_err(|e| format!("the server's answer was not JSON: {e}"))?;
        Ok(Found {
            items: v.as_array().into_iter().flatten().map(item).collect(),
            next,
            total,
        })
    }

    async fn item(&mut self, key: &str) -> Result<Detail> {
        let n = number(key)?;
        let i = self
            .http
            .get_json(&format!("/projects/{}/issues/{n}", self.project))
            .await?;
        let mut comments = Vec::new();
        let mut comments_note = None;
        for page in 1..=MAX_COMMENTS.div_ceil(PAGE) {
            let v = match self
                .http
                .get_json(&format!(
                    "/projects/{}/issues/{n}/notes?sort=asc&per_page={PAGE}&page={page}",
                    self.project
                ))
                .await
            {
                Ok(v) => v,
                // gitlab.com serves a public issue anonymously but not its
                // notes: the issue still opens, and says so.
                Err(e) if e.contains("(401)") => {
                    comments_note = Some("GitLab shows an issue's notes only to a signed-in user: add a token to read them.".to_string());
                    break;
                }
                Err(e) => return Err(e),
            };
            let list = v.as_array().cloned().unwrap_or_default();
            let done = list.len() < PAGE;
            comments.extend(
                list.iter()
                    // "changed the description", "added ~bug label"…
                    .filter(|c| c.get("system") != Some(&Value::Bool(true)))
                    .map(|c| Comment {
                        author: c
                            .get("author")
                            .map(|a| s(a, "username"))
                            .unwrap_or_default(),
                        created_ms: iso_ms(&s(c, "created_at")),
                        body: s(c, "body"),
                    }),
            );
            if done {
                break;
            }
        }
        comments.truncate(MAX_COMMENTS);
        let mut fields = vec![(
            "author".to_string(),
            i.get("author")
                .map(|a| s(a, "username"))
                .unwrap_or_default(),
        )];
        if let Some(t) = iso_ms(&s(&i, "created_at")) {
            fields.push(("created".into(), time_ms(t)));
        }
        if let Some(m) = i.get("milestone").filter(|m| !m.is_null()) {
            fields.push(("milestone".into(), s(m, "title")));
        }
        let labels = i
            .get("labels")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|l| l.as_str().map(String::from))
            .collect();
        Ok(Detail {
            item: item(&i),
            fields,
            description: s(&i, "description"),
            lossy: None,
            labels,
            due: s(&i, "due_date"),
            comments,
            comments_note,
            attachments: Vec::new(),
            links: Vec::new(),
        })
    }

    async fn transitions(&mut self, key: &str) -> Result<Vec<Transition>> {
        let d = self.item(key).await?;
        Ok(crate::github::forge_transitions(&d.item.status, false))
    }

    async fn transition(&mut self, key: &str, to: &Transition) -> Result<String> {
        let n = number(key)?;
        let event = if to.id == "open" { "reopen" } else { "close" };
        self.put(n, json!({ "state_event": event })).await?;
        Ok(format!("#{n} is now {}", to.name))
    }

    async fn edit(&mut self, key: &str, e: &Edit) -> Result<String> {
        let n = number(key)?;
        let mut body = Map::new();
        if let Some(v) = &e.summary {
            body.insert("title".into(), json!(v));
        }
        if let Some(v) = &e.description {
            body.insert("description".into(), json!(v));
        }
        if let Some(v) = &e.assignee {
            body.insert("assignee_ids".into(), json!(self.user_ids(v).await?));
        }
        if let Some(v) = &e.labels {
            body.insert("labels".into(), json!(v.join(",")));
        }
        if let Some(v) = &e.due {
            body.insert("due_date".into(), json!(v));
        }
        self.put(n, Value::Object(body)).await?;
        Ok(format!("#{n} saved"))
    }

    async fn comment(&mut self, key: &str, body: &str) -> Result<String> {
        let n = number(key)?;
        self.http
            .json(
                self.http
                    .request(
                        Method::POST,
                        &format!("/projects/{}/issues/{n}/notes", self.project),
                    )
                    .json(&json!({ "body": body })),
            )
            .await?;
        Ok(format!("Comment added to #{n}"))
    }

    async fn fields(&mut self, _project: &str) -> Result<CreateForm> {
        Ok(CreateForm::default())
    }

    async fn create(&mut self, it: &NewItem) -> Result<String> {
        let v = self
            .http
            .json(
                self.http
                    .request(Method::POST, &format!("/projects/{}/issues", self.project))
                    .json(&json!({
                        "title": it.summary,
                        "description": it.description,
                        "labels": it.labels.join(","),
                    })),
            )
            .await?;
        Ok(format!("#{}", s(&v, "iid")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_opened_issue_reads_as_open() {
        let i = json!({
            "iid": 7, "title": "Slow", "state": "opened",
            "assignees": [{ "username": "ada" }],
            "web_url": "https://gitlab.com/g/p/-/issues/7"
        });
        let it = item(&i);
        assert_eq!(it.key, "#7");
        assert_eq!(it.status, "open");
        assert_eq!(it.assignee, "ada");
    }
}
