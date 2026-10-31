//! The tab's server session, owned by one task on the tokio runtime. The
//! UI sends [`Cmd`]s through a bounded channel and reads [`Shared`] at
//! frame start; nothing here runs on the UI thread (ADR-013).

use std::sync::Arc;

use parking_lot::Mutex;
use tokio::sync::mpsc;

use crate::driver::{self, Caps, Job, Listing, QueueItem, Run, RunForm, ServerInfo};
use crate::profile::ConnectConfig;

#[derive(Debug, Clone, PartialEq)]
pub enum Status {
    Connecting,
    Ready,
    Failed(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Cmd {
    Refresh,
    Runs(Job),
    Form(Job),
    Trigger(Job, Vec<(String, String)>),
    Cancel(Job, Run),
    /// Read the run's log on from where the last read stopped.
    Log(Job, Run),
    Queue,
}

/// A result for one job, so a late answer for the job the user has since
/// left is not shown under the one they are on now.
#[derive(Debug, Clone, PartialEq)]
pub struct For<T> {
    pub job: String,
    pub result: Result<T, String>,
}

/// The log of one run, as much as has been read.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LogBuf {
    pub job: String,
    pub run: String,
    pub text: String,
    /// Where the next read starts.
    pub next: u64,
    /// The run is still writing.
    pub more: bool,
    /// The beginning was dropped to stay inside the bound.
    pub cut: bool,
    pub error: Option<String>,
}

#[derive(Debug, Default)]
pub struct Shared {
    pub status: Option<Status>,
    pub info: ServerInfo,
    pub caps: Caps,
    pub listing: Option<Listing>,
    pub listing_error: Option<String>,
    pub runs: Option<For<Vec<Run>>>,
    pub form: Option<For<RunForm>>,
    pub log: Option<LogBuf>,
    pub queue: Option<Result<Vec<QueueItem>, String>>,
    /// What is running now, as a Fluent key, for the spinner.
    pub busy: Option<&'static str>,
    /// (success, text) for the tab to toast.
    pub toasts: Vec<(bool, String)>,
}

/// Keep `buf` inside `max` bytes by dropping its beginning, on a line
/// boundary where there is one.
pub fn bound(buf: &mut LogBuf, max: usize) {
    if buf.text.len() <= max {
        return;
    }
    let mut cut = buf.text.len() - max;
    while !buf.text.is_char_boundary(cut) {
        cut += 1;
    }
    let cut = buf.text[cut..]
        .find('\n')
        .map(|i| cut + i + 1)
        .unwrap_or(cut);
    buf.text.drain(..cut);
    buf.cut = true;
}

pub fn spawn(
    runtime: &hs_core::runtime::AppRuntime,
    config: ConnectConfig,
    shared: Arc<Mutex<Shared>>,
    log_max: usize,
) -> mpsc::Sender<Cmd> {
    let (tx, mut rx) = mpsc::channel::<Cmd>(16);
    let waker = runtime.waker();
    shared.lock().status = Some(Status::Connecting);
    runtime.spawn(async move {
        let mut d = match driver::connect(&config).await {
            Ok(d) => d,
            Err(e) => {
                shared.lock().status = Some(Status::Failed(e));
                waker.request_repaint();
                return;
            }
        };
        {
            let mut sh = shared.lock();
            sh.status = Some(Status::Ready);
            sh.info = d.info().clone();
            sh.caps = d.caps();
        }
        waker.request_repaint();
        // Polls pile up behind a slow server; one is enough.
        let mut pending = None;
        while let Some(cmd) =
            hs_core::runtime::next_coalesced(&mut rx, &mut pending, &Cmd::Refresh).await
        {
            shared.lock().busy = Some(busy_key(&cmd));
            waker.request_repaint();
            serve(cmd, d.as_mut(), &shared, &config, log_max).await;
            shared.lock().busy = None;
            waker.request_repaint();
        }
    });
    tx
}

fn busy_key(cmd: &Cmd) -> &'static str {
    match cmd {
        Cmd::Refresh => "automation-busy-refresh",
        Cmd::Runs(_) => "automation-busy-runs",
        Cmd::Form(_) => "automation-busy-form",
        Cmd::Trigger(..) => "automation-busy-trigger",
        Cmd::Cancel(..) => "automation-busy-cancel",
        Cmd::Log(..) => "automation-busy-log",
        Cmd::Queue => "automation-busy-queue",
    }
}

async fn serve(
    cmd: Cmd,
    d: &mut dyn driver::Driver,
    shared: &Arc<Mutex<Shared>>,
    cfg: &ConnectConfig,
    log_max: usize,
) {
    let toast = |ok: bool, text: String| shared.lock().toasts.push((ok, text));
    match cmd {
        Cmd::Refresh => {
            let r = d.jobs().await;
            let mut sh = shared.lock();
            match r {
                Ok(l) => {
                    sh.listing = Some(l);
                    sh.listing_error = None;
                }
                Err(e) => sh.listing_error = Some(e),
            }
        }
        Cmd::Runs(job) => {
            let r = d.runs(&job).await;
            shared.lock().runs = Some(For {
                job: job.id,
                result: r,
            });
        }
        Cmd::Form(job) => {
            let r = d.form(&job).await;
            shared.lock().form = Some(For {
                job: job.id,
                result: r,
            });
        }
        Cmd::Trigger(job, values) => match d.trigger(&job, &values).await {
            Ok(said) => {
                // Never the values: a parameter may be a secret.
                tracing::info!("automation: ran {} on {}", job.name, cfg.kind);
                toast(true, said);
                if let Ok(r) = d.runs(&job).await {
                    shared.lock().runs = Some(For {
                        job: job.id,
                        result: Ok(r),
                    });
                }
            }
            Err(e) => toast(false, e),
        },
        Cmd::Cancel(job, run) => match d.cancel(&job, &run).await {
            Ok(said) => {
                tracing::info!(
                    "automation: cancelled {} {} on {}",
                    job.name,
                    run.number,
                    cfg.kind
                );
                toast(true, said);
            }
            Err(e) => toast(false, e),
        },
        Cmd::Log(job, run) => {
            let from = {
                let sh = shared.lock();
                match &sh.log {
                    Some(b) if b.job == job.id && b.run == run.id => b.next,
                    _ => 0,
                }
            };
            let r = d.log(&job, &run, from).await;
            let mut sh = shared.lock();
            let same = sh
                .log
                .as_ref()
                .is_some_and(|b| b.job == job.id && b.run == run.id);
            if !same {
                sh.log = Some(LogBuf {
                    job: job.id.clone(),
                    run: run.id.clone(),
                    ..Default::default()
                });
            }
            let buf = sh.log.as_mut().expect("set above");
            match r {
                Ok(chunk) => {
                    buf.text.push_str(&chunk.text);
                    buf.next = chunk.next;
                    buf.more = chunk.more;
                    buf.error = None;
                    bound(buf, log_max);
                }
                Err(e) => {
                    buf.more = false;
                    buf.error = Some(e);
                }
            }
        }
        Cmd::Queue => {
            let r = d.queue().await;
            shared.lock().queue = Some(r);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_log_loses_its_beginning_on_a_line_boundary() {
        let mut b = LogBuf {
            text: "one\ntwo\nthree\n".into(),
            ..Default::default()
        };
        bound(&mut b, 9);
        assert_eq!(b.text, "three\n");
        assert!(b.cut);
        let mut small = LogBuf {
            text: "ok\n".into(),
            ..Default::default()
        };
        bound(&mut small, 9);
        assert!(!small.cut);
    }
}
