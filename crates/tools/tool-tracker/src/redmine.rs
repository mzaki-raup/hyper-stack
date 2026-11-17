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
use hs_core::service_http::{Http, s, seg};

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
    /// The members of the project last asked about, name → user id, so a
    /// name picked from the Assignee list resolves without a second call
    /// (FEAT-0095).
    members: Vec<(String, u64)>,
    info: ServerInfo,
}

/// The `name=value` pairs of a response's `Set-Cookie` headers, as a
/// `Cookie` header sends them back.
fn cookies(resp: &reqwest::Response) -> String {
    resp.headers()
        .get_all(reqwest::header::SET_COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .filter_map(|v| v.split(';').next())
        .map(str::trim)
        .collect::<Vec<_>>()
        .join("; ")
}

/// The form token of Redmine's sign-in page.
fn login_token(html: &str) -> Option<&str> {
    let at = html.find("name=\"authenticity_token\"")?;
    let rest = &html[at..];
    let value = rest.find("value=\"")? + "value=\"".len();
    let end = rest[value..].find('"')?;
    Some(&rest[value..value + end])
}

/// The version Administration → Information names ("Redmine
/// 6.1.5.stable" gives `6.1.5.stable`); empty when the page is not that
/// page (the sign-in form a non-admin is sent back to).
pub fn info_page_version(html: &str) -> String {
    let mut rest = html;
    while let Some(at) = rest.find("Redmine ") {
        let tail = &rest[at + "Redmine ".len()..];
        if tail.starts_with(|c: char| c.is_ascii_digit()) {
            return tail
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '.')
                .collect();
        }
        rest = tail;
    }
    String::new()
}

/// Redmine names its version nowhere in its API or its headers, only on
/// Administration → Information, which is a page for an administrator
/// signed in through the form (an API key is not taken there). With a
/// username and password the driver signs in once, reads that page and
/// forgets the session; Redmine keeps a session in its cookie, so there
/// is nothing left on the server. Anything else - an API key, an account
/// that is no administrator, a changed page - gives no version (CR-0101).
async fn admin_version(
    base: &str,
    user: &str,
    password: &str,
    timeout: std::time::Duration,
) -> String {
    let read = async {
        // No redirects: the session cookie comes with the 302 after the
        // sign-in, which a followed redirect would drop.
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(timeout)
            .build()
            .ok()?;
        let form = http.get(format!("{base}/login")).send().await.ok()?;
        let cookie = cookies(&form);
        let page = form.text().await.ok()?;
        let token = login_token(&page)?;
        let body = format!(
            "authenticity_token={}&username={}&password={}",
            seg(token),
            seg(user),
            seg(password)
        );
        let signed_in = http
            .post(format!("{base}/login"))
            .header("Cookie", cookie)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(body)
            .send()
            .await
            .ok()?;
        // A wrong password shows the form again (200), not a redirect.
        if !signed_in.status().is_redirection() {
            return None;
        }
        let info = http
            .get(format!("{base}/admin/info"))
            .header("Cookie", cookies(&signed_in))
            .send()
            .await
            .ok()?;
        if !info.status().is_success() {
            return None;
        }
        Some(info_page_version(&info.text().await.ok()?))
    };
    read.await.unwrap_or_default()
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
            members: Vec::new(),
            info: ServerInfo {
                product: "Redmine".into(),
                version: match &cfg.auth {
                    hs_core::service_auth::Auth::Password { user, password } => {
                        admin_version(&cfg.url, user, password, cfg.timeout).await
                    }
                    _ => String::new(),
                },
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
        // A name from the Assignee list (FEAT-0095).
        if let Some((_, id)) = self
            .members
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(who))
        {
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
            "Redmine takes the assignee as “me”, a user id or a project member's name, not “{who}”"
        ))
    }
}

/// The people in a `/projects/{id}/memberships.json` answer, name → user
/// id. Group memberships carry no user and are left out.
fn member_names(v: &Value) -> Vec<(String, u64)> {
    v.get("memberships")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|m| {
            let u = m.get("user")?;
            Some((s(u, "name"), u.get("id")?.as_u64()?))
        })
        .filter(|(n, _)| !n.is_empty())
        .collect()
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
        // The project's members are who an item can go to (FEAT-0095);
        // a site that hides them leaves the field free text.
        if !project.is_empty() {
            self.members = self
                .http
                .get_json(&format!(
                    "/projects/{}/memberships.json?limit=100",
                    q(project)
                ))
                .await
                .map(|v| member_names(&v))
                .unwrap_or_default();
        }
        Ok(CreateForm {
            kinds,
            priorities: self.priorities.iter().map(|(n, _)| n.clone()).collect(),
            assignees: self.members.iter().map(|(n, _)| n.clone()).collect(),
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

    /// CR-0101: the version comes from Administration → Information.
    #[test]
    fn the_information_page_names_the_version() {
        let page = "<title>Information - Redmine</title><table>            <tr><td class=\"name\">Redmine 6.1.5.stable</td></tr></table>";
        assert_eq!(info_page_version(page), "6.1.5.stable");
        // The sign-in form a non-admin lands on names no version.
        assert_eq!(
            info_page_version("<title>Redmine</title> Powered by Redmine"),
            ""
        );
        assert_eq!(info_page_version(""), "");
    }

    #[test]
    fn the_sign_in_form_gives_its_token() {
        let page = "<form action=\"/login\"><input type=\"hidden\"             name=\"authenticity_token\" value=\"aB+c/d==\" autocomplete=\"off\" /></form>";
        assert_eq!(login_token(page), Some("aB+c/d=="));
        assert_eq!(login_token("<form></form>"), None);
    }

    #[test]
    fn project_members_become_the_assignee_list_without_the_groups() {
        // FEAT-0095: the Assignee field is fed from the project's people.
        let v = json!({"memberships": [
            {"id": 1, "user": {"id": 5, "name": "Ada Lovelace"}},
            {"id": 2, "group": {"id": 9, "name": "Ops"}},
            {"id": 3, "user": {"id": 7, "name": "Bob"}}
        ]});
        assert_eq!(
            member_names(&v),
            vec![("Ada Lovelace".to_string(), 5), ("Bob".to_string(), 7)]
        );
        assert!(member_names(&json!({})).is_empty());
    }

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
