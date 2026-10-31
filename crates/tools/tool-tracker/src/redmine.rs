//! Redmine through its REST API (docs/tools/tracker.md §4), API key in
//! `X-Redmine-API-Key`. The query bar takes Redmine's filters as words -
//! `project:ops status:closed assignee:me tracker:Bug` - and searches the
//! rest in the subject.

use async_trait::async_trait;
use reqwest::Method;
use serde_json::{Map, Value, json};

use crate::driver::{
    Caps, Comment, CreateForm, Detail, Driver, Edit, Found, Item, MAX_COMMENTS, NewItem, PAGE,
    Result, ServerInfo, Transition, iso_ms, q, time_ms,
};
use crate::github::number;
use crate::markup::{Lite, lite_to_md, md_to_lite};
use crate::profile::ConnectConfig;
use hs_core::service_http::{Http, s};

pub struct Redmine {
    http: Http,
    signed_in: bool,
    web: String,
    project: String,
    textile: bool,
    /// Name → id, read once: statuses, trackers, priorities.
    statuses: Vec<(String, u64)>,
    trackers: Vec<(String, u64)>,
    priorities: Vec<(String, u64)>,
    info: ServerInfo,
}

fn named(v: &Value, list: &str) -> Vec<(String, u64)> {
    v.get(list)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|x| Some((s(x, "name"), x.get("id")?.as_u64()?)))
        .collect()
}

fn lookup(list: &[(String, u64)], name: &str, what: &str) -> Result<u64> {
    list.iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, id)| *id)
        .ok_or_else(|| format!("Redmine has no {what} “{name}”"))
}

fn name_of(v: &Value, k: &str) -> String {
    v.get(k).map(|x| s(x, "name")).unwrap_or_default()
}

/// One filter in the query-string form the web UI uses.
fn filter(field: &str, op: &str, value: Option<&str>) -> String {
    let mut out = format!("&f%5B%5D={field}&op%5B{field}%5D={}", q(op));
    if let Some(v) = value {
        out.push_str(&format!("&v%5B{field}%5D%5B%5D={}", q(v)));
    }
    out
}

impl Redmine {
    pub async fn connect(cfg: &ConnectConfig) -> Result<Redmine> {
        let http = cfg.http(&cfg.url)?;
        // Readable anonymously on a public site, so this proves the URL;
        // the credential is proved by the first write or private project.
        http.get_json("/issues.json?limit=1").await?;
        let statuses = http
            .get_json("/issue_statuses.json")
            .await
            .map(|v| named(&v, "issue_statuses"))
            .unwrap_or_default();
        let trackers = http
            .get_json("/trackers.json")
            .await
            .map(|v| named(&v, "trackers"))
            .unwrap_or_default();
        let priorities = http
            .get_json("/enumerations/issue_priorities.json")
            .await
            .map(|v| named(&v, "issue_priorities"))
            .unwrap_or_default();
        Ok(Redmine {
            http,
            signed_in: cfg.auth != hs_core::service_auth::Auth::None,
            web: cfg.url.clone(),
            project: cfg.project.clone(),
            textile: cfg.textile,
            statuses,
            trackers,
            priorities,
            info: ServerInfo {
                product: "Redmine".into(),
                version: String::new(),
            },
        })
    }

    fn md_in(&self, text: &str) -> (String, Option<String>) {
        if self.textile {
            lite_to_md(text, Lite::Textile)
        } else {
            (text.to_string(), None)
        }
    }

    fn md_out(&self, md: &str) -> String {
        if self.textile {
            md_to_lite(md, Lite::Textile)
        } else {
            md.to_string()
        }
    }

    fn row(&self, i: &Value) -> Item {
        let id = s(i, "id");
        Item {
            key: format!("#{id}"),
            summary: s(i, "subject"),
            status: name_of(i, "status"),
            assignee: name_of(i, "assigned_to"),
            priority: name_of(i, "priority"),
            updated_ms: iso_ms(&s(i, "updated_on")),
            due: s(i, "due_date"),
            url: format!("{}/issues/{id}", self.web),
        }
    }

    /// The query bar's words as Redmine's issue filters.
    fn query_path(&self, query: &str) -> Result<String> {
        let mut project = self.project.clone();
        let mut status = filter("status_id", "o", None);
        let mut rest = String::new();
        let mut text = Vec::new();
        for w in query.split_whitespace() {
            match w.split_once(':') {
                Some(("project", v)) => project = v.to_string(),
                Some(("status", v)) => {
                    status = match v {
                        "open" => filter("status_id", "o", None),
                        "closed" => filter("status_id", "c", None),
                        "all" | "*" => filter("status_id", "*", None),
                        name => {
                            let id = lookup(&self.statuses, &name.replace('_', " "), "status")?;
                            filter("status_id", "=", Some(&id.to_string()))
                        }
                    }
                }
                Some(("assignee", v)) => rest.push_str(&filter("assigned_to_id", "=", Some(v))),
                Some(("tracker", v)) => {
                    let id = lookup(&self.trackers, v, "tracker")?;
                    rest.push_str(&filter("tracker_id", "=", Some(&id.to_string())));
                }
                _ => text.push(w),
            }
        }
        if !text.is_empty() {
            rest.push_str(&filter("subject", "~", Some(&text.join(" "))));
        }
        let mut path = format!("/issues.json?set_filter=1&sort=updated_on:desc{status}{rest}");
        if !project.is_empty() {
            path.push_str(&format!("&project_id={}", q(&project)));
        }
        Ok(path)
    }

    async fn put(&self, n: u64, issue: Map<String, Value>) -> Result<()> {
        self.http
            .send(
                self.http
                    .request(Method::PUT, &format!("/issues/{n}.json"))
                    .json(&json!({ "issue": issue })),
            )
            .await?;
        Ok(())
    }

    /// "me", an id, or "" to clear.
    async fn user_id(&self, who: &str) -> Result<Value> {
        let who = who.trim();
        if who.is_empty() {
            return Ok(json!(""));
        }
        if let Ok(id) = who.parse::<u64>() {
            return Ok(json!(id));
        }
        if who == "me" {
            let v = self.http.get_json("/users/current.json").await?;
            return v
                .pointer("/user/id")
                .cloned()
                .ok_or_else(|| "Redmine did not say who you are".to_string());
        }
        Err(format!(
            "Redmine takes the assignee as “me” or a user id, not “{who}”"
        ))
    }
}

#[async_trait]
impl Driver for Redmine {
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
        "status:open".into()
    }

    fn mine_query(&self) -> Option<String> {
        self.signed_in.then(|| "status:open assignee:me".into())
    }

    async fn search(&mut self, query: &str, from: Option<&str>) -> Result<Found> {
        let offset: u64 = from.and_then(|p| p.parse().ok()).unwrap_or(0);
        let path = format!("{}&limit={PAGE}&offset={offset}", self.query_path(query)?);
        let v = self.http.get_json(&path).await?;
        let items: Vec<Item> = v
            .get("issues")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|i| self.row(i))
            .collect();
        let total = v.get("total_count").and_then(Value::as_u64);
        let seen = offset + items.len() as u64;
        Ok(Found {
            next: total.filter(|t| seen < *t).map(|_| seen.to_string()),
            items,
            total,
        })
    }

    async fn item(&mut self, key: &str) -> Result<Detail> {
        let n = number(key)?;
        let v = self
            .http
            .get_json(&format!(
                "/issues/{n}.json?include=journals,attachments,relations"
            ))
            .await?;
        let i = v.get("issue").cloned().unwrap_or_default();
        let (description, lossy) = self.md_in(&s(&i, "description"));
        let comments = i
            .get("journals")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|j| !s(j, "notes").trim().is_empty())
            .take(MAX_COMMENTS)
            .map(|j| Comment {
                author: name_of(j, "user"),
                created_ms: iso_ms(&s(j, "created_on")),
                body: self.md_in(&s(j, "notes")).0,
            })
            .collect();
        let mut fields = vec![
            ("project".to_string(), name_of(&i, "project")),
            ("type".into(), name_of(&i, "tracker")),
            ("author".into(), name_of(&i, "author")),
        ];
        if let Some(t) = iso_ms(&s(&i, "created_on")) {
            fields.push(("created".into(), time_ms(t)));
        }
        fields.push(("done".into(), format!("{}%", s(&i, "done_ratio"))));
        let attachments = i
            .get("attachments")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|a| (s(a, "filename"), s(a, "content_url")))
            .collect();
        let links = i
            .get("relations")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|r| {
                let other = if s(r, "issue_id") == n.to_string() {
                    s(r, "issue_to_id")
                } else {
                    s(r, "issue_id")
                };
                (s(r, "relation_type").replace('_', " "), format!("#{other}"))
            })
            .collect();
        Ok(Detail {
            item: self.row(&i),
            fields,
            description,
            lossy,
            labels: Vec::new(),
            due: s(&i, "due_date"),
            comments,
            comments_note: None,
            attachments,
            links,
        })
    }

    async fn transitions(&mut self, key: &str) -> Result<Vec<Transition>> {
        let n = number(key)?;
        let v = self
            .http
            .get_json(&format!("/issues/{n}.json?include=allowed_statuses"))
            .await?;
        let current = v
            .pointer("/issue/status/id")
            .and_then(Value::as_u64)
            .unwrap_or_default();
        // Redmine before 5.0 does not list them; offer every status then.
        let allowed = named(v.get("issue").unwrap_or(&Value::Null), "allowed_statuses");
        let list = if allowed.is_empty() {
            self.statuses.clone()
        } else {
            allowed
        };
        Ok(list
            .into_iter()
            .filter(|(_, id)| *id != current)
            .map(|(name, id)| Transition {
                id: id.to_string(),
                name,
            })
            .collect())
    }

    async fn transition(&mut self, key: &str, to: &Transition) -> Result<String> {
        let n = number(key)?;
        let mut issue = Map::new();
        issue.insert(
            "status_id".into(),
            json!(to.id.parse::<u64>().map_err(|_| "not a Redmine status")?),
        );
        self.put(n, issue).await?;
        Ok(format!("#{n} is now {}", to.name))
    }

    async fn edit(&mut self, key: &str, e: &Edit) -> Result<String> {
        let n = number(key)?;
        let mut issue = Map::new();
        if let Some(v) = &e.summary {
            issue.insert("subject".into(), json!(v));
        }
        if let Some(v) = &e.description {
            issue.insert("description".into(), json!(self.md_out(v)));
        }
        if let Some(v) = &e.assignee {
            issue.insert("assigned_to_id".into(), self.user_id(v).await?);
        }
        if let Some(v) = &e.priority {
            issue.insert(
                "priority_id".into(),
                json!(lookup(&self.priorities, v, "priority")?),
            );
        }
        if let Some(v) = &e.due {
            issue.insert("due_date".into(), json!(v));
        }
        if e.labels.is_some() {
            return Err("Redmine issues have no labels".into());
        }
        self.put(n, issue).await?;
        Ok(format!("#{n} saved"))
    }

    async fn comment(&mut self, key: &str, body: &str) -> Result<String> {
        let n = number(key)?;
        let mut issue = Map::new();
        issue.insert("notes".into(), json!(self.md_out(body)));
        self.put(n, issue).await?;
        Ok(format!("Note added to #{n}"))
    }

    async fn fields(&mut self, project: &str) -> Result<CreateForm> {
        let project = if project.is_empty() {
            &self.project
        } else {
            project
        };
        // A project's own trackers where it names one; else all of them.
        let kinds = if project.is_empty() {
            self.trackers.iter().map(|(n, _)| n.clone()).collect()
        } else {
            let v = self
                .http
                .get_json(&format!("/projects/{}.json?include=trackers", q(project)))
                .await?;
            named(v.get("project").unwrap_or(&Value::Null), "trackers")
                .into_iter()
                .map(|(n, _)| n)
                .collect()
        };
        Ok(CreateForm {
            kinds,
            priorities: self.priorities.iter().map(|(n, _)| n.clone()).collect(),
        })
    }

    async fn create(&mut self, it: &NewItem) -> Result<String> {
        let project = if it.project.is_empty() {
            self.project.clone()
        } else {
            it.project.clone()
        };
        if project.is_empty() {
            return Err("a new Redmine issue needs a project".into());
        }
        let mut issue = Map::new();
        issue.insert("project_id".into(), json!(project));
        issue.insert("subject".into(), json!(it.summary));
        issue.insert("description".into(), json!(self.md_out(&it.description)));
        if !it.kind.is_empty() {
            issue.insert(
                "tracker_id".into(),
                json!(lookup(&self.trackers, &it.kind, "tracker")?),
            );
        }
        if !it.priority.is_empty() {
            issue.insert(
                "priority_id".into(),
                json!(lookup(&self.priorities, &it.priority, "priority")?),
            );
        }
        let v = self
            .http
            .json(
                self.http
                    .request(Method::POST, "/issues.json")
                    .json(&json!({ "issue": issue })),
            )
            .await?;
        Ok(format!(
            "#{}",
            v.pointer("/issue/id")
                .map(|i| i.to_string())
                .unwrap_or_default()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_filter_is_written_as_the_web_ui_writes_it() {
        assert_eq!(
            filter("subject", "~", Some("disk full")),
            "&f%5B%5D=subject&op%5Bsubject%5D=~&v%5Bsubject%5D%5B%5D=disk%20full"
        );
        assert_eq!(
            filter("status_id", "*", None),
            "&f%5B%5D=status_id&op%5Bstatus_id%5D=%2A"
        );
    }
}
