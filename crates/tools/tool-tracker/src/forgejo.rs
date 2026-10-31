//! Forgejo (and Gitea) issues through the Forgejo API
//! (docs/tools/tracker.md §4). A connection is one repository. The API has
//! no search language, so the query bar takes GitHub's words for the
//! filters it offers (`is:open`, `label:bug`, `assignee:ada`) and searches
//! the rest as text.

use async_trait::async_trait;
use reqwest::Method;
use serde_json::{Value, json};

use crate::driver::{
    Caps, Comment, CreateForm, Detail, Driver, Edit, Found, MAX_COMMENTS, NewItem, Result,
    ServerInfo, Transition, q,
};
use crate::github::{
    forge_comment, forge_detail, forge_edit, forge_item, forge_transitions, number,
};
use crate::profile::ConnectConfig;
use hs_core::service_http::{Http, s};

/// Forgejo's default page cap (`MAX_RESPONSE_ITEMS`).
const LIMIT: usize = 50;

/// The query bar's words for the forges without a search language.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ForgeQuery {
    /// "open", "closed" or "all".
    pub state: String,
    pub labels: Vec<String>,
    pub assignee: String,
    pub text: String,
}

pub fn parse_query(query: &str) -> ForgeQuery {
    let mut out = ForgeQuery {
        state: "open".into(),
        ..Default::default()
    };
    let mut text = Vec::new();
    for w in query.split_whitespace() {
        match w.split_once(':') {
            Some(("is" | "state", v)) => {
                out.state = match v {
                    "closed" => "closed",
                    "all" => "all",
                    _ => "open",
                }
                .into()
            }
            Some(("label", v)) => out.labels.push(v.trim_matches('"').to_string()),
            Some(("assignee", v)) => out.assignee = v.trim_start_matches('@').to_string(),
            _ => text.push(w),
        }
    }
    out.text = text.join(" ");
    out
}

pub struct Forgejo {
    http: Http,
    repo: String,
    /// The signed-in user's name, for the dashboard's "assigned to me".
    me: Option<String>,
    info: ServerInfo,
}

impl Forgejo {
    pub async fn connect(cfg: &ConnectConfig) -> Result<Forgejo> {
        let http = cfg.http(&format!("{}/api/v1", cfg.url))?;
        let v = http.get_json("/version").await?;
        let r = http.get_json(&format!("/repos/{}", cfg.repo)).await?;
        if r.get("has_issues") == Some(&Value::Bool(false)) {
            return Err(format!("{} has issues turned off", cfg.repo));
        }
        let me = match &cfg.auth {
            hs_core::service_auth::Auth::None => None,
            _ => http
                .get_json("/user")
                .await
                .ok()
                .map(|u| s(&u, "login"))
                .filter(|u| !u.is_empty()),
        };
        Ok(Forgejo {
            http,
            repo: cfg.repo.clone(),
            me,
            info: ServerInfo {
                product: "Forgejo Issues".into(),
                version: s(&v, "version"),
            },
        })
    }

    async fn issue(&self, n: u64) -> Result<Value> {
        self.http
            .get_json(&format!("/repos/{}/issues/{n}", self.repo))
            .await
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

    /// Labels by name: the API takes them on their own call.
    async fn set_labels(&self, n: u64, labels: &[String]) -> Result<()> {
        self.http
            .send(
                self.http
                    .request(
                        Method::PUT,
                        &format!("/repos/{}/issues/{n}/labels", self.repo),
                    )
                    .json(&json!({ "labels": labels })),
            )
            .await?;
        Ok(())
    }
}

#[async_trait]
impl Driver for Forgejo {
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
        let mut path = format!(
            "/repos/{}/issues?type=issues&state={}&limit={LIMIT}&page={page}",
            self.repo, fq.state
        );
        if !fq.text.is_empty() {
            path.push_str(&format!("&q={}", q(&fq.text)));
        }
        if !fq.labels.is_empty() {
            path.push_str(&format!("&labels={}", q(&fq.labels.join(","))));
        }
        if !fq.assignee.is_empty() {
            path.push_str(&format!("&assigned_by={}", q(&fq.assignee)));
        }
        let resp = self
            .http
            .send(self.http.request(Method::GET, &path))
            .await?;
        let total = resp
            .headers()
            .get("x-total-count")
            .and_then(|h| h.to_str().ok())
            .and_then(|h| h.parse::<u64>().ok());
        let v: Value = resp
            .json()
            .await
            .map_err(|e| format!("the server's answer was not JSON: {e}"))?;
        let items: Vec<_> = v.as_array().into_iter().flatten().map(forge_item).collect();
        let more = match total {
            Some(t) => page * (LIMIT as u64) < t,
            None => items.len() == LIMIT,
        };
        Ok(Found {
            items,
            next: more.then(|| (page + 1).to_string()),
            total,
        })
    }

    async fn item(&mut self, key: &str) -> Result<Detail> {
        let n = number(key)?;
        let i = self.issue(n).await?;
        let mut comments: Vec<Comment> = Vec::new();
        for page in 1..=MAX_COMMENTS.div_ceil(LIMIT) {
            let v = self
                .http
                .get_json(&format!(
                    "/repos/{}/issues/{n}/comments?limit={LIMIT}&page={page}",
                    self.repo
                ))
                .await?;
            let list = v.as_array().cloned().unwrap_or_default();
            let done = list.len() < LIMIT;
            comments.extend(list.iter().map(forge_comment));
            if done {
                break;
            }
        }
        comments.truncate(MAX_COMMENTS);
        Ok(forge_detail(&i, comments))
    }

    async fn transitions(&mut self, key: &str) -> Result<Vec<Transition>> {
        let i = self.issue(number(key)?).await?;
        Ok(forge_transitions(&s(&i, "state"), false))
    }

    async fn transition(&mut self, key: &str, to: &Transition) -> Result<String> {
        let n = number(key)?;
        let state = if to.id == "open" { "open" } else { "closed" };
        self.patch(n, json!({ "state": state })).await?;
        Ok(format!("#{n} is now {}", to.name))
    }

    async fn edit(&mut self, key: &str, e: &Edit) -> Result<String> {
        let n = number(key)?;
        let mut body = forge_edit(e);
        if let Some(d) = &e.due {
            // Forgejo wants a time; an empty one clears the date.
            if d.is_empty() {
                body.insert("unset_due_date".into(), json!(true));
            } else {
                body.insert("due_date".into(), json!(format!("{d}T00:00:00Z")));
            }
        }
        if !body.is_empty() {
            self.patch(n, Value::Object(body)).await?;
        }
        if let Some(l) = &e.labels {
            self.set_labels(n, l).await?;
        }
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
                    .json(&json!({ "title": item.summary, "body": item.description })),
            )
            .await?;
        let n: u64 = v.get("number").and_then(Value::as_u64).unwrap_or_default();
        if !item.labels.is_empty() {
            self.set_labels(n, &item.labels).await?;
        }
        Ok(format!("#{n}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_query_bar_takes_github_words_and_searches_the_rest() {
        let fq = parse_query("is:closed label:bug label:\"ui\" assignee:@ada crash on start");
        assert_eq!(fq.state, "closed");
        assert_eq!(fq.labels, vec!["bug", "ui"]);
        assert_eq!(fq.assignee, "ada");
        assert_eq!(fq.text, "crash on start");
        assert_eq!(parse_query("").state, "open");
    }
}
