//! A tab's session (docs/tools/analytics.md §7): one task on the tokio
//! runtime signs in and reads the server's facts; the tab reads the result
//! at frame start. Nothing here runs on the UI thread and nothing calls
//! `block_on`. Work for the pages (a collection's items, a question's run)
//! joins this as jobs with their own topics in the steps that add them.

use std::sync::Arc;

use hs_core::runtime::AppRuntime;
use parking_lot::Mutex;

use crate::provider::{BiProvider, ServerInfo};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Connecting,
    Ready,
    Failed(String),
}

/// What the session task writes and the tab reads.
#[derive(Debug)]
pub struct Shared {
    pub status: Status,
    pub info: ServerInfo,
}

/// One tab's connection. Dropping it ends the session: the task is
/// aborted and the server-side session, where there is one, is ended.
pub struct Session {
    shared: Arc<Mutex<Shared>>,
    provider: Arc<dyn BiProvider>,
    runtime: AppRuntime,
    task: tokio::task::JoinHandle<()>,
}

impl Session {
    /// Start connecting; the status is `Connecting` until the task ends.
    pub fn spawn(runtime: &AppRuntime, provider: Arc<dyn BiProvider>) -> Session {
        let shared = Arc::new(Mutex::new(Shared {
            status: Status::Connecting,
            info: ServerInfo::default(),
        }));
        let (p, sh) = (provider.clone(), shared.clone());
        let task = runtime.spawn(async move {
            let result = p.connect().await;
            let mut g = sh.lock();
            match result {
                Ok(info) => {
                    g.info = info;
                    g.status = Status::Ready;
                }
                Err(e) => g.status = Status::Failed(e),
            }
        });
        Session {
            shared,
            provider,
            runtime: runtime.clone(),
            task,
        }
    }

    pub fn status(&self) -> Status {
        self.shared.lock().status.clone()
    }

    pub fn info(&self) -> ServerInfo {
        self.shared.lock().info.clone()
    }

    pub fn provider(&self) -> &Arc<dyn BiProvider> {
        &self.provider
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.task.abort();
        let provider = self.provider.clone();
        self.runtime
            .spawn(async move { provider.disconnect().await });
    }
}
