//! Drivers against a mock HTTP server, for the servers that cannot run on
//! the bench (TeamCity's first start needs its setup wizard, AWX needs a
//! Kubernetes operator) and for the writes the owner keeps off GitHub and
//! GitLab (2026-10-31).
//!
//! The responses are written from each API's reference documentation,
//! trimmed to the fields the drivers read; they were not captured from a
//! live server. What these tests prove is that the drivers call the
//! documented endpoints with the documented bodies and read the documented
//! answers - not that a given server version still answers that way.

use std::sync::Arc;
use std::time::Duration;

use hs_core::service_auth::{Auth, TokenAs};
use parking_lot::Mutex;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tool_automation::driver::{self, Job, Run, RunState};
use tool_automation::profile::ConnectConfig;

/// One canned answer: method, path prefix (query included where it
/// matters), status, body.
type Route = (&'static str, &'static str, u16, String);

/// A request the server saw: method, path, Authorization header, body.
#[derive(Debug, Clone)]
struct Seen {
    method: String,
    path: String,
    auth: String,
    body: String,
}

async fn serve(routes: Vec<Route>) -> (String, Arc<Mutex<Vec<Seen>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                return;
            };
            let routes = routes.clone();
            let log = log.clone();
            tokio::spawn(async move {
                let mut buf = Vec::new();
                let mut chunk = [0u8; 4096];
                // Headers, then as much body as Content-Length says.
                let (head_end, length) = loop {
                    let n = sock.read(&mut chunk).await.unwrap_or(0);
                    if n == 0 {
                        return;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                    if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                        let head = String::from_utf8_lossy(&buf[..i]).to_lowercase();
                        let length = head
                            .lines()
                            .find_map(|l| l.strip_prefix("content-length:"))
                            .and_then(|v| v.trim().parse::<usize>().ok())
                            .unwrap_or(0);
                        break (i + 4, length);
                    }
                };
                while buf.len() < head_end + length {
                    let n = sock.read(&mut chunk).await.unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                }
                let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
                let body = String::from_utf8_lossy(&buf[head_end..]).to_string();
                let mut first = head.lines().next().unwrap_or("").split(' ');
                let method = first.next().unwrap_or("").to_string();
                let path = first.next().unwrap_or("").to_string();
                let auth = head
                    .lines()
                    .find_map(|l| {
                        let (k, v) = l.split_once(':')?;
                        k.eq_ignore_ascii_case("authorization")
                            .then(|| v.trim().to_string())
                    })
                    .unwrap_or_default();
                log.lock().push(Seen {
                    method: method.clone(),
                    path: path.clone(),
                    auth,
                    body,
                });
                let (status, reply) = routes
                    .iter()
                    .filter(|(m, p, _, _)| *m == method && path.starts_with(p))
                    // The longest prefix is the most specific route.
                    .max_by_key(|(_, p, _, _)| p.len())
                    .map(|(_, _, s, b)| (*s, b.clone()))
                    .unwrap_or((404, r#"{"message":"no such route"}"#.into()));
                let resp = format!(
                    "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{reply}",
                    reply.len()
                );
                let _ = sock.write_all(resp.as_bytes()).await;
            });
        }
    });
    (format!("http://{addr}"), seen)
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
        timeout: Duration::from_secs(5),
    }
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("runtime")
}

fn token(t: &str) -> Auth {
    Auth::Token {
        user: String::new(),
        token: t.into(),
    }
}

#[test]
fn teamcity_lists_triggers_and_cancels_through_the_build_queue() {
    runtime().block_on(async {
        let build = r#"{"id":71,"number":"12","state":"running","status":"SUCCESS","branchName":"main","startDate":"20261031T100000+0000","triggered":{"user":{"username":"ada"}},"webUrl":"w"}"#;
        let (url, seen) = serve(vec![
            ("GET", "/app/rest/server", 200, r#"{"version":"2025.07.3 (build 197398)"}"#.into()),
            (
                "GET",
                "/app/rest/buildTypes?",
                200,
                format!(r#"{{"buildType":[{{"id":"App_Build","name":"Build","projectName":"App","webUrl":"w","builds":{{"build":[{build}]}}}}]}}"#),
            ),
            ("GET", "/app/rest/builds?", 200, format!(r#"{{"build":[{build}]}}"#)),
            (
                "GET",
                "/app/rest/buildTypes/id:App_Build/parameters",
                200,
                r#"{"property":[{"name":"env.TARGET","value":"staging"},{"name":"secret","value":"","type":{"rawValue":"password display='hidden'"}}]}"#.into(),
            ),
            ("POST", "/app/rest/buildQueue", 200, r#"{"id":72,"state":"queued"}"#.into()),
            ("POST", "/app/rest/builds/id:71", 200, "{}".into()),
            ("GET", "/downloadBuildLog.html", 200, "line one\nline two\n".into()),
        ])
        .await;
        let mut d = driver::connect(&config("teamcity", &url, token("tc-t"), TokenAs::Bearer))
            .await
            .expect("connect");
        assert_eq!(d.info().version, "2025.07.3 (build 197398)");
        let jobs = d.jobs().await.expect("jobs").jobs;
        assert_eq!(jobs[0].name, "App / Build");
        assert_eq!(jobs[0].last.as_ref().unwrap().state, RunState::Running);
        let job = jobs[0].clone();
        let form = d.form(&job).await.expect("form");
        assert_eq!(form.params.len(), 2);
        let said = d
            .trigger(&job, &[("env.TARGET".into(), "qa".into())])
            .await
            .expect("trigger");
        assert!(said.contains("72"), "{said}");
        let run = d.runs(&job).await.expect("runs")[0].clone();
        d.cancel(&job, &run).await.expect("cancel");
        let log = d.log(&job, &run, 9).await.expect("log");
        assert_eq!(log.text, "line two\n");
        assert!(log.more);
        let seen = seen.lock().clone();
        let trigger = seen.iter().find(|s| s.path == "/app/rest/buildQueue").unwrap();
        assert!(trigger.body.contains(r#""buildType":{"id":"App_Build"}"#), "{}", trigger.body);
        assert!(trigger.body.contains(r#""name":"env.TARGET","value":"qa""#));
        assert_eq!(trigger.auth, "Bearer tc-t");
        let cancel = seen.iter().find(|s| s.path == "/app/rest/builds/id:71").unwrap();
        assert!(cancel.body.contains("readdIntoQueue"));
    });
}

#[test]
fn awx_launches_a_template_with_its_survey_and_cancels_the_job() {
    runtime().block_on(async {
        let (url, seen) = serve(vec![
            ("GET", "/api/v2/ping/", 200, r#"{"version":"24.6.1"}"#.into()),
            ("GET", "/api/v2/me/", 200, r#"{"results":[{"username":"ada"}]}"#.into()),
            (
                "GET",
                "/api/v2/job_templates/?",
                200,
                r#"{"next":null,"results":[{"id":7,"name":"Deploy","summary_fields":{"last_job":{"id":40,"status":"failed","finished":"2026-10-31T10:00:00Z"}}}]}"#.into(),
            ),
            (
                "GET",
                "/api/v2/job_templates/7/jobs/",
                200,
                r#"{"results":[{"id":41,"status":"running","started":"2026-10-31T10:05:00Z","elapsed":0}]}"#.into(),
            ),
            (
                "GET",
                "/api/v2/job_templates/7/",
                200,
                r#"{"id":7,"survey_enabled":true,"ask_variables_on_launch":true,"extra_vars":""}"#.into(),
            ),
            (
                "GET",
                "/api/v2/job_templates/7/survey_spec/",
                200,
                r#"{"spec":[{"variable":"env","type":"multiplechoice","choices":["staging","prod"],"default":"staging","question_name":"Where?","required":true}]}"#.into(),
            ),
            ("POST", "/api/v2/job_templates/7/launch/", 201, r#"{"job":42}"#.into()),
            ("POST", "/api/v2/jobs/41/cancel/", 202, "".into()),
            ("GET", "/api/v2/jobs/41/stdout/", 200, "PLAY [all]\nTASK [ping]\n".into()),
        ])
        .await;
        let mut d = driver::connect(&config(
            "awx",
            &url,
            Auth::Password {
                user: "ada".into(),
                password: "pw".into(),
            },
            TokenAs::Bearer,
        ))
        .await
        .expect("connect");
        assert_eq!(d.info().version, "24.6.1");
        let job = d.jobs().await.expect("templates").jobs[0].clone();
        assert_eq!(job.last.as_ref().unwrap().state, RunState::Failure);
        let form = d.form(&job).await.expect("form");
        let names: Vec<&str> = form.params.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["env", "extra_vars"]);
        let said = d
            .trigger(
                &job,
                &[
                    ("env".into(), "prod".into()),
                    ("extra_vars".into(), "replicas: 3".into()),
                ],
            )
            .await
            .expect("launch");
        assert!(said.contains("42"), "{said}");
        let run: Run = d.runs(&job).await.expect("jobs")[0].clone();
        assert_eq!(run.state, RunState::Running);
        d.cancel(&job, &run).await.expect("cancel");
        assert!(d.log(&job, &run, 0).await.expect("stdout").text.contains("PLAY"));
        let seen = seen.lock().clone();
        let launch = seen
            .iter()
            .find(|s| s.path == "/api/v2/job_templates/7/launch/")
            .unwrap();
        let body: serde_json::Value = serde_json::from_str(&launch.body).unwrap();
        assert_eq!(body["extra_vars"]["env"], "prod");
        assert_eq!(body["extra_vars"]["replicas"], 3);
        assert!(launch.auth.starts_with("Basic "));
    });
}

#[test]
fn github_dispatches_with_inputs_and_cancels_a_run() {
    runtime().block_on(async {
        let (url, seen) = serve(vec![
            (
                "GET",
                "/repos/o/r",
                200,
                r#"{"default_branch":"main"}"#.into(),
            ),
            (
                "POST",
                "/repos/o/r/actions/workflows/9/dispatches",
                204,
                "".into(),
            ),
            (
                "POST",
                "/repos/o/r/actions/runs/55/cancel",
                202,
                "{}".into(),
            ),
        ])
        .await;
        let mut cfg = config("github-actions", &url, token("ghp"), TokenAs::Bearer);
        cfg.repo = "o/r".into();
        let mut d = driver::connect(&cfg).await.expect("connect");
        let job = Job {
            id: "9".into(),
            name: "CI".into(),
            ..Default::default()
        };
        d.trigger(
            &job,
            &[("ref".into(), "dev".into()), ("env".into(), "qa".into())],
        )
        .await
        .expect("dispatch");
        let run = Run {
            id: "55".into(),
            number: "#3".into(),
            state: RunState::Running,
            ..Default::default()
        };
        d.cancel(&job, &run).await.expect("cancel");
        let seen = seen.lock().clone();
        let dispatch = seen
            .iter()
            .find(|s| s.path.ends_with("/dispatches"))
            .unwrap();
        let body: serde_json::Value = serde_json::from_str(&dispatch.body).unwrap();
        assert_eq!(body["ref"], "dev");
        assert_eq!(body["inputs"]["env"], "qa");
        assert_eq!(dispatch.auth, "Bearer ghp");
    });
}

#[test]
fn gitlab_creates_a_pipeline_with_variables_and_cancels_it() {
    runtime().block_on(async {
        let (url, seen) = serve(vec![
            (
                "GET",
                "/api/v4/projects/team%2Fapp",
                200,
                r#"{"id":5}"#.into(),
            ),
            (
                "GET",
                "/api/v4/version",
                200,
                r#"{"version":"18.5.0"}"#.into(),
            ),
            (
                "POST",
                "/api/v4/projects/team%2Fapp/pipeline",
                201,
                r#"{"id":900,"iid":31}"#.into(),
            ),
            (
                "POST",
                "/api/v4/projects/team%2Fapp/pipelines/900/cancel",
                200,
                "{}".into(),
            ),
        ])
        .await;
        let mut cfg = config(
            "gitlab-ci",
            &url,
            Auth::ApiKey {
                header: "PRIVATE-TOKEN".into(),
                key: "glpat".into(),
            },
            TokenAs::Bearer,
        );
        cfg.project = "team/app".into();
        let mut d = driver::connect(&cfg).await.expect("connect");
        assert_eq!(d.info().version, "18.5.0");
        let job = Job {
            id: "main".into(),
            name: "main".into(),
            ..Default::default()
        };
        let said = d
            .trigger(&job, &[("variables".into(), "DEPLOY=1\nREGION=eu".into())])
            .await
            .expect("pipeline");
        assert!(said.contains("#31"), "{said}");
        let run = Run {
            id: "900".into(),
            number: "#31".into(),
            state: RunState::Running,
            ..Default::default()
        };
        d.cancel(&job, &run).await.expect("cancel");
        let seen = seen.lock().clone();
        let create = seen.iter().find(|s| s.path.ends_with("/pipeline")).unwrap();
        let body: serde_json::Value = serde_json::from_str(&create.body).unwrap();
        assert_eq!(body["ref"], "main");
        assert_eq!(body["variables"][1]["key"], "REGION");
        assert_eq!(body["variables"][1]["value"], "eu");
    });
}

#[test]
fn a_read_only_session_refuses_a_run_before_the_server_sees_it() {
    runtime().block_on(async {
        let (url, seen) = serve(vec![
            ("GET", "/api/v2/ping/", 200, r#"{"version":"24"}"#.into()),
            ("GET", "/api/v2/me/", 200, "{}".into()),
        ])
        .await;
        let mut cfg = config("awx", &url, Auth::None, TokenAs::Bearer);
        cfg.read_only = true;
        let mut d = driver::connect(&cfg).await.expect("connect");
        let job = Job {
            id: "7".into(),
            ..Default::default()
        };
        assert!(
            d.trigger(&job, &[])
                .await
                .unwrap_err()
                .contains("read-only")
        );
        assert!(!seen.lock().iter().any(|s| s.method == "POST"));
    });
}
