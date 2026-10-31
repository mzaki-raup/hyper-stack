//! Driver tests against real servers, each skipped unless its variable
//! names one:
//!
//! - `HS_JENKINS=host:port:user:api-token` - writes (build, stop)
//! - `HS_FORGEJO=host:port:owner/repo:token` - writes (dispatch); the
//!   repository needs a workflow with `workflow_dispatch` and a runner
//! - `HS_GITHUB_REPO=owner/name` - a public repository, read only
//! - `HS_GITLAB_PROJECT=group/name` - a public project on gitlab.com, read
//!   only
//!
//! Writes go only to the local bench servers (owner, 2026-10-31).

use std::time::Duration;

use hs_core::service_auth::{Auth, TokenAs};
use tool_automation::driver::{self, Driver, Job, Run, RunState};
use tool_automation::profile::ConnectConfig;

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("runtime")
}

fn config(kind: &str, url: &str, auth: Auth, token_as: TokenAs) -> ConnectConfig {
    ConnectConfig {
        kind: kind.into(),
        url: url.into(),
        repo: String::new(),
        project: String::new(),
        auth,
        token_as,
        read_only: false,
        timeout: Duration::from_secs(20),
    }
}

/// `host:port:a:b` into its four parts.
fn four(spec: &str) -> (String, String, String) {
    let parts: Vec<&str> = spec.splitn(4, ':').collect();
    let [host, port, a, b] = parts[..] else {
        panic!("expected host:port:a:b");
    };
    (
        format!("http://{host}:{port}"),
        a.to_string(),
        b.to_string(),
    )
}

async fn find(d: &mut dyn Driver, name: &str) -> Job {
    d.jobs()
        .await
        .expect("jobs")
        .jobs
        .into_iter()
        .find(|j| j.name == name)
        .unwrap_or_else(|| panic!("no job {name}"))
}

/// Wait until the job's newest run satisfies `ok`, reading runs again.
async fn wait_run(d: &mut dyn Driver, job: &Job, ok: impl Fn(&Run) -> bool) -> Run {
    for _ in 0..60 {
        if let Some(r) = d.runs(job).await.expect("runs").into_iter().next()
            && ok(&r)
        {
            return r;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    panic!("{} never reached the expected state", job.name);
}

#[test]
fn jenkins_runs_a_parameterised_job_follows_its_log_and_stops_it() {
    let Ok(spec) = std::env::var("HS_JENKINS") else {
        eprintln!("HS_JENKINS not set; skipping");
        return;
    };
    let (url, user, token) = four(&spec);
    runtime().block_on(async {
        let mut d = driver::connect(&config(
            "jenkins",
            &url,
            Auth::Token { user, token },
            TokenAs::Basic,
        ))
        .await
        .expect("connect");
        assert_eq!(d.info().product, "Jenkins");
        assert!(!d.info().version.is_empty());
        let job = find(d.as_mut(), "deploy-staging").await;
        let form = d.form(&job).await.expect("form");
        let names: Vec<&str> = form.params.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["ENV", "FAIL", "STEPS"]);
        let before = d
            .runs(&job)
            .await
            .expect("runs")
            .first()
            .map(|r| r.id.clone());
        let said = d
            .trigger(
                &job,
                &[
                    ("ENV".into(), "staging".into()),
                    ("STEPS".into(), "30".into()),
                ],
            )
            .await
            .expect("trigger");
        assert!(said.contains("queued"), "{said}");
        let run = wait_run(d.as_mut(), &job, |r| {
            Some(&r.id) != before.as_ref() && r.state == RunState::Running
        })
        .await;
        // The log grows while it runs, from the offset already read.
        let first = d.log(&job, &run, 0).await.expect("log");
        assert!(first.more, "a running build has more to come");
        assert!(first.text.contains("Deploying to staging"));
        tokio::time::sleep(Duration::from_secs(3)).await;
        let next = d.log(&job, &run, first.next).await.expect("log again");
        assert!(next.text.contains("step"), "{:?}", next.text);
        assert!(
            !next.text.contains("Deploying to staging"),
            "read from the offset"
        );
        d.cancel(&job, &run).await.expect("stop");
        let stopped = wait_run(d.as_mut(), &job, |r| r.id == run.id && !r.state.is_active()).await;
        assert_eq!(stopped.state, RunState::Cancelled);
        // A refused credential is named.
        let bad = driver::connect(&config(
            "jenkins",
            &url,
            Auth::Token {
                user: "dev".into(),
                token: "wrong".into(),
            },
            TokenAs::Basic,
        ))
        .await
        .err()
        .expect("refused");
        assert!(bad.contains("401"), "{bad}");
    });
}

#[test]
fn forgejo_dispatches_a_workflow_and_lists_its_runs() {
    let Ok(spec) = std::env::var("HS_FORGEJO") else {
        eprintln!("HS_FORGEJO not set; skipping");
        return;
    };
    let (url, repo, token) = four(&spec);
    runtime().block_on(async {
        let mut cfg = config(
            "forgejo-actions",
            &url,
            Auth::Token {
                user: String::new(),
                token,
            },
            TokenAs::TokenPrefix,
        );
        cfg.repo = repo;
        let mut d = driver::connect(&cfg).await.expect("connect");
        assert!(d.info().version.starts_with("11") || !d.info().version.is_empty());
        let job = find(d.as_mut(), "build.yml").await;
        let form = d.form(&job).await.expect("form");
        assert!(form.allowed, "{}", form.why);
        let names: Vec<&str> = form.params.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["ref", "target", "verbose"]);
        let before = d.runs(&job).await.expect("runs").len();
        let said = d
            .trigger(&job, &[("target".into(), "windows".into())])
            .await
            .expect("dispatch");
        assert!(said.contains("dispatched"), "{said}");
        // The runner takes it, so it appears among the tasks.
        for _ in 0..60 {
            if d.runs(&job).await.expect("runs").len() > before {
                return;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        panic!("the dispatched run never appeared");
    });
}

#[test]
fn github_actions_lists_a_public_repositorys_workflows() {
    let Ok(repo) = std::env::var("HS_GITHUB_REPO") else {
        eprintln!("HS_GITHUB_REPO not set; skipping");
        return;
    };
    runtime().block_on(async {
        let mut cfg = config(
            "github-actions",
            "https://api.github.com",
            Auth::None,
            TokenAs::Bearer,
        );
        cfg.repo = repo;
        let mut d = driver::connect(&cfg).await.expect("connect");
        let jobs = d.jobs().await.expect("workflows").jobs;
        assert!(!jobs.is_empty(), "a repository with workflows");
        let runs = d.runs(&jobs[0]).await.expect("runs");
        assert!(runs.iter().all(|r| r.number.starts_with('#')));
        // Reading the workflow file decides whether Run is offered.
        let form = d.form(&jobs[0]).await.expect("form");
        assert!(form.allowed || !form.why.is_empty());
    });
}

#[test]
fn gitlab_ci_lists_a_public_projects_pipelines() {
    let Ok(project) = std::env::var("HS_GITLAB_PROJECT") else {
        eprintln!("HS_GITLAB_PROJECT not set; skipping");
        return;
    };
    runtime().block_on(async {
        let mut cfg = config(
            "gitlab-ci",
            "https://gitlab.com",
            Auth::None,
            TokenAs::Bearer,
        );
        cfg.project = project;
        let mut d = driver::connect(&cfg).await.expect("connect");
        let jobs = d.jobs().await.expect("refs").jobs;
        assert!(!jobs.is_empty(), "a project with pipelines");
        let runs = d.runs(&jobs[0]).await.expect("pipelines");
        assert!(!runs.is_empty());
        assert!(runs.iter().all(|r| r.reference == jobs[0].id));
    });
}
