//! Jenkins through its JSON API (docs/tools/automation.md §4).
//!
//! Folders and multibranch projects are flattened into `folder/job` names.
//! A username with an API token is sent as HTTP basic, which Jenkins
//! exempts from its CSRF crumb; a password needs a crumb, and the crumb is
//! tied to the session it was issued in, so the session cookie goes back
//! with it.

use async_trait::async_trait;
use reqwest::Method;
use serde_json::Value;

use crate::driver::{
    Caps, Driver, Job, Listing, LogChunk, MAX_JOBS, MAX_RUNS, Param, ParamKind, QueueItem, Result,
    Run, RunForm, RunState, ServerInfo,
};
use crate::profile::ConnectConfig;
use hs_core::service_auth::Auth;
use hs_core::service_http::{Http, s, seg};

/// How deep folders are followed.
const DEPTH: usize = 4;

pub struct Jenkins {
    http: Http,
    password_auth: bool,
    info: ServerInfo,
}

const BUILD: &str = "number,result,timestamp,duration,building,url";

/// `jobs[…,jobs[…,jobs[…]]]`, `depth` levels deep.
fn tree(depth: usize) -> String {
    let fields = format!("name,url,_class,buildable,lastBuild[{BUILD}]");
    if depth <= 1 {
        format!("jobs[{fields}]")
    } else {
        format!("jobs[{fields},{}]", tree(depth - 1))
    }
}

/// A job's URL path from its flattened name: `a/b` → `/job/a/job/b`.
fn job_path(id: &str) -> String {
    id.split('/').map(|p| format!("/job/{}", seg(p))).collect()
}

fn state(b: &Value) -> RunState {
    if b.get("building").and_then(Value::as_bool) == Some(true) {
        return RunState::Running;
    }
    match b.get("result").and_then(Value::as_str) {
        Some("SUCCESS") => RunState::Success,
        Some("FAILURE") => RunState::Failure,
        Some("UNSTABLE") => RunState::Unstable,
        Some("ABORTED") => RunState::Cancelled,
        Some("NOT_BUILT") => RunState::Skipped,
        _ => RunState::Unknown,
    }
}

fn run(b: &Value) -> Run {
    let who = b
        .get("actions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|a| a.get("causes").and_then(Value::as_array))
        .flatten()
        .find_map(|c| {
            c.get("userName")
                .or_else(|| c.get("shortDescription"))
                .and_then(Value::as_str)
                .map(String::from)
        })
        .unwrap_or_default();
    let number = s(b, "number");
    Run {
        id: number.clone(),
        number: format!("#{number}"),
        state: state(b),
        reference: String::new(),
        started_ms: b.get("timestamp").and_then(Value::as_i64),
        duration_ms: b.get("duration").and_then(Value::as_i64).filter(|d| *d > 0),
        who,
        url: s(b, "url"),
    }
}

/// Walk the folder tree into flat jobs.
fn flatten(list: &Value, prefix: &str, out: &mut Vec<Job>) {
    for j in list.as_array().into_iter().flatten() {
        if out.len() >= MAX_JOBS {
            return;
        }
        let name = s(j, "name");
        let id = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        if let Some(children) = j.get("jobs") {
            flatten(children, &id, out);
            continue;
        }
        out.push(Job {
            id: id.clone(),
            name: id,
            last: j.get("lastBuild").filter(|b| !b.is_null()).map(run),
            url: s(j, "url"),
        });
    }
}

fn param(p: &Value) -> Param {
    let default = p
        .get("defaultParameterValue")
        .map(|d| s(d, "value"))
        .unwrap_or_default();
    let kind = match p.get("type").and_then(Value::as_str).unwrap_or_default() {
        "BooleanParameterDefinition" => ParamKind::Bool,
        "ChoiceParameterDefinition" => ParamKind::Choice(
            p.get("choices")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|c| c.as_str().map(String::from))
                .collect(),
        ),
        "TextParameterDefinition" => ParamKind::LongText,
        "PasswordParameterDefinition" => ParamKind::Secret,
        _ => ParamKind::Text,
    };
    Param {
        name: s(p, "name"),
        kind,
        default,
        description: s(p, "description"),
        required: false,
    }
}

/// Jenkins hides "console notes" (serialised hyperlinks and markup) in the
/// plain-text log as `ESC[8mha:<base64>ESC[0m`; its own page never shows
/// them, and neither should this. A note cut by a chunk boundary is left
/// as it came - the offsets are Jenkins' and must not move.
pub fn strip_notes(text: &str) -> String {
    const START: &str = "\u{1b}[8mha:";
    const END: &str = "\u{1b}[0m";
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find(START) {
        let Some(j) = rest[i..].find(END) else {
            break;
        };
        out.push_str(&rest[..i]);
        rest = &rest[i + j + END.len()..];
    }
    out.push_str(rest);
    out
}

impl Jenkins {
    pub async fn connect(cfg: &ConnectConfig) -> Result<Jenkins> {
        let http = cfg.http(&cfg.url)?;
        let resp = http
            .send(http.request(Method::GET, "/api/json?tree=mode"))
            .await?;
        let version = resp
            .headers()
            .get("x-jenkins")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_string();
        Ok(Jenkins {
            http,
            password_auth: matches!(cfg.auth, Auth::Password { .. }),
            info: ServerInfo {
                product: "Jenkins".into(),
                version,
            },
        })
    }

    /// A POST, with a crumb and its session when signed in by password.
    async fn post(&self, path: &str, form: &[(String, String)]) -> Result<reqwest::Response> {
        let mut rb = self.http.request(Method::POST, path).form(form);
        if self.password_auth {
            let resp = self
                .http
                .send(self.http.request(Method::GET, "/crumbIssuer/api/json"))
                .await;
            // No crumb issuer means CSRF protection is off.
            if let Ok(resp) = resp {
                let cookie: Vec<String> = resp
                    .headers()
                    .get_all("set-cookie")
                    .iter()
                    .filter_map(|v| v.to_str().ok())
                    .filter_map(|c| c.split(';').next().map(String::from))
                    .collect();
                let v: Value = resp.json().await.map_err(|e| e.to_string())?;
                rb = rb.header(s(&v, "crumbRequestField"), s(&v, "crumb"));
                if !cookie.is_empty() {
                    rb = rb.header("Cookie", cookie.join("; "));
                }
            }
        }
        self.http.send(rb).await
    }
}

#[async_trait]
impl Driver for Jenkins {
    fn info(&self) -> &ServerInfo {
        &self.info
    }

    fn caps(&self) -> Caps {
        Caps {
            run: true,
            cancel: true,
            queue: true,
            log: true,
        }
    }

    async fn jobs(&mut self) -> Result<Listing> {
        let v = self
            .http
            .get_json(&format!("/api/json?tree={}", tree(DEPTH)))
            .await?;
        let mut jobs = Vec::new();
        flatten(v.get("jobs").unwrap_or(&Value::Null), "", &mut jobs);
        let note = (jobs.len() >= MAX_JOBS).then(|| format!("the first {MAX_JOBS} jobs"));
        Ok(Listing { jobs, note })
    }

    async fn runs(&mut self, job: &Job) -> Result<Vec<Run>> {
        let v = self
            .http
            .get_json(&format!(
                "{}/api/json?tree=builds[{BUILD},actions[causes[userName,shortDescription]]]{{0,{MAX_RUNS}}}",
                job_path(&job.id)
            ))
            .await?;
        Ok(v.get("builds")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(run)
            .collect())
    }

    async fn form(&mut self, job: &Job) -> Result<RunForm> {
        let v = self
            .http
            .get_json(&format!(
                "{}/api/json?tree=buildable,property[parameterDefinitions[name,type,description,choices,defaultParameterValue[value]]]",
                job_path(&job.id)
            ))
            .await?;
        let params: Vec<Param> = v
            .get("property")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|p| p.get("parameterDefinitions").and_then(Value::as_array))
            .flatten()
            .map(param)
            .collect();
        let buildable = v.get("buildable").and_then(Value::as_bool) != Some(false);
        Ok(RunForm {
            allowed: buildable,
            why: if buildable {
                String::new()
            } else {
                "this job is disabled".into()
            },
            params,
            free_vars: false,
        })
    }

    async fn trigger(&mut self, job: &Job, values: &[(String, String)]) -> Result<String> {
        let path = if values.is_empty() {
            format!("{}/build", job_path(&job.id))
        } else {
            format!("{}/buildWithParameters", job_path(&job.id))
        };
        let resp = self.post(&path, values).await?;
        // Jenkins answers with the queue item, not a build number.
        let item = resp
            .headers()
            .get("location")
            .and_then(|v| v.to_str().ok())
            .and_then(|l| l.trim_end_matches('/').rsplit('/').next().map(String::from))
            .unwrap_or_default();
        Ok(if item.is_empty() {
            format!("queued {}", job.name)
        } else {
            format!("queued {} as item {item}", job.name)
        })
    }

    async fn cancel(&mut self, job: &Job, run: &Run) -> Result<String> {
        self.post(&format!("{}/{}/stop", job_path(&job.id), run.id), &[])
            .await?;
        Ok(format!("stopped {} {}", job.name, run.number))
    }

    async fn log(&mut self, job: &Job, run: &Run, from: u64) -> Result<LogChunk> {
        let resp = self
            .http
            .send(self.http.request(
                Method::GET,
                &format!(
                    "{}/{}/logText/progressiveText?start={from}",
                    job_path(&job.id),
                    run.id
                ),
            ))
            .await?;
        let header = |name: &str| {
            resp.headers()
                .get(name)
                .and_then(|v| v.to_str().ok())
                .map(String::from)
        };
        let next = header("x-text-size").and_then(|v| v.parse().ok());
        let more = header("x-more-data").as_deref() == Some("true");
        let raw = resp.text().await.map_err(|e| e.to_string())?;
        Ok(LogChunk {
            next: next.unwrap_or(from + raw.len() as u64),
            text: strip_notes(&raw),
            more,
        })
    }

    async fn queue(&mut self) -> Result<Vec<QueueItem>> {
        let v = self
            .http
            .get_json("/queue/api/json?tree=items[id,why,inQueueSince,task[name,url]]")
            .await?;
        Ok(v.get("items")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|i| QueueItem {
                id: s(i, "id"),
                job: i.get("task").map(|t| s(t, "name")).unwrap_or_default(),
                why: s(i, "why"),
                since_ms: i.get("inQueueSince").and_then(Value::as_i64),
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn folders_are_flattened_into_slash_names() {
        let list = json!([
            {"name": "deploy", "url": "u", "jobs": [
                {"name": "staging", "url": "u2", "lastBuild": {"number": 7, "result": "FAILURE", "timestamp": 1, "duration": 2000, "building": false}}
            ]},
            {"name": "unit", "url": "u3", "lastBuild": null}
        ]);
        let mut jobs = Vec::new();
        flatten(&list, "", &mut jobs);
        assert_eq!(jobs.len(), 2);
        assert_eq!(jobs[0].name, "deploy/staging");
        assert_eq!(jobs[0].last.as_ref().unwrap().state, RunState::Failure);
        assert_eq!(jobs[0].last.as_ref().unwrap().number, "#7");
        assert!(jobs[1].last.is_none());
        assert_eq!(job_path("deploy/staging"), "/job/deploy/job/staging");
    }

    #[test]
    fn a_building_run_is_running_whatever_its_result() {
        assert_eq!(
            state(&json!({"building": true, "result": null})),
            RunState::Running
        );
        assert_eq!(state(&json!({"result": "ABORTED"})), RunState::Cancelled);
    }

    #[test]
    fn parameters_keep_their_kind_and_default() {
        let p = param(&json!({
            "name": "ENV", "type": "ChoiceParameterDefinition",
            "choices": ["staging", "prod"], "defaultParameterValue": {"value": "staging"}
        }));
        assert_eq!(
            p.kind,
            ParamKind::Choice(vec!["staging".into(), "prod".into()])
        );
        assert_eq!(p.default, "staging");
        assert_eq!(
            param(&json!({"name": "DRY", "type": "BooleanParameterDefinition", "defaultParameterValue": {"value": true}})).default,
            "true"
        );
    }

    #[test]
    fn console_notes_are_taken_out_of_the_text() {
        let raw = "Aborted by \u{1b}[8mha:////4J4F+abc=\u{1b}[0mdev\nFinished: ABORTED\n";
        assert_eq!(strip_notes(raw), "Aborted by dev\nFinished: ABORTED\n");
        // A note with no end is left alone.
        assert_eq!(strip_notes("a\u{1b}[8mha:xyz"), "a\u{1b}[8mha:xyz");
    }

    #[test]
    fn the_tree_query_follows_folders_to_its_depth() {
        assert_eq!(tree(2).matches("jobs[").count(), 2);
    }
}
