//! One trait every server kind implements (docs/tools/automation.md §8),
//! and the types the tab sees. What a kind cannot do it says through
//! [`Caps`], and the tab leaves that control out.

use async_trait::async_trait;

use crate::profile::ConnectConfig;

pub type Result<T> = std::result::Result<T, String>;

/// Listing bounds (docs/tools/automation.md §7).
pub const MAX_JOBS: usize = 500;
pub const MAX_RUNS: usize = 50;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ServerInfo {
    /// "Jenkins", "TeamCity", "GitHub Actions", …
    pub product: String,
    pub version: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Caps {
    pub run: bool,
    pub cancel: bool,
    pub queue: bool,
    pub log: bool,
}

/// Where a run is, in the words every kind can be mapped to.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum RunState {
    Queued,
    Running,
    Success,
    Failure,
    /// Jenkins' and TeamCity's "passed with problems".
    Unstable,
    Cancelled,
    Skipped,
    #[default]
    Unknown,
}

impl RunState {
    /// The Fluent key the tab shows for it.
    pub fn key(&self) -> &'static str {
        match self {
            RunState::Queued => "automation-state-queued",
            RunState::Running => "automation-state-running",
            RunState::Success => "automation-state-success",
            RunState::Failure => "automation-state-failure",
            RunState::Unstable => "automation-state-unstable",
            RunState::Cancelled => "automation-state-cancelled",
            RunState::Skipped => "automation-state-skipped",
            RunState::Unknown => "automation-state-unknown",
        }
    }

    pub fn is_active(&self) -> bool {
        matches!(self, RunState::Queued | RunState::Running)
    }
}

/// One run: a build, a workflow run, a pipeline, an AWX job, a report.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Run {
    /// The server's id for it, what `cancel` and `log` take.
    pub id: String,
    /// What people call it: "#42".
    pub number: String,
    pub state: RunState,
    /// Branch, ref or environment.
    pub reference: String,
    /// Unix milliseconds.
    pub started_ms: Option<i64>,
    pub duration_ms: Option<i64>,
    pub who: String,
    /// The run's page on the server, for Open in Browser.
    pub url: String,
}

/// A job, a build configuration, a workflow, a ref with pipelines, a job
/// template or a node.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Job {
    /// The server's id for it, what the other calls take.
    pub id: String,
    /// The name shown, with any folder: "deploy/staging".
    pub name: String,
    pub last: Option<Run>,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ParamKind {
    Text,
    /// Several lines: AWX extra vars, Jenkins text parameters.
    LongText,
    Bool,
    Choice(Vec<String>),
    /// Typed, never shown back, never kept.
    Secret,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub name: String,
    pub kind: ParamKind,
    pub default: String,
    pub description: String,
    pub required: bool,
}

/// What Run asks for before it sends anything.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RunForm {
    /// `false` with `why` when this job cannot be started from here (a
    /// workflow without `workflow_dispatch`, a disabled job).
    pub allowed: bool,
    pub why: String,
    pub params: Vec<Param>,
    /// The user may add name = value pairs of their own (GitLab
    /// pipeline variables).
    pub free_vars: bool,
}

/// A piece of a run's log from an offset.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LogChunk {
    pub text: String,
    /// Where the next read starts.
    pub next: u64,
    /// The run is still writing.
    pub more: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct QueueItem {
    pub id: String,
    pub job: String,
    pub why: String,
    pub since_ms: Option<i64>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Listing {
    pub jobs: Vec<Job>,
    pub note: Option<String>,
}

#[async_trait]
pub trait Driver: Send {
    fn info(&self) -> &ServerInfo;
    fn caps(&self) -> Caps;
    async fn jobs(&mut self) -> Result<Listing>;
    async fn runs(&mut self, job: &Job) -> Result<Vec<Run>>;
    async fn form(&mut self, job: &Job) -> Result<RunForm>;
    /// Returns what the server said, for the toast.
    async fn trigger(&mut self, job: &Job, values: &[(String, String)]) -> Result<String>;
    async fn cancel(&mut self, job: &Job, run: &Run) -> Result<String>;
    async fn log(&mut self, job: &Job, run: &Run, from: u64) -> Result<LogChunk>;
    async fn queue(&mut self) -> Result<Vec<QueueItem>> {
        Ok(Vec::new())
    }
}

/// Open a session to the server `cfg` describes, within its timeout.
pub async fn connect(cfg: &ConnectConfig) -> Result<Box<dyn Driver>> {
    let open = async {
        let d: Box<dyn Driver> = match cfg.kind.as_str() {
            "jenkins" => Box::new(crate::jenkins::Jenkins::connect(cfg).await?),
            "teamcity" => Box::new(crate::teamcity::TeamCity::connect(cfg).await?),
            "github-actions" => Box::new(crate::github::GitHub::connect(cfg).await?),
            "gitlab-ci" => Box::new(crate::gitlab::GitLab::connect(cfg).await?),
            "forgejo-actions" => Box::new(crate::forgejo::Forgejo::connect(cfg).await?),
            "awx" => Box::new(crate::awx::Awx::connect(cfg).await?),
            other => return Err(format!("“{other}” is not an automation kind")),
        };
        Ok(d)
    };
    let inner = tokio::time::timeout(cfg.timeout, open)
        .await
        .map_err(|_| {
            format!(
                "the server did not answer within {} s",
                cfg.timeout.as_secs()
            )
        })??;
    Ok(Box::new(Guarded {
        inner,
        read_only: cfg.read_only,
    }))
}

/// Refuses every write on a read-only session, whatever asked for it: the
/// tab disables the controls, and this is what stops a palette command
/// getting round them (docs/tools/automation.md §6).
pub struct Guarded {
    inner: Box<dyn Driver>,
    read_only: bool,
}

impl Guarded {
    fn allow(&self, what: &str) -> Result<()> {
        if self.read_only {
            Err(format!(
                "{what} refused: this connection is read-only (a protected environment, or the profile says so)"
            ))
        } else {
            Ok(())
        }
    }
}

#[async_trait]
impl Driver for Guarded {
    fn info(&self) -> &ServerInfo {
        self.inner.info()
    }
    fn caps(&self) -> Caps {
        self.inner.caps()
    }
    async fn jobs(&mut self) -> Result<Listing> {
        self.inner.jobs().await
    }
    async fn runs(&mut self, job: &Job) -> Result<Vec<Run>> {
        self.inner.runs(job).await
    }
    async fn form(&mut self, job: &Job) -> Result<RunForm> {
        self.inner.form(job).await
    }
    async fn trigger(&mut self, job: &Job, values: &[(String, String)]) -> Result<String> {
        self.allow("Run")?;
        if !self.inner.caps().run {
            return Err("this server cannot start runs from here".into());
        }
        self.inner.trigger(job, values).await
    }
    async fn cancel(&mut self, job: &Job, run: &Run) -> Result<String> {
        self.allow("Cancel")?;
        if !self.inner.caps().cancel {
            return Err("this server cannot cancel runs from here".into());
        }
        self.inner.cancel(job, run).await
    }
    async fn log(&mut self, job: &Job, run: &Run, from: u64) -> Result<LogChunk> {
        self.inner.log(job, run, from).await
    }
    async fn queue(&mut self) -> Result<Vec<QueueItem>> {
        self.inner.queue().await
    }
}

/// The rest of a log a server returns whole, from `from` on, for kinds
/// with no offset of their own.
pub fn slice_from(text: String, from: u64, more: bool) -> LogChunk {
    let len = text.len() as u64;
    let start = (from.min(len)) as usize;
    // Never cut a character in half.
    let start = (start..=text.len())
        .find(|i| text.is_char_boundary(*i))
        .unwrap_or(text.len());
    LogChunk {
        text: text[start..].to_string(),
        next: len,
        more,
    }
}

/// A Unix time in milliseconds as the tables show it.
pub fn time_ms(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms)
        .map(|t| {
            t.with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_default()
}

/// RFC 3339 / ISO 8601 to Unix milliseconds.
pub fn iso_ms(s: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|t| t.timestamp_millis())
}

/// "1h 02m", "3m 07s", "12s".
pub fn duration(ms: i64) -> String {
    let s = (ms / 1000).max(0);
    let (h, m, s) = (s / 3600, (s % 3600) / 60, s % 60);
    if h > 0 {
        format!("{h}h {m:02}m")
    } else if m > 0 {
        format!("{m}m {s:02}s")
    } else {
        format!("{s}s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fake {
        info: ServerInfo,
        writes: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    }

    #[async_trait]
    impl Driver for Fake {
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
            Ok(Listing::default())
        }
        async fn runs(&mut self, _: &Job) -> Result<Vec<Run>> {
            Ok(Vec::new())
        }
        async fn form(&mut self, _: &Job) -> Result<RunForm> {
            Ok(RunForm::default())
        }
        async fn trigger(&mut self, _: &Job, _: &[(String, String)]) -> Result<String> {
            self.writes
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(String::new())
        }
        async fn cancel(&mut self, _: &Job, _: &Run) -> Result<String> {
            self.writes
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(String::new())
        }
        async fn log(&mut self, _: &Job, _: &Run, _: u64) -> Result<LogChunk> {
            Ok(LogChunk::default())
        }
    }

    #[test]
    fn a_read_only_session_refuses_run_and_cancel_before_the_server_sees_them() {
        let writes = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut g = Guarded {
            inner: Box::new(Fake {
                info: ServerInfo::default(),
                writes: writes.clone(),
            }),
            read_only: true,
        };
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("runtime");
        rt.block_on(async {
            let job = Job::default();
            assert!(
                g.trigger(&job, &[])
                    .await
                    .unwrap_err()
                    .contains("read-only")
            );
            assert!(g.cancel(&job, &Run::default()).await.is_err());
            assert!(g.jobs().await.is_ok());
        });
        assert_eq!(writes.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    #[test]
    fn a_whole_log_is_sliced_from_the_offset_on_a_character_boundary() {
        let c = slice_from("abc€def".into(), 3, true);
        assert_eq!(c.text, "€def");
        assert_eq!(c.next, 9);
        // Inside the euro sign: the slice moves to the next boundary.
        assert_eq!(slice_from("abc€def".into(), 4, false).text, "def");
        assert_eq!(slice_from("abc".into(), 99, false).text, "");
    }

    #[test]
    fn durations_read_as_people_say_them() {
        assert_eq!(duration(12_000), "12s");
        assert_eq!(duration(187_000), "3m 07s");
        assert_eq!(duration(3_720_000), "1h 02m");
        assert_eq!(iso_ms("2026-10-31T10:00:00Z"), Some(1_793_440_800_000));
    }
}
