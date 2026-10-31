//! TeamCity through its REST API (`/app/rest`, JSON;
//! docs/tools/automation.md §4). A job is a build configuration.

use async_trait::async_trait;
use reqwest::Method;
use serde_json::{Value, json};

use crate::driver::{
    Caps, Driver, Job, Listing, LogChunk, MAX_JOBS, MAX_RUNS, Param, ParamKind, QueueItem, Result,
    Run, RunForm, RunState, ServerInfo, slice_from,
};
use crate::profile::ConnectConfig;
use hs_core::service_http::{Http, s, seg};

pub struct TeamCity {
    http: Http,
    info: ServerInfo,
}

const BUILD_FIELDS: &str = "id,number,state,status,branchName,startDate,finishDate,queuedDate,webUrl,canceledInfo,triggered(user(username,name))";

/// TeamCity's own date form: `20261031T101500+0000`.
fn tc_ms(s: &str) -> Option<i64> {
    chrono::DateTime::parse_from_str(s, "%Y%m%dT%H%M%S%z")
        .ok()
        .map(|t| t.timestamp_millis())
}

fn run(b: &Value) -> Run {
    let state = match s(b, "state").as_str() {
        "queued" => RunState::Queued,
        "running" => RunState::Running,
        _ if b.get("canceledInfo").is_some() => RunState::Cancelled,
        _ => match s(b, "status").as_str() {
            "SUCCESS" => RunState::Success,
            "FAILURE" | "ERROR" => RunState::Failure,
            _ => RunState::Unknown,
        },
    };
    let started = tc_ms(&s(b, "startDate"));
    let finished = tc_ms(&s(b, "finishDate"));
    let who = b
        .get("triggered")
        .and_then(|t| t.get("user"))
        .map(|u| {
            let name = s(u, "name");
            if name.is_empty() {
                s(u, "username")
            } else {
                name
            }
        })
        .unwrap_or_default();
    let number = s(b, "number");
    Run {
        id: s(b, "id"),
        number: if number.is_empty() {
            format!("id {}", s(b, "id"))
        } else {
            format!("#{number}")
        },
        state,
        reference: s(b, "branchName"),
        started_ms: started.or_else(|| tc_ms(&s(b, "queuedDate"))),
        duration_ms: match (started, finished) {
            (Some(a), Some(b)) => Some(b - a),
            _ => None,
        },
        who,
        url: s(b, "webUrl"),
    }
}

impl TeamCity {
    pub async fn connect(cfg: &ConnectConfig) -> Result<TeamCity> {
        let http = cfg
            .http(&cfg.url)?
            .with_header("Accept", "application/json");
        let v = http.get_json("/app/rest/server").await?;
        Ok(TeamCity {
            info: ServerInfo {
                product: "TeamCity".into(),
                version: s(&v, "version"),
            },
            http,
        })
    }
}

#[async_trait]
impl Driver for TeamCity {
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
        // Each configuration with its newest build, in one call.
        let v = self
            .http
            .get_json(&format!(
                "/app/rest/buildTypes?fields=buildType(id,name,projectName,webUrl,builds($locator(count:1,defaultFilter:false),build({BUILD_FIELDS})))"
            ))
            .await?;
        let mut jobs: Vec<Job> = v
            .get("buildType")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|t| Job {
                id: s(t, "id"),
                name: format!("{} / {}", s(t, "projectName"), s(t, "name")),
                last: t
                    .get("builds")
                    .and_then(|b| b.get("build"))
                    .and_then(Value::as_array)
                    .and_then(|a| a.first())
                    .map(run),
                url: s(t, "webUrl"),
            })
            .collect();
        let cut = jobs.len() > MAX_JOBS;
        jobs.truncate(MAX_JOBS);
        Ok(Listing {
            jobs,
            note: cut.then(|| format!("the first {MAX_JOBS} build configurations")),
        })
    }

    async fn runs(&mut self, job: &Job) -> Result<Vec<Run>> {
        let v = self
            .http
            .get_json(&format!(
                "/app/rest/builds?locator=buildType:(id:{}),count:{MAX_RUNS},defaultFilter:false,state:any&fields=build({BUILD_FIELDS})",
                seg(&job.id)
            ))
            .await?;
        Ok(v.get("build")
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
                "/app/rest/buildTypes/id:{}/parameters?fields=property(name,value,type(rawValue))",
                seg(&job.id)
            ))
            .await?;
        let params = v
            .get("property")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|p| {
                let raw = p.get("type").map(|t| s(t, "rawValue")).unwrap_or_default();
                Param {
                    name: s(p, "name"),
                    kind: if raw.starts_with("password") {
                        ParamKind::Secret
                    } else if raw.starts_with("checkbox") {
                        ParamKind::Bool
                    } else {
                        ParamKind::Text
                    },
                    default: s(p, "value"),
                    description: String::new(),
                    required: false,
                }
            })
            .collect();
        Ok(RunForm {
            allowed: true,
            why: String::new(),
            params,
            free_vars: false,
        })
    }

    async fn trigger(&mut self, job: &Job, values: &[(String, String)]) -> Result<String> {
        let props: Vec<Value> = values
            .iter()
            .map(|(k, v)| json!({"name": k, "value": v}))
            .collect();
        let body = json!({
            "buildType": {"id": job.id},
            "properties": {"property": props},
        });
        let v = self
            .http
            .json(
                self.http
                    .request(Method::POST, "/app/rest/buildQueue")
                    .json(&body),
            )
            .await?;
        Ok(format!("queued {} as build {}", job.name, s(&v, "id")))
    }

    async fn cancel(&mut self, job: &Job, run: &Run) -> Result<String> {
        let body = json!({"comment": "Cancelled from HyperStack", "readdIntoQueue": false});
        // A queued build is cancelled in the queue, a running one where it runs.
        let path = if run.state == RunState::Queued {
            format!("/app/rest/buildQueue/id:{}", seg(&run.id))
        } else {
            format!("/app/rest/builds/id:{}", seg(&run.id))
        };
        self.http
            .send(self.http.request(Method::POST, &path).json(&body))
            .await?;
        Ok(format!("cancelled {} {}", job.name, run.number))
    }

    async fn log(&mut self, _job: &Job, run: &Run, from: u64) -> Result<LogChunk> {
        let text = self
            .http
            .get_text(&format!(
                "/downloadBuildLog.html?buildId={}&plain=true",
                seg(&run.id)
            ))
            .await?;
        Ok(slice_from(text, from, run.state.is_active()))
    }

    async fn queue(&mut self) -> Result<Vec<QueueItem>> {
        let v = self
            .http
            .get_json("/app/rest/buildQueue?fields=build(id,waitReason,queuedDate,buildType(name))")
            .await?;
        Ok(v.get("build")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|b| QueueItem {
                id: s(b, "id"),
                job: b.get("buildType").map(|t| s(t, "name")).unwrap_or_default(),
                why: s(b, "waitReason"),
                since_ms: tc_ms(&s(b, "queuedDate")),
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_build_reads_its_state_dates_and_trigger() {
        let r = run(&json!({
            "id": 12, "number": "34", "state": "finished", "status": "FAILURE",
            "branchName": "main",
            "startDate": "20261031T100000+0000", "finishDate": "20261031T100230+0000",
            "triggered": {"user": {"username": "ada"}}
        }));
        assert_eq!(r.id, "12");
        assert_eq!(r.number, "#34");
        assert_eq!(r.state, RunState::Failure);
        assert_eq!(r.duration_ms, Some(150_000));
        assert_eq!(r.who, "ada");
        let c =
            run(&json!({"id": 1, "state": "finished", "status": "UNKNOWN", "canceledInfo": {}}));
        assert_eq!(c.state, RunState::Cancelled);
        assert_eq!(
            run(&json!({"id": 2, "state": "queued"})).state,
            RunState::Queued
        );
    }
}
