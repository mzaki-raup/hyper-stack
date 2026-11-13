//! A tab's session (docs/tools/idm.md §7): one task on the tokio runtime
//! owns the guarded provider and its token. The tab hands it work through
//! a bounded channel ([`Session::submit`]) and drains the answers at
//! frame start ([`Session::take`]); nothing here runs on the UI thread and
//! nothing calls `block_on`.
//!
//! Work is grouped by **topic** - a name the tab picks, one per thing it
//! shows ("users", "user", "groups.children") - and each topic keeps only
//! its newest request: a new one aborts the one still running (the
//! request future is dropped, so the work stops) and an older answer is
//! never shown under a newer request.

use std::any::Any;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use parking_lot::Mutex;
use tokio::sync::mpsc;

use crate::guarded::{Guarded, WriteGate};
use crate::provider::{Caps, IdmError, IdmProvider, Realm, ServerInfo};

/// How many requests may wait for the session task.
pub const QUEUE: usize = 32;

type Answer = Box<dyn Any + Send>;
type Work = Pin<Box<dyn Future<Output = Answer> + Send>>;
type JobFn = Box<dyn FnOnce(Arc<Guarded>) -> Work + Send>;

struct Job {
    topic: &'static str,
    generation: u64,
    run: JobFn,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Status {
    Connecting,
    Ready,
    Failed(IdmError),
}

/// What the session task writes and the tab reads.
pub struct Shared {
    pub status: Status,
    pub info: ServerInfo,
    pub caps: Caps,
    /// The realm picker's list, read once on connect.
    pub realms: Option<Result<Vec<Realm>, IdmError>>,
    /// The newest answer per topic, with its generation.
    answers: HashMap<&'static str, (u64, Answer)>,
    /// The generation each topic is running, while it runs.
    running: HashMap<&'static str, u64>,
}

impl Default for Shared {
    fn default() -> Self {
        Shared {
            status: Status::Connecting,
            info: ServerInfo::default(),
            caps: Caps::default(),
            realms: None,
            answers: HashMap::new(),
            running: HashMap::new(),
        }
    }
}

/// Why [`Session::submit`] did not queue the work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubmitError {
    /// [`QUEUE`] requests are already waiting.
    Busy,
    /// The session task has ended.
    Closed,
}

/// One tab's connection. Dropping it ends the session: running work is
/// aborted and the password kind's admin session is logged out.
pub struct Session {
    tx: mpsc::Sender<Job>,
    shared: Arc<Mutex<Shared>>,
    provider: Arc<Guarded>,
    generations: HashMap<&'static str, u64>,
}

impl Session {
    /// Connect (the token, the server facts, the realm list) and start
    /// serving the tab's requests.
    pub fn spawn(runtime: &hs_core::runtime::AppRuntime, provider: Arc<Guarded>) -> Session {
        let (tx, mut rx) = mpsc::channel::<Job>(QUEUE);
        let shared = Arc::new(Mutex::new(Shared::default()));
        let waker = runtime.waker();
        let task_shared = shared.clone();
        let p = provider.clone();
        runtime.spawn(async move {
            match p.connect().await {
                Ok(info) => {
                    let realms = p.realms().await;
                    let mut sh = task_shared.lock();
                    sh.status = Status::Ready;
                    sh.info = info;
                    sh.caps = p.capabilities();
                    sh.realms = Some(realms);
                }
                Err(e) => {
                    task_shared.lock().status = Status::Failed(e);
                    waker.request_repaint();
                    // The tab reconnects with a new session; nothing more
                    // is served on this one.
                    return;
                }
            }
            waker.request_repaint();
            let mut handles: HashMap<&'static str, tokio::task::JoinHandle<()>> = HashMap::new();
            while let Some(job) = rx.recv().await {
                if let Some(old) = handles.remove(job.topic) {
                    old.abort();
                }
                task_shared.lock().running.insert(job.topic, job.generation);
                let sh = task_shared.clone();
                let w = waker.clone();
                let p = p.clone();
                let topic = job.topic;
                let generation = job.generation;
                handles.insert(
                    topic,
                    tokio::spawn(async move {
                        let answer = (job.run)(p.clone()).await;
                        let mut s = sh.lock();
                        // A newer request for this topic has started since.
                        if s.running.get(topic) == Some(&generation) {
                            s.running.remove(topic);
                            s.answers.insert(topic, (generation, answer));
                        }
                        // Writes can change what the account may do.
                        s.caps = p.capabilities();
                        drop(s);
                        w.request_repaint();
                    }),
                );
                handles.retain(|_, h| !h.is_finished());
            }
            for (_, h) in handles {
                h.abort();
            }
            p.disconnect().await;
        });
        Session {
            tx,
            shared,
            provider,
            generations: HashMap::new(),
        }
    }

    /// The shared state, for the status, info, caps and realm list.
    pub fn shared(&self) -> parking_lot::MutexGuard<'_, Shared> {
        self.shared.lock()
    }

    pub fn status(&self) -> Status {
        self.shared.lock().status.clone()
    }

    pub fn gate(&self) -> &Arc<WriteGate> {
        self.provider.gate()
    }

    /// The provider, for a [`hs_core::jobs`] Job (an export, a subtree
    /// walk) that needs its own progress and cancel.
    pub fn provider(&self) -> Arc<Guarded> {
        self.provider.clone()
    }

    /// Queue `work` under `topic`, superseding any earlier request for
    /// the same topic. Returns the request's generation.
    pub fn submit<T, F, Fut>(&mut self, topic: &'static str, work: F) -> Result<u64, SubmitError>
    where
        T: Send + 'static,
        F: FnOnce(Arc<Guarded>) -> Fut + Send + 'static,
        Fut: Future<Output = T> + Send + 'static,
    {
        let generation = self.generations.get(topic).copied().unwrap_or(0) + 1;
        let run: JobFn =
            Box::new(move |p| Box::pin(async move { Box::new(work(p).await) as Answer }));
        match self.tx.try_send(Job {
            topic,
            generation,
            run,
        }) {
            Ok(()) => {
                self.generations.insert(topic, generation);
                Ok(generation)
            }
            Err(mpsc::error::TrySendError::Full(_)) => Err(SubmitError::Busy),
            Err(mpsc::error::TrySendError::Closed(_)) => Err(SubmitError::Closed),
        }
    }

    /// The answer to the newest request of `topic`, once, as the type the
    /// work returned. An answer of another type, or to an older request,
    /// is dropped.
    pub fn take<T: 'static>(&self, topic: &'static str) -> Option<T> {
        let mut sh = self.shared.lock();
        let (generation, answer) = sh.answers.remove(topic)?;
        if self.generations.get(topic) != Some(&generation) {
            return None;
        }
        answer.downcast::<T>().ok().map(|b| *b)
    }

    /// Whether `topic` has a request running (for its loading state).
    pub fn busy(&self, topic: &'static str) -> bool {
        self.shared.lock().running.contains_key(topic)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audit::AuditContext;
    use crate::guarded::tests_support::Fake;
    use crate::provider::{Page, Paged, UserQuery};
    use std::time::Duration;

    fn session() -> (hs_core::runtime::AppRuntime, Session) {
        let rt = hs_core::runtime::AppRuntime::new(Arc::new(hs_core::runtime::NoopWaker))
            .expect("runtime");
        let p = Arc::new(Guarded::new(
            Arc::new(Fake::default()),
            WriteGate::open(),
            None,
            AuditContext::default(),
        ));
        let s = Session::spawn(&rt, p);
        (rt, s)
    }

    fn wait_for<T: 'static>(s: &Session, topic: &'static str) -> Option<T> {
        for _ in 0..200 {
            if let Some(v) = s.take::<T>(topic) {
                return Some(v);
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        None
    }

    #[test]
    fn a_newer_request_supersedes_an_older_one_of_the_same_topic() {
        let (_rt, mut s) = session();
        s.submit("users", |_| async {
            tokio::time::sleep(Duration::from_millis(300)).await;
            "old"
        })
        .expect("queued");
        s.submit("users", |_| async { "new" }).expect("queued");
        assert_eq!(wait_for::<&str>(&s, "users"), Some("new"));
        // The old one was aborted; nothing else arrives.
        std::thread::sleep(Duration::from_millis(400));
        assert_eq!(s.take::<&str>("users"), None);
        assert_eq!(s.status(), Status::Ready);
    }

    #[test]
    fn work_reaches_the_provider_and_answers_its_own_type() {
        let (_rt, mut s) = session();
        s.submit("list", |p| async move {
            p.users("hs-demo", &UserQuery::default(), Page::default())
                .await
        })
        .expect("queued");
        let got = wait_for::<crate::provider::Result<Paged<crate::provider::UserRow>>>(&s, "list");
        assert!(matches!(got, Some(Ok(_))));
        s.submit("other", |_| async { 7u32 }).expect("queued");
        // Asked for as the wrong type, it is dropped rather than misread.
        for _ in 0..100 {
            if !s.busy("other") {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(s.take::<String>("other"), None);
        assert!(s.shared().realms.is_some());
    }
}
