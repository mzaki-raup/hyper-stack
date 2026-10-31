//! AWX / Ansible Automation Platform through `/api/v2`
//! (docs/tools/automation.md §4). A job is a job template; its runs are
//! jobs.

use async_trait::async_trait;
use reqwest::Method;
use serde_json::{Map, Value, json};

use crate::driver::{
    Caps, Driver, Job, Listing, LogChunk, MAX_JOBS, MAX_RUNS, Param, ParamKind, Result, Run,
    RunForm, RunState, ServerInfo, iso_ms, slice_from,
};
use crate::http::{Http, s};
use crate::profile::ConnectConfig;

pub struct Awx {
    http: Http,
    web: String,
    info: ServerInfo,
}

/// The extra-vars field a template that asks for variables gets.
pub const EXTRA_VARS: &str = "extra_vars";

fn state(status: &str) -> RunState {
    match status {
        "new" | "pending" | "waiting" => RunState::Queued,
        "running" => RunState::Running,
        "successful" => RunState::Success,
        "failed" | "error" => RunState::Failure,
        "canceled" => RunState::Cancelled,
        _ => RunState::Unknown,
    }
}

fn run(j: &Value, web: &str) -> Run {
    let id = s(j, "id");
    Run {
        number: format!("#{id}"),
        state: state(&s(j, "status")),
        reference: s(j, "scm_branch"),
        started_ms: iso_ms(&s(j, "started")),
        duration_ms: j
            .get("elapsed")
            .and_then(Value::as_f64)
            .filter(|e| *e > 0.0)
            .map(|e| (e * 1000.0) as i64),
        who: j
            .get("summary_fields")
            .and_then(|f| f.get("created_by"))
            .map(|u| s(u, "username"))
            .unwrap_or_default(),
        url: format!("{web}/#/jobs/playbook/{id}/output"),
        id,
    }
}

/// A survey question as a form field.
fn survey_param(q: &Value) -> Param {
    let choices = || -> Vec<String> {
        match q.get("choices") {
            Some(Value::Array(a)) => a
                .iter()
                .filter_map(|c| c.as_str().map(String::from))
                .collect(),
            // Older AWX keeps choices as one newline-separated string.
            Some(Value::String(t)) => t.lines().map(String::from).collect(),
            _ => Vec::new(),
        }
    };
    Param {
        name: s(q, "variable"),
        kind: match s(q, "type").as_str() {
            "multiplechoice" => ParamKind::Choice(choices()),
            "password" => ParamKind::Secret,
            "textarea" => ParamKind::LongText,
            _ => ParamKind::Text,
        },
        default: s(q, "default"),
        description: s(q, "question_name"),
        required: q.get("required").and_then(Value::as_bool) == Some(true),
    }
}

/// Form values into the `extra_vars` object a launch sends: survey answers
/// as they are, and the free text field parsed as YAML or JSON.
pub fn extra_vars(values: &[(String, String)]) -> std::result::Result<Map<String, Value>, String> {
    let mut out = Map::new();
    for (k, v) in values {
        if k == EXTRA_VARS {
            if v.trim().is_empty() {
                continue;
            }
            let parsed: Value =
                serde_norway::from_str(v).map_err(|e| format!("extra vars: {e}"))?;
            match parsed {
                Value::Object(m) => out.extend(m),
                _ => return Err("extra vars must be a mapping of names to values".into()),
            }
        } else {
            out.insert(k.clone(), Value::String(v.clone()));
        }
    }
    Ok(out)
}

impl Awx {
    pub async fn connect(cfg: &ConnectConfig) -> Result<Awx> {
        let http = Http::new(cfg, &format!("{}/api/v2", cfg.url))?;
        let ping = http.get_json("/ping/").await?;
        // Ping answers anyone; this says whether the credential works.
        http.get_json("/me/").await?;
        Ok(Awx {
            web: cfg.url.clone(),
            info: ServerInfo {
                product: "AWX".into(),
                version: s(&ping, "version"),
            },
            http,
        })
    }
}

#[async_trait]
impl Driver for Awx {
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
                "/job_templates/?page_size={MAX_JOBS}&order_by=name"
            ))
            .await?;
        let jobs = v
            .get("results")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|t| {
                let last = t
                    .get("summary_fields")
                    .and_then(|f| f.get("last_job"))
                    .map(|j| Run {
                        id: s(j, "id"),
                        number: format!("#{}", s(j, "id")),
                        state: state(&s(j, "status")),
                        started_ms: iso_ms(&s(j, "finished")),
                        ..Default::default()
                    });
                Job {
                    id: s(t, "id"),
                    name: s(t, "name"),
                    last,
                    url: format!(
                        "{}/#/templates/job_template/{}/details",
                        self.web,
                        s(t, "id")
                    ),
                }
            })
            .collect();
        let more = v.get("next").is_some_and(|n| !n.is_null());
        Ok(Listing {
            jobs,
            note: more.then(|| format!("the first {MAX_JOBS} job templates")),
        })
    }

    async fn runs(&mut self, job: &Job) -> Result<Vec<Run>> {
        let v = self
            .http
            .get_json(&format!(
                "/job_templates/{}/jobs/?order_by=-id&page_size={MAX_RUNS}",
                job.id
            ))
            .await?;
        Ok(v.get("results")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|j| run(j, &self.web))
            .collect())
    }

    async fn form(&mut self, job: &Job) -> Result<RunForm> {
        let t = self
            .http
            .get_json(&format!("/job_templates/{}/", job.id))
            .await?;
        let mut params = Vec::new();
        if t.get("survey_enabled").and_then(Value::as_bool) == Some(true) {
            let survey = self
                .http
                .get_json(&format!("/job_templates/{}/survey_spec/", job.id))
                .await?;
            params.extend(
                survey
                    .get("spec")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .map(survey_param),
            );
        }
        if t.get("ask_variables_on_launch").and_then(Value::as_bool) == Some(true) {
            params.push(Param {
                name: EXTRA_VARS.into(),
                kind: ParamKind::LongText,
                default: s(&t, "extra_vars"),
                description: "Extra variables, YAML or JSON".into(),
                required: false,
            });
        }
        Ok(RunForm {
            allowed: true,
            why: String::new(),
            params,
            free_vars: false,
        })
    }

    async fn trigger(&mut self, job: &Job, values: &[(String, String)]) -> Result<String> {
        let vars = extra_vars(values)?;
        let body = if vars.is_empty() {
            json!({})
        } else {
            json!({"extra_vars": vars})
        };
        let v = self
            .http
            .json(
                self.http
                    .request(Method::POST, &format!("/job_templates/{}/launch/", job.id))
                    .json(&body),
            )
            .await?;
        Ok(format!("launched {} as job {}", job.name, s(&v, "job")))
    }

    async fn cancel(&mut self, job: &Job, run: &Run) -> Result<String> {
        self.http
            .send(
                self.http
                    .request(Method::POST, &format!("/jobs/{}/cancel/", run.id)),
            )
            .await?;
        Ok(format!("cancelling {} {}", job.name, run.number))
    }

    async fn log(&mut self, _job: &Job, run: &Run, from: u64) -> Result<LogChunk> {
        let text = self
            .http
            .get_text(&format!("/jobs/{}/stdout/?format=txt", run.id))
            .await?;
        Ok(slice_from(text, from, run.state.is_active()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn survey_questions_become_form_fields() {
        let p = survey_param(&json!({
            "variable": "env", "type": "multiplechoice", "choices": "staging\nprod",
            "default": "staging", "question_name": "Where?", "required": true
        }));
        assert_eq!(p.name, "env");
        assert_eq!(
            p.kind,
            ParamKind::Choice(vec!["staging".into(), "prod".into()])
        );
        assert!(p.required);
    }

    #[test]
    fn extra_vars_merge_survey_answers_and_yaml() {
        let v = extra_vars(&[
            ("env".into(), "prod".into()),
            (EXTRA_VARS.into(), "replicas: 3\nfeature: on\n".into()),
        ])
        .unwrap();
        assert_eq!(v["env"], "prod");
        assert_eq!(v["replicas"], 3);
        assert!(extra_vars(&[(EXTRA_VARS.into(), "- a\n- b".into())]).is_err());
    }

    #[test]
    fn a_job_reads_its_state_and_elapsed_time() {
        let r = run(
            &json!({"id": 9, "status": "failed", "elapsed": 12.5, "started": "2026-10-31T10:00:00Z",
                    "summary_fields": {"created_by": {"username": "ada"}}}),
            "https://awx",
        );
        assert_eq!(r.state, RunState::Failure);
        assert_eq!(r.duration_ms, Some(12_500));
        assert_eq!(r.who, "ada");
    }
}
