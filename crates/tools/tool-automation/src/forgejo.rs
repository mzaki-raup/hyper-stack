//! Forgejo (and Gitea) Actions through the Forgejo API
//! (docs/tools/automation.md §4). A connection is one repository; a job is
//! a workflow file.
//!
//! The API (Forgejo 11) lists a repository's action tasks - a run's jobs
//! once a runner has taken them - and the jobs still waiting for a runner,
//! and dispatches a workflow. It has no call to cancel a run or read its
//! log, so those are left to the server's web page (Open in Browser).

use std::collections::BTreeMap;

use async_trait::async_trait;
use base64::Engine;
use reqwest::Method;
use serde_json::{Map, Value, json};

use crate::driver::{
    Caps, Driver, Job, Listing, LogChunk, MAX_RUNS, Param, ParamKind, QueueItem, Result, Run,
    RunForm, ServerInfo, iso_ms,
};
use crate::github::{REF, dispatch_inputs, gh_state};
use crate::http::{Http, s, seg};
use crate::profile::ConnectConfig;

/// Where a repository keeps its workflows, in the order Forgejo reads them.
const WORKFLOW_DIRS: &[&str] = &[
    ".forgejo/workflows",
    ".gitea/workflows",
    ".github/workflows",
];

pub struct Forgejo {
    http: Http,
    repo: String,
    web: String,
    default_branch: String,
    info: ServerInfo,
}

fn run(t: &Value, web: &str, repo: &str) -> Run {
    let started = iso_ms(&s(t, "run_started_at")).or_else(|| iso_ms(&s(t, "created_at")));
    let state = gh_state(&s(t, "status"), &s(t, "status"));
    let ended = iso_ms(&s(t, "updated_at"));
    let number = s(t, "run_number");
    Run {
        id: s(t, "id"),
        number: format!("#{number}"),
        reference: s(t, "head_branch"),
        started_ms: started,
        duration_ms: match (started, ended) {
            (Some(a), Some(b)) if !state.is_active() => Some(b - a),
            _ => None,
        },
        who: String::new(),
        url: format!("{web}/{repo}/actions/runs/{number}"),
        state,
    }
}

impl Forgejo {
    pub async fn connect(cfg: &ConnectConfig) -> Result<Forgejo> {
        let http = Http::new(cfg, &format!("{}/api/v1", cfg.url))?;
        let v = http.get_json("/version").await?;
        let r = http.get_json(&format!("/repos/{}", cfg.repo)).await?;
        Ok(Forgejo {
            default_branch: s(&r, "default_branch"),
            repo: cfg.repo.clone(),
            web: cfg.url.clone(),
            info: ServerInfo {
                product: "Forgejo Actions".into(),
                version: s(&v, "version"),
            },
            http,
        })
    }

    /// The repository's action tasks, newest first.
    async fn tasks(&self) -> Result<Vec<Value>> {
        let v = self
            .http
            .get_json(&format!("/repos/{}/actions/tasks?limit=50", self.repo))
            .await?;
        Ok(v.get("workflow_runs")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default())
    }

    /// Workflow files: name → path.
    async fn workflows(&self) -> BTreeMap<String, String> {
        let mut out = BTreeMap::new();
        for dir in WORKFLOW_DIRS {
            let Ok(v) = self
                .http
                .get_json(&format!("/repos/{}/contents/{dir}", self.repo))
                .await
            else {
                continue;
            };
            for f in v.as_array().into_iter().flatten() {
                let name = s(f, "name");
                if name.ends_with(".yml") || name.ends_with(".yaml") {
                    out.entry(name).or_insert_with(|| s(f, "path"));
                }
            }
        }
        out
    }
}

#[async_trait]
impl Driver for Forgejo {
    fn info(&self) -> &ServerInfo {
        &self.info
    }

    fn caps(&self) -> Caps {
        Caps {
            run: true,
            cancel: false,
            queue: true,
            log: false,
        }
    }

    async fn jobs(&mut self) -> Result<Listing> {
        let tasks = self.tasks().await?;
        let mut names: Vec<String> = self.workflows().await.into_keys().collect();
        // A workflow that ran but has since been removed still has runs.
        for t in &tasks {
            let w = s(t, "workflow_id");
            if !w.is_empty() && !names.contains(&w) {
                names.push(w);
            }
        }
        names.sort();
        let jobs = names
            .into_iter()
            .map(|name| Job {
                last: tasks
                    .iter()
                    .find(|t| s(t, "workflow_id") == name)
                    .map(|t| run(t, &self.web, &self.repo)),
                url: format!("{}/{}/actions?workflow={}", self.web, self.repo, seg(&name)),
                id: name.clone(),
                name,
            })
            .collect();
        Ok(Listing { jobs, note: None })
    }

    async fn runs(&mut self, job: &Job) -> Result<Vec<Run>> {
        Ok(self
            .tasks()
            .await?
            .iter()
            .filter(|t| s(t, "workflow_id") == job.id)
            .take(MAX_RUNS)
            .map(|t| run(t, &self.web, &self.repo))
            .collect())
    }

    async fn form(&mut self, job: &Job) -> Result<RunForm> {
        let Some(path) = self.workflows().await.get(&job.id).cloned() else {
            return Ok(RunForm {
                allowed: false,
                why: "the workflow file is no longer in the repository".into(),
                ..Default::default()
            });
        };
        let file = self
            .http
            .get_json(&format!("/repos/{}/contents/{path}", self.repo))
            .await?;
        let raw = s(&file, "content").replace('\n', "");
        let yaml = base64::engine::general_purpose::STANDARD
            .decode(raw)
            .map_err(|e| e.to_string())?;
        let Some(mut params) = dispatch_inputs(&String::from_utf8_lossy(&yaml))? else {
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
        let v = self
            .http
            .json(
                self.http
                    .request(
                        Method::POST,
                        &format!(
                            "/repos/{}/actions/workflows/{}/dispatches",
                            self.repo,
                            seg(&job.id)
                        ),
                    )
                    .json(&json!({"ref": reference, "inputs": inputs, "return_run_info": true})),
            )
            .await?;
        let number = s(&v, "run_number");
        Ok(if number.is_empty() {
            format!("dispatched {} on {reference}", job.name)
        } else {
            format!("dispatched {} on {reference} as run #{number}", job.name)
        })
    }

    async fn cancel(&mut self, _job: &Job, _run: &Run) -> Result<String> {
        Err("Forgejo's API has no call to cancel a run; use its web page".into())
    }

    async fn log(&mut self, _job: &Job, _run: &Run, _from: u64) -> Result<LogChunk> {
        Err("Forgejo's API has no call to read a run's log; use its web page".into())
    }

    /// The jobs waiting for a runner.
    async fn queue(&mut self) -> Result<Vec<QueueItem>> {
        let v = self
            .http
            .get_json(&format!("/repos/{}/actions/runners/jobs", self.repo))
            .await?;
        Ok(v.as_array()
            .into_iter()
            .flatten()
            .map(|j| QueueItem {
                id: s(j, "id"),
                job: s(j, "name"),
                why: format!(
                    "{} (runs on {})",
                    s(j, "status"),
                    j.get("runs_on")
                        .and_then(Value::as_array)
                        .map(|a| a
                            .iter()
                            .filter_map(Value::as_str)
                            .collect::<Vec<_>>()
                            .join(", "))
                        .unwrap_or_default()
                ),
                since_ms: None,
            })
            .collect())
    }
}
