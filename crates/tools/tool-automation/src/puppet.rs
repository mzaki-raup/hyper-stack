//! Puppet through PuppetDB's query API (`/pdb/query/v4`) and, when the
//! profile names one, a Puppet Enterprise orchestrator
//! (docs/tools/automation.md §4, FEAT-0092). A job is a node; its runs are
//! the node's reports; a report's log lines are the log. **Run** deploys
//! the node through the orchestrator and is offered only when the profile
//! carries an orchestrator URL - open-source Puppet has no way to start a
//! run from outside the agent.

use async_trait::async_trait;
use reqwest::Method;
use serde_json::{Value, json};

use crate::driver::{
    Caps, Driver, Job, Listing, LogChunk, MAX_JOBS, MAX_RUNS, Param, ParamKind, Result, Run,
    RunForm, RunState, ServerInfo, iso_ms, slice_from, time_ms,
};
use crate::profile::ConnectConfig;
use hs_core::service_http::{Http, s};

pub struct Puppet {
    /// PuppetDB, rooted at the profile's URL.
    db: Http,
    /// The orchestrator, rooted at the profile's project field, when set.
    orchestrator: Option<(Http, String)>,
    info: ServerInfo,
}

/// A report's `status` (and the node's `latest_report_status`) in the words
/// every kind shares. A noop run changed nothing by design: Skipped.
pub fn state(status: &str, noop: bool) -> RunState {
    match (status, noop) {
        ("failed", _) => RunState::Failure,
        (_, true) => RunState::Skipped,
        ("changed" | "unchanged", false) => RunState::Success,
        _ => RunState::Unknown,
    }
}

/// One PuppetDB report as a run: the hash is the id, the start time what
/// people see, the environment the reference.
pub fn report(r: &Value) -> Run {
    let started_ms = iso_ms(&s(r, "start_time"));
    let ended_ms = iso_ms(&s(r, "end_time"));
    let noop = r.get("noop").and_then(Value::as_bool) == Some(true);
    Run {
        id: s(r, "hash"),
        number: started_ms.map(time_ms).unwrap_or_else(|| s(r, "hash")),
        state: state(&s(r, "status"), noop),
        reference: s(r, "environment"),
        started_ms,
        duration_ms: match (started_ms, ended_ms) {
            (Some(a), Some(b)) if b >= a => Some(b - a),
            _ => None,
        },
        who: s(r, "producer"),
        url: String::new(),
    }
}

/// A node row as a job, its latest report as the last run.
pub fn node(n: &Value) -> Job {
    let certname = s(n, "certname");
    let hash = s(n, "latest_report_hash");
    let last = (!hash.is_empty()).then(|| {
        let noop = n.get("latest_report_noop").and_then(Value::as_bool) == Some(true);
        Run {
            id: hash,
            number: iso_ms(&s(n, "report_timestamp"))
                .map(time_ms)
                .unwrap_or_default(),
            state: state(&s(n, "latest_report_status"), noop),
            reference: s(n, "report_environment"),
            started_ms: iso_ms(&s(n, "report_timestamp")),
            ..Default::default()
        }
    });
    Job {
        id: certname.clone(),
        name: certname,
        last,
        url: String::new(),
    }
}

/// A report's log entries as text, one line each: time, level, source and
/// message, as `puppet agent -t` prints them.
pub fn log_text(lines: &[Value]) -> String {
    let mut out = String::new();
    for l in lines {
        let time = s(l, "time");
        let time = time.get(11..19).unwrap_or(&time);
        let source = s(l, "source");
        out.push_str(&format!(
            "{time} {:<7} {}{}\n",
            s(l, "level").to_uppercase(),
            if source.is_empty() || source == "Puppet" {
                String::new()
            } else {
                format!("{source}: ")
            },
            s(l, "message")
        ));
    }
    out
}

/// `?k=v&k=v` with every byte outside the unreserved set percent-encoded:
/// PuppetDB takes its queries as JSON in the URL.
pub fn query(pairs: &[(&str, &str)]) -> String {
    fn enc(s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        for b in s.bytes() {
            match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    out.push(b as char)
                }
                _ => out.push_str(&format!("%{b:02X}")),
            }
        }
        out
    }
    let mut out = String::new();
    for (i, (k, v)) in pairs.iter().enumerate() {
        out.push(if i == 0 { '?' } else { '&' });
        out.push_str(&enc(k));
        out.push('=');
        out.push_str(&enc(v));
    }
    out
}

impl Puppet {
    pub async fn connect(cfg: &ConnectConfig) -> Result<Puppet> {
        let db = cfg.http(cfg.url.trim_end_matches('/'))?;
        let version = db.get_json("/pdb/meta/v1/version").await?;
        let orch = cfg.project.trim().trim_end_matches('/').to_string();
        let orchestrator = if orch.is_empty() {
            None
        } else {
            Some((cfg.http(&orch)?, orch))
        };
        Ok(Puppet {
            db,
            orchestrator,
            info: ServerInfo {
                product: "Puppet".into(),
                version: s(&version, "version"),
            },
        })
    }
}

#[async_trait]
impl Driver for Puppet {
    fn info(&self) -> &ServerInfo {
        &self.info
    }

    fn caps(&self) -> Caps {
        Caps {
            run: self.orchestrator.is_some(),
            // An orchestrator job is not a report: the runs listed here
            // are PuppetDB's, which are over by the time they are stored.
            cancel: false,
            queue: false,
            log: true,
        }
    }

    async fn jobs(&mut self) -> Result<Listing> {
        let v = self
            .db
            .get_json(&format!(
                "/pdb/query/v4/nodes{}",
                query(&[
                    ("order_by", r#"[{"field":"certname","order":"asc"}]"#),
                    ("limit", &(MAX_JOBS + 1).to_string()),
                ])
            ))
            .await?;
        let mut jobs: Vec<Job> = v.as_array().into_iter().flatten().map(node).collect();
        let more = jobs.len() > MAX_JOBS;
        jobs.truncate(MAX_JOBS);
        Ok(Listing {
            jobs,
            note: more.then(|| format!("the first {MAX_JOBS} nodes")),
        })
    }

    async fn runs(&mut self, job: &Job) -> Result<Vec<Run>> {
        let by_node = json!(["=", "certname", job.id]).to_string();
        let v = self
            .db
            .get_json(&format!(
                "/pdb/query/v4/reports{}",
                query(&[
                    ("query", by_node.as_str()),
                    ("order_by", r#"[{"field":"start_time","order":"desc"}]"#),
                    ("limit", &MAX_RUNS.to_string()),
                ])
            ))
            .await?;
        Ok(v.as_array().into_iter().flatten().map(report).collect())
    }

    async fn form(&mut self, _job: &Job) -> Result<RunForm> {
        if self.orchestrator.is_none() {
            return Ok(RunForm {
                allowed: false,
                why: "Runs need a Puppet Enterprise orchestrator URL in the profile; open-source Puppet runs on the agent's schedule".into(),
                params: Vec::new(),
                free_vars: false,
            });
        }
        Ok(RunForm {
            allowed: true,
            why: String::new(),
            params: vec![
                Param {
                    name: "environment".into(),
                    kind: ParamKind::Text,
                    default: "production".into(),
                    description: "The environment to deploy".into(),
                    required: true,
                },
                Param {
                    name: "noop".into(),
                    kind: ParamKind::Bool,
                    default: "false".into(),
                    description: "Simulate: report what would change without changing it".into(),
                    required: false,
                },
            ],
            free_vars: false,
        })
    }

    async fn trigger(&mut self, job: &Job, values: &[(String, String)]) -> Result<String> {
        let Some((http, _)) = &self.orchestrator else {
            return Err("this profile has no orchestrator URL".into());
        };
        let get = |k: &str| {
            values
                .iter()
                .find(|(n, _)| n == k)
                .map(|(_, v)| v.trim().to_string())
                .unwrap_or_default()
        };
        let environment = {
            let e = get("environment");
            if e.is_empty() {
                "production".to_string()
            } else {
                e
            }
        };
        let noop = get("noop") == "true";
        let body = json!({
            "environment": environment,
            "noop": noop,
            "scope": { "nodes": [job.id] },
        });
        let v = http
            .json(
                http.request(Method::POST, "/orchestrator/v1/command/deploy")
                    .json(&body),
            )
            .await?;
        let id = v
            .get("job")
            .map(|j| s(j, "name"))
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| s(&v, "job"));
        Ok(format!("deploying {} as orchestrator job {id}", job.name))
    }

    async fn cancel(&mut self, _job: &Job, _run: &Run) -> Result<String> {
        Err("a Puppet report cannot be cancelled: it is already over".into())
    }

    async fn log(&mut self, _job: &Job, run: &Run, from: u64) -> Result<LogChunk> {
        let v = self
            .db
            .get_json(&format!("/pdb/query/v4/reports/{}/logs", run.id))
            .await?;
        let text = log_text(v.as_array().map(Vec::as_slice).unwrap_or(&[]));
        Ok(slice_from(text, from, false))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_report_reads_its_state_environment_and_duration() {
        let r = report(&json!({
            "hash": "abc", "status": "changed", "noop": false, "environment": "production",
            "start_time": "2026-11-06T10:00:00.000Z", "end_time": "2026-11-06T10:00:42.000Z",
            "producer": "puppet.example.com"
        }));
        assert_eq!(r.id, "abc");
        assert_eq!(r.state, RunState::Success);
        assert_eq!(r.reference, "production");
        assert_eq!(r.duration_ms, Some(42_000));
        assert_eq!(state("failed", true), RunState::Failure);
        assert_eq!(state("unchanged", true), RunState::Skipped);
        assert_eq!(state("pending", false), RunState::Unknown);
    }

    #[test]
    fn a_node_carries_its_latest_report_and_a_fresh_one_has_none() {
        let n = node(&json!({
            "certname": "web1.example.com", "latest_report_hash": "h1",
            "latest_report_status": "failed", "latest_report_noop": false,
            "report_timestamp": "2026-11-06T09:30:00Z", "report_environment": "staging"
        }));
        assert_eq!(n.name, "web1.example.com");
        let last = n.last.expect("a last run");
        assert_eq!(last.state, RunState::Failure);
        assert_eq!(last.reference, "staging");
        assert!(node(&json!({"certname": "new.example.com"})).last.is_none());
    }

    #[test]
    fn a_query_string_carries_json_percent_encoded() {
        let q = query(&[("query", r#"["=","certname","a b"]"#), ("limit", "50")]);
        assert_eq!(
            q,
            "?query=%5B%22%3D%22%2C%22certname%22%2C%22a%20b%22%5D&limit=50"
        );
    }

    #[test]
    fn log_lines_read_like_the_agents_output() {
        let text = log_text(&[
            json!({"time": "2026-11-06T10:00:01.000Z", "level": "notice", "source": "Puppet", "message": "Applied catalog in 1.2 seconds"}),
            json!({"time": "2026-11-06T10:00:00.500Z", "level": "err", "source": "/Stage[main]/Nginx/Service[nginx]", "message": "Could not start"}),
        ]);
        assert_eq!(
            text,
            "10:00:01 NOTICE  Applied catalog in 1.2 seconds\n10:00:00 ERR     /Stage[main]/Nginx/Service[nginx]: Could not start\n"
        );
    }
}
