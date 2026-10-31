//! The tab's broker session, owned by one task on the tokio runtime. The
//! UI sends [`Cmd`]s through a bounded channel and reads [`Shared`] at
//! frame start; nothing here runs on the UI thread (ADR-013).

use std::sync::Arc;

use parking_lot::Mutex;
use tokio::sync::mpsc;

use crate::driver::{
    self, Binding, Caps, ConsumerGroup, Destination, Listing, Message, PeekRequest, Progress,
    PublishRequest, ServerInfo,
};
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
    Peek(Destination, PeekRequest),
    Publish(PublishRequest),
    Consumers(Destination),
    Bindings(Destination),
    Purge(Destination),
    Delete(Destination),
}

/// A result for one destination, so a late answer for the queue the user
/// has since left is not shown under the one they are on now.
#[derive(Debug, Clone, PartialEq)]
pub struct For<T> {
    pub dest: String,
    pub result: Result<T, String>,
}

#[derive(Debug, Default)]
pub struct Shared {
    pub status: Option<Status>,
    pub info: ServerInfo,
    pub caps: Caps,
    pub listing: Option<Listing>,
    pub listing_error: Option<String>,
    pub messages: Option<For<Vec<Message>>>,
    pub consumers: Option<For<Vec<ConsumerGroup>>>,
    pub bindings: Option<For<Vec<Binding>>>,
    /// What is running now, as a Fluent key, for the spinner.
    pub busy: Option<&'static str>,
    pub progress: Arc<Progress>,
    /// (success, text) for the tab to toast.
    pub toasts: Vec<(bool, String)>,
    /// Bumped by every refresh, so the dashboard's figures can follow.
    pub generation: u64,
}

pub fn spawn(
    runtime: &hs_core::runtime::AppRuntime,
    config: ConnectConfig,
    shared: Arc<Mutex<Shared>>,
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
        let progress = shared.lock().progress.clone();
        while let Some(cmd) = rx.recv().await {
            // Polls pile up behind a slow broker; one is enough.
            if cmd == Cmd::Refresh {
                while let Ok(Cmd::Refresh) = rx.try_recv() {}
            }
            shared.lock().busy = Some(busy_key(&cmd));
            waker.request_repaint();
            let refresh_after = serve(cmd, d.as_mut(), &shared, &progress, &config).await;
            if refresh_after {
                list(d.as_mut(), &shared).await;
            }
            shared.lock().busy = None;
            waker.request_repaint();
        }
    });
    tx
}

fn busy_key(cmd: &Cmd) -> &'static str {
    match cmd {
        Cmd::Refresh => "queue-busy-refresh",
        Cmd::Peek(..) => "queue-busy-peek",
        Cmd::Publish(_) => "queue-busy-publish",
        Cmd::Consumers(_) => "queue-busy-consumers",
        Cmd::Bindings(_) => "queue-busy-bindings",
        Cmd::Purge(_) => "queue-busy-purge",
        Cmd::Delete(_) => "queue-busy-delete",
    }
}

async fn list(d: &mut dyn driver::Driver, shared: &Arc<Mutex<Shared>>) {
    let r = d.destinations().await;
    let mut sh = shared.lock();
    sh.generation += 1;
    match r {
        Ok(l) => {
            sh.listing = Some(l);
            sh.listing_error = None;
        }
        Err(e) => sh.listing_error = Some(e),
    }
}

/// Run one command; `true` when the listing should be read again because
/// the broker changed.
async fn serve(
    cmd: Cmd,
    d: &mut dyn driver::Driver,
    shared: &Arc<Mutex<Shared>>,
    progress: &Arc<Progress>,
    cfg: &ConnectConfig,
) -> bool {
    let toast = |ok: bool, text: String| shared.lock().toasts.push((ok, text));
    match cmd {
        Cmd::Refresh => {
            list(d, shared).await;
            false
        }
        Cmd::Peek(dest, req) => {
            let r = d.peek(&dest, &req).await;
            shared.lock().messages = Some(For {
                dest: dest.name,
                result: r,
            });
            false
        }
        Cmd::Publish(req) => match d.publish(&req).await {
            Ok(said) => {
                tracing::info!("queue: published to {} on {}", req.target, cfg.kind);
                toast(true, said);
                true
            }
            Err(e) => {
                toast(false, e);
                false
            }
        },
        Cmd::Consumers(dest) => {
            progress.start(0);
            let r = d.consumers(&dest, progress).await;
            let r = if progress.cancelled() {
                Err("stopped".to_string())
            } else {
                r
            };
            shared.lock().consumers = Some(For {
                dest: dest.name,
                result: r,
            });
            false
        }
        Cmd::Bindings(dest) => {
            let r = d.bindings(&dest).await;
            shared.lock().bindings = Some(For {
                dest: dest.name,
                result: r,
            });
            false
        }
        Cmd::Purge(dest) => match d.purge(&dest).await {
            Ok(said) => {
                tracing::info!("queue: purged {} on {}", dest.name, cfg.kind);
                toast(true, said);
                true
            }
            Err(e) => {
                toast(false, e);
                false
            }
        },
        Cmd::Delete(dest) => match d.delete(&dest).await {
            Ok(()) => {
                tracing::info!("queue: deleted {} on {}", dest.name, cfg.kind);
                toast(true, format!("deleted “{}”", dest.name));
                true
            }
            Err(e) => {
                toast(false, e);
                false
            }
        },
    }
}
