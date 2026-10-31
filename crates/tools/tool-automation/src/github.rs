//! GitHub Actions through the REST API (docs/tools/automation.md §4). A
//! connection is one repository; a job is a workflow.
//!
//! Only a workflow with a `workflow_dispatch` trigger can be run, so the
//! Run form reads the workflow file for that trigger and its inputs.

use async_trait::async_trait;
use base64::Engine;
use reqwest::Method;
use serde_json::{Map, Value, json};

use crate::driver::{
    Caps, Driver, Job, Listing, LogChunk, MAX_RUNS, Param, ParamKind, Result, Run, RunForm,
    RunState, ServerInfo, iso_ms, slice_from,
};
use crate::http::{Http, s, seg};
use crate::profile::ConnectConfig;

pub struct GitHub {
    http: Http,
    repo: String,
    default_branch: String,
    /// No token: GitHub then refuses every log, so say that once.
    anonymous: bool,
    info: ServerInfo,
}

/// A run's state from GitHub's `status` and `conclusion`. Shared with
/// Forgejo, which copies GitHub's words.
pub fn gh_state(status: &str, conclusion: &str) -> RunState {
    match status {
        "queued" | "waiting" | "pending" | "requested" => RunState::Queued,
        "in_progress" | "running" => RunState::Running,
        _ => match conclusion {
            "success" => RunState::Success,
            "failure" | "timed_out" | "startup_failure" => RunState::Failure,
            "cancelled" => RunState::Cancelled,
            "skipped" | "neutral" => RunState::Skipped,
            "action_required" | "stale" => RunState::Unstable,
            _ => RunState::Unknown,
        },
    }
}

fn run(r: &Value) -> Run {
    let started = iso_ms(&s(r, "run_started_at")).or_else(|| iso_ms(&s(r, "created_at")));
    let state = gh_state(&s(r, "status"), &s(r, "conclusion"));
    let ended = iso_ms(&s(r, "updated_at"));
    Run {
        id: s(r, "id"),
        number: format!("#{}", s(r, "run_number")),
        reference: s(r, "head_branch"),
        started_ms: started,
        duration_ms: match (started, ended) {
            (Some(a), Some(b)) if !state.is_active() => Some(b - a),
            _ => None,
        },
        who: r.get("actor").map(|a| s(a, "login")).unwrap_or_default(),
        url: s(r, "html_url"),
        state,
    }
}

/// The inputs of a workflow's `workflow_dispatch`, or `None` when it has
/// no such trigger. `on:` may be a word, a list or a map.
pub fn dispatch_inputs(yaml: &str) -> std::result::Result<Option<Vec<Param>>, String> {
    let doc: serde_norway::Value = serde_norway::from_str(yaml).map_err(|e| e.to_string())?;
    let on = doc
        .get("on")
        // A YAML 1.1 reader turns a bare `on` into `true`.
        .or_else(|| doc.get(serde_norway::Value::Bool(true)));
    let Some(on) = on else {
        return Ok(None);
    };
    match on {
        serde_norway::Value::String(s) if s == "workflow_dispatch" => Ok(Some(Vec::new())),
        serde_norway::Value::Sequence(list) => Ok(list
            .iter()
            .any(|v| v.as_str() == Some("workflow_dispatch"))
            .then(Vec::new)),
        serde_norway::Value::Mapping(m) => {
            let Some(wd) = m.get("workflow_dispatch") else {
                return Ok(None);
            };
            let mut params = Vec::new();
            if let Some(inputs) = wd.get("inputs").and_then(|i| i.as_mapping()) {
                for (name, spec) in inputs {
                    let text = |k: &str| match spec.get(k) {
                        Some(serde_norway::Value::String(s)) => s.clone(),
                        Some(serde_norway::Value::Bool(b)) => b.to_string(),
                        Some(serde_norway::Value::Number(n)) => n.to_string(),
                        _ => String::new(),
                    };
                    let kind = match text("type").as_str() {
                        "boolean" => ParamKind::Bool,
                        "choice" => ParamKind::Choice(
                            spec.get("options")
                                .and_then(|o| o.as_sequence())
                                .into_iter()
                                .flatten()
                                .filter_map(|o| o.as_str().map(String::from))
                                .collect(),
                        ),
                        _ => ParamKind::Text,
                    };
                    params.push(Param {
                        name: name.as_str().unwrap_or_default().to_string(),
                        kind,
                        default: text("default"),
                        description: text("description"),
                        required: spec.get("required").and_then(|r| r.as_bool()) == Some(true),
                    });
                }
            }
            Ok(Some(params))
        }
        _ => Ok(None),
    }
}

/// The name of the form field that carries the ref to run on.
pub const REF: &str = "ref";

impl GitHub {
    pub async fn connect(cfg: &ConnectConfig) -> Result<GitHub> {
        let http = Http::new(cfg, &cfg.url)?
            .with_header("Accept", "application/vnd.github+json")
            .with_header("X-GitHub-Api-Version", "2022-11-28");
        let v = http.get_json(&format!("/repos/{}", cfg.repo)).await?;
        Ok(GitHub {
            anonymous: matches!(cfg.auth, hs_core::service_auth::Auth::None),
            default_branch: s(&v, "default_branch"),
            repo: cfg.repo.clone(),
            info: ServerInfo {
                product: "GitHub Actions".into(),
                version: String::new(),
            },
            http,
        })
    }
}

#[async_trait]
impl Driver for GitHub {
    fn info(&self) -> &ServerInfo {
        &self.info
    }

    fn caps(&self) -> Caps {
        Caps {
            run: true,
            cancel: true,
            queue: false,
            log: true,
        }
    }

    async fn jobs(&mut self) -> Result<Listing> {
        let wf = self
            .http
            .get_json(&format!(
                "/repos/{}/actions/workflows?per_page=100",
                self.repo
            ))
            .await?;
        // The newest runs, to give each workflow its last one.
        let runs = self
            .http
            .get_json(&format!("/repos/{}/actions/runs?per_page=100", self.repo))
            .await?;
        let runs: Vec<&Value> = runs
            .get("workflow_runs")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .collect();
        let jobs = wf
            .get("workflows")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|w| {
                let id = s(w, "id");
                Job {
                    last: runs
                        .iter()
                        .find(|r| s(r, "workflow_id") == id)
                        .map(|r| run(r)),
                    id,
                    name: s(w, "name"),
                    url: s(w, "html_url"),
                }
            })
            .collect();
        Ok(Listing { jobs, note: None })
    }

    async fn runs(&mut self, job: &Job) -> Result<Vec<Run>> {
        let v = self
            .http
            .get_json(&format!(
                "/repos/{}/actions/workflows/{}/runs?per_page={MAX_RUNS}",
                self.repo,
                seg(&job.id)
            ))
            .await?;
        Ok(v.get("workflow_runs")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(run)
            .collect())
    }

    async fn form(&mut self, job: &Job) -> Result<RunForm> {
        let w = self
            .http
            .get_json(&format!(
                "/repos/{}/actions/workflows/{}",
                self.repo,
                seg(&job.id)
            ))
            .await?;
        let path = s(&w, "path");
        let file = self
            .http
            .get_json(&format!("/repos/{}/contents/{path}", self.repo))
            .await?;
        let raw = s(&file, "content").replace('\n', "");
        let yaml = base64::engine::general_purpose::STANDARD
            .decode(raw)
            .map_err(|e| e.to_string())?;
        let yaml = String::from_utf8_lossy(&yaml);
        let Some(mut params) = dispatch_inputs(&yaml)? else {
            return Ok(RunForm {
                allowed: false,
                why: "this workflow has no workflow_dispatch trigger".into(),
                ..Default::default()
            });
        };
        params.insert(
            0,
            Param {
                name: REF.into(),
                kind: ParamKind::Text,
                default: self.default_branch.clone(),
                description: "Branch or tag to run on".into(),
                required: true,
            },
        );
        Ok(RunForm {
            allowed: true,
            why: String::new(),
            params,
            free_vars: false,
        })
    }

    async fn trigger(&mut self, job: &Job, values: &[(String, String)]) -> Result<String> {
        let mut reference = self.default_branch.clone();
        let mut inputs = Map::new();
        for (k, v) in values {
            if k == REF {
                reference = v.clone();
            } else {
                inputs.insert(k.clone(), Value::String(v.clone()));
            }
        }
        self.http
            .send(
                self.http
                    .request(
                        Method::POST,
                        &format!(
                            "/repos/{}/actions/workflows/{}/dispatches",
                            self.repo,
                            seg(&job.id)
                        ),
                    )
                    .json(&json!({"ref": reference, "inputs": inputs})),
            )
            .await?;
        Ok(format!("dispatched {} on {reference}", job.name))
    }

    async fn cancel(&mut self, job: &Job, run: &Run) -> Result<String> {
        self.http
            .send(self.http.request(
                Method::POST,
                &format!("/repos/{}/actions/runs/{}/cancel", self.repo, seg(&run.id)),
            ))
            .await?;
        Ok(format!("cancelling {} {}", job.name, run.number))
    }

    async fn log(&mut self, _job: &Job, run: &Run, from: u64) -> Result<LogChunk> {
        let jobs = self
            .http
            .get_json(&format!(
                "/repos/{}/actions/runs/{}/jobs?per_page=100",
                self.repo,
                seg(&run.id)
            ))
            .await?;
        let mut text = String::new();
        for j in jobs
            .get("jobs")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            text.push_str(&format!("===== {} =====\n", s(j, "name")));
            // A job that has not started has no log yet.
            match self
                .http
                .get_text(&format!(
                    "/repos/{}/actions/jobs/{}/logs",
                    self.repo,
                    s(j, "id")
                ))
                .await
            {
                Ok(t) => text.push_str(&t),
                // Every job would say the same; say it once, and why.
                Err(e) if self.anonymous && (e.contains("(401)") || e.contains("(403)")) => {
                    return Err(
                        "GitHub serves workflow logs only to a token, even for a public repository; give this connection a token credential"
                            .into(),
                    );
                }
                Err(e) => text.push_str(&format!("({e})\n")),
            }
            if !text.ends_with('\n') {
                text.push('\n');
            }
        }
        Ok(slice_from(text, from, run.state.is_active()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dispatch_inputs_are_read_from_every_form_of_on() {
        assert_eq!(
            dispatch_inputs("on: workflow_dispatch\n").unwrap(),
            Some(vec![])
        );
        assert_eq!(
            dispatch_inputs("on: [push, workflow_dispatch]\n").unwrap(),
            Some(vec![])
        );
        assert_eq!(dispatch_inputs("on:\n  push:\n").unwrap(), None);
        let p = dispatch_inputs(
            "on:\n  workflow_dispatch:\n    inputs:\n      env:\n        type: choice\n        options: [staging, prod]\n        default: staging\n        required: true\n      dry:\n        type: boolean\n        default: false\n",
        )
        .unwrap()
        .unwrap();
        assert_eq!(p.len(), 2);
        assert_eq!(p[0].name, "env");
        assert_eq!(
            p[0].kind,
            ParamKind::Choice(vec!["staging".into(), "prod".into()])
        );
        assert!(p[0].required);
        assert_eq!(p[1].kind, ParamKind::Bool);
        assert_eq!(p[1].default, "false");
    }

    #[test]
    fn states_map_from_status_and_conclusion() {
        assert_eq!(gh_state("in_progress", ""), RunState::Running);
        assert_eq!(gh_state("completed", "success"), RunState::Success);
        assert_eq!(gh_state("completed", "cancelled"), RunState::Cancelled);
        assert_eq!(gh_state("queued", ""), RunState::Queued);
    }

    #[test]
    fn a_run_reads_its_number_branch_and_actor() {
        let r = run(&json!({
            "id": 99, "run_number": 12, "status": "completed", "conclusion": "failure",
            "head_branch": "main", "run_started_at": "2026-10-31T10:00:00Z",
            "updated_at": "2026-10-31T10:01:00Z", "actor": {"login": "ada"}
        }));
        assert_eq!((r.id.as_str(), r.number.as_str()), ("99", "#12"));
        assert_eq!(r.duration_ms, Some(60_000));
        assert_eq!(r.who, "ada");
    }
}
