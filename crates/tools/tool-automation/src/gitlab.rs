//! GitLab CI through REST v4 (docs/tools/automation.md §4). A connection
//! is one project; the "jobs" in the table are the refs that have
//! pipelines, so it reads like the other kinds, and a ref's runs are its
//! pipelines.

use std::collections::HashSet;

use async_trait::async_trait;
use reqwest::Method;
use serde_json::{Value, json};

use crate::driver::{
    Caps, Driver, Job, Listing, LogChunk, MAX_JOBS, MAX_RUNS, Param, ParamKind, Result, Run,
    RunForm, RunState, ServerInfo, iso_ms, slice_from,
};
use crate::http::{Http, s, seg};
use crate::profile::ConnectConfig;

pub struct GitLab {
    http: Http,
    project: String,
    info: ServerInfo,
}

fn state(status: &str) -> RunState {
    match status {
        "created" | "waiting_for_resource" | "preparing" | "pending" | "scheduled" | "manual" => {
            RunState::Queued
        }
        "running" => RunState::Running,
        "success" => RunState::Success,
        "failed" => RunState::Failure,
        "canceled" | "canceling" => RunState::Cancelled,
        "skipped" => RunState::Skipped,
        _ => RunState::Unknown,
    }
}

fn run(p: &Value) -> Run {
    let started = iso_ms(&s(p, "started_at")).or_else(|| iso_ms(&s(p, "created_at")));
    let state = state(&s(p, "status"));
    let ended = iso_ms(&s(p, "finished_at")).or_else(|| iso_ms(&s(p, "updated_at")));
    Run {
        id: s(p, "id"),
        number: format!("#{}", s(p, "iid")),
        reference: s(p, "ref"),
        started_ms: started,
        duration_ms: match (started, ended) {
            (Some(a), Some(b)) if !state.is_active() => Some(b - a),
            _ => None,
        },
        who: p.get("user").map(|u| s(u, "username")).unwrap_or_default(),
        url: s(p, "web_url"),
        state,
    }
}

/// The name of the form field that carries extra pipeline variables,
/// `KEY=value` per line.
pub const VARIABLES: &str = "variables";

/// `KEY=value` lines into pairs; blank and malformed lines dropped.
pub fn parse_vars(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        .filter(|(k, _)| !k.is_empty())
        .collect()
}

impl GitLab {
    pub async fn connect(cfg: &ConnectConfig) -> Result<GitLab> {
        let http = Http::new(cfg, &format!("{}/api/v4", cfg.url))?;
        let project = seg(&cfg.project);
        // The project first: it says whether the credential can see it.
        http.get_json(&format!("/projects/{project}")).await?;
        let version = match http.get_json("/version").await {
            Ok(v) => s(&v, "version"),
            // Anonymous callers may not read the version; that is no failure.
            Err(_) => String::new(),
        };
        Ok(GitLab {
            http,
            project,
            info: ServerInfo {
                product: "GitLab CI".into(),
                version,
            },
        })
    }
}

#[async_trait]
impl Driver for GitLab {
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
        let v = self
            .http
            .get_json(&format!(
                "/projects/{}/pipelines?per_page=100&order_by=id&sort=desc",
                self.project
            ))
            .await?;
        // Newest first, so the first pipeline seen for a ref is its last.
        let mut seen = HashSet::new();
        let mut jobs = Vec::new();
        for p in v.as_array().into_iter().flatten() {
            let reference = s(p, "ref");
            if jobs.len() >= MAX_JOBS || !seen.insert(reference.clone()) {
                continue;
            }
            jobs.push(Job {
                id: reference.clone(),
                name: reference,
                last: Some(run(p)),
                url: String::new(),
            });
        }
        jobs.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(Listing {
            jobs,
            note: Some("refs with a pipeline among the newest 100".into()),
        })
    }

    async fn runs(&mut self, job: &Job) -> Result<Vec<Run>> {
        let v = self
            .http
            .get_json(&format!(
                "/projects/{}/pipelines?ref={}&per_page={MAX_RUNS}",
                self.project,
                seg(&job.id)
            ))
            .await?;
        Ok(v.as_array().into_iter().flatten().map(run).collect())
    }

    async fn form(&mut self, job: &Job) -> Result<RunForm> {
        Ok(RunForm {
            allowed: true,
            why: String::new(),
            params: vec![Param {
                name: VARIABLES.into(),
                kind: ParamKind::LongText,
                default: String::new(),
                description: format!("Pipeline variables for {}, KEY=value per line", job.name),
                required: false,
            }],
            free_vars: true,
        })
    }

    async fn trigger(&mut self, job: &Job, values: &[(String, String)]) -> Result<String> {
        let mut variables: Vec<Value> = Vec::new();
        for (k, v) in values {
            if k == VARIABLES {
                for (vk, vv) in parse_vars(v) {
                    variables.push(json!({"key": vk, "value": vv}));
                }
            } else {
                variables.push(json!({"key": k, "value": v}));
            }
        }
        let p = self
            .http
            .json(
                self.http
                    .request(
                        Method::POST,
                        &format!("/projects/{}/pipeline", self.project),
                    )
                    .json(&json!({"ref": job.id, "variables": variables})),
            )
            .await?;
        Ok(format!("pipeline #{} on {}", s(&p, "iid"), job.name))
    }

    async fn cancel(&mut self, _job: &Job, run: &Run) -> Result<String> {
        self.http
            .send(self.http.request(
                Method::POST,
                &format!("/projects/{}/pipelines/{}/cancel", self.project, run.id),
            ))
            .await?;
        Ok(format!("cancelled pipeline {}", run.number))
    }

    async fn log(&mut self, _job: &Job, run: &Run, from: u64) -> Result<LogChunk> {
        let jobs = self
            .http
            .get_json(&format!(
                "/projects/{}/pipelines/{}/jobs?per_page=100",
                self.project, run.id
            ))
            .await?;
        let mut list: Vec<&Value> = jobs.as_array().into_iter().flatten().collect();
        // Oldest job first, so the log reads in the order it ran.
        list.sort_by_key(|j| j.get("id").and_then(Value::as_i64).unwrap_or(0));
        let mut text = String::new();
        for j in list {
            text.push_str(&format!(
                "===== {} / {} =====\n",
                s(j, "stage"),
                s(j, "name")
            ));
            match self
                .http
                .get_text(&format!(
                    "/projects/{}/jobs/{}/trace",
                    self.project,
                    s(j, "id")
                ))
                .await
            {
                Ok(t) => text.push_str(&t),
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
    fn variables_are_read_one_per_line() {
        assert_eq!(
            parse_vars("A=1\n\nbad line\n B = two \n=x"),
            vec![("A".into(), "1".into()), ("B".into(), "two".into())]
        );
    }

    #[test]
    fn a_pipeline_reads_its_state_and_user() {
        let r = run(&json!({
            "id": 5, "iid": 3, "status": "failed", "ref": "main",
            "created_at": "2026-10-31T10:00:00Z", "updated_at": "2026-10-31T10:00:30Z",
            "user": {"username": "ada"}, "web_url": "w"
        }));
        assert_eq!(r.number, "#3");
        assert_eq!(r.state, RunState::Failure);
        assert_eq!(r.duration_ms, Some(30_000));
        assert_eq!(state("manual"), RunState::Queued);
    }
}
