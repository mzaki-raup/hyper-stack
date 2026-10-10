//! Background jobs: anything slower than ~200 ms gets an id, a title, progress,
//! a cancel token and a place in the Jobs panel.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use parking_lot::Mutex;
use tokio_util::sync::CancellationToken;

use crate::runtime::AppRuntime;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct JobId(pub u64);

#[derive(Debug, Clone, PartialEq)]
pub enum Progress {
    Indeterminate,
    Fraction(f32),
    Bytes { done: u64, total: Option<u64> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobStatus {
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone)]
pub struct JobInfo {
    pub id: JobId,
    pub title: String,
    pub tool: String,
    pub started: Instant,
    pub finished: Option<Instant>,
    pub status: JobStatus,
    pub progress: Progress,
    pub message: Option<String>,
    pub log: Vec<String>,
}

struct Shared {
    info: Mutex<JobInfo>,
    cancel: CancellationToken,
}

/// Handed to the running task so it can report progress and check for cancel.
#[derive(Clone)]
pub struct JobHandle {
    shared: Arc<Shared>,
}

impl JobHandle {
    pub fn id(&self) -> JobId {
        self.shared.info.lock().id
    }
    pub fn set_progress(&self, p: Progress) {
        self.shared.info.lock().progress = p;
    }
    pub fn set_message(&self, m: impl Into<String>) {
        self.shared.info.lock().message = Some(m.into());
    }
    pub fn log(&self, line: impl Into<String>) {
        let mut i = self.shared.info.lock();
        i.log.push(line.into());
        if i.log.len() > 2000 {
            i.log.drain(..1000);
        }
    }
    pub fn is_cancelled(&self) -> bool {
        self.shared.cancel.is_cancelled()
    }
    pub fn cancel_token(&self) -> CancellationToken {
        self.shared.cancel.clone()
    }
    fn finish(&self, status: JobStatus, message: Option<String>) {
        let mut i = self.shared.info.lock();
        i.status = status;
        i.finished = Some(Instant::now());
        if message.is_some() {
            i.message = message;
        }
    }
}

/// The registry the Jobs panel reads.
#[derive(Clone)]
pub struct Jobs {
    next: Arc<AtomicU64>,
    jobs: Arc<Mutex<Vec<Arc<Shared>>>>,
    runtime: AppRuntime,
}

impl Jobs {
    pub fn new(runtime: AppRuntime) -> Self {
        Self {
            next: Arc::new(AtomicU64::new(1)),
            jobs: Arc::new(Mutex::new(Vec::new())),
            runtime,
        }
    }

    /// Register a job and run `f` with its handle. The future's `Result`
    /// decides success or failure; cancellation is reported when the token was
    /// triggered and the future returned an error.
    pub fn spawn<F, Fut>(
        &self,
        title: impl Into<String>,
        tool: impl Into<String>,
        f: F,
    ) -> JobHandle
    where
        F: FnOnce(JobHandle) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<(), String>> + Send + 'static,
    {
        let id = JobId(self.next.fetch_add(1, Ordering::Relaxed));
        let shared = Arc::new(Shared {
            info: Mutex::new(JobInfo {
                id,
                title: title.into(),
                tool: tool.into(),
                started: Instant::now(),
                finished: None,
                status: JobStatus::Running,
                progress: Progress::Indeterminate,
                message: None,
                log: Vec::new(),
            }),
            cancel: CancellationToken::new(),
        });
        self.jobs.lock().push(shared.clone());
        let handle = JobHandle { shared };
        let h2 = handle.clone();
        self.runtime.spawn(async move {
            match f(h2.clone()).await {
                Ok(()) => h2.finish(JobStatus::Succeeded, None),
                Err(e) if h2.is_cancelled() => h2.finish(JobStatus::Cancelled, Some(e)),
                Err(e) => h2.finish(JobStatus::Failed, Some(e)),
            }
        });
        handle
    }

    pub fn cancel(&self, id: JobId) {
        if let Some(j) = self.jobs.lock().iter().find(|j| j.info.lock().id == id) {
            j.cancel.cancel();
        }
    }

    pub fn snapshot(&self) -> Vec<JobInfo> {
        self.jobs
            .lock()
            .iter()
            .map(|j| j.info.lock().clone())
            .collect()
    }

    pub fn running_count(&self) -> usize {
        self.jobs
            .lock()
            .iter()
            .filter(|j| j.info.lock().status == JobStatus::Running)
            .count()
    }

    /// Drop finished jobs older than `keep` entries.
    pub fn prune(&self, keep: usize) {
        let mut jobs = self.jobs.lock();
        let finished: Vec<usize> = jobs
            .iter()
            .enumerate()
            .filter(|(_, j)| j.info.lock().status != JobStatus::Running)
            .map(|(i, _)| i)
            .collect();
        if finished.len() > keep {
            for i in finished[..finished.len() - keep].iter().rev() {
                jobs.remove(*i);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::NoopWaker;

    #[test]
    fn job_runs_reports_and_finishes() {
        let rt = AppRuntime::new(Arc::new(NoopWaker)).unwrap();
        let jobs = Jobs::new(rt.clone());
        let h = jobs.spawn("copy", "test", |h| async move {
            h.set_progress(Progress::Fraction(0.5));
            h.log("half way");
            Ok(())
        });
        let start = Instant::now();
        while jobs.snapshot()[0].status == JobStatus::Running {
            assert!(start.elapsed().as_secs() < 5, "job never finished");
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let info = jobs.snapshot().remove(0);
        assert_eq!(info.id, h.id());
        assert_eq!(info.status, JobStatus::Succeeded);
        assert_eq!(info.log, vec!["half way".to_string()]);
    }

    #[test]
    fn cancelled_job_is_marked_cancelled() {
        let rt = AppRuntime::new(Arc::new(NoopWaker)).unwrap();
        let jobs = Jobs::new(rt.clone());
        let h = jobs.spawn("slow", "test", |h| async move {
            h.cancel_token().cancelled().await;
            Err("stopped".to_string())
        });
        jobs.cancel(h.id());
        let start = Instant::now();
        while jobs.snapshot()[0].status == JobStatus::Running {
            assert!(start.elapsed().as_secs() < 5);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(jobs.snapshot()[0].status, JobStatus::Cancelled);
    }
}
