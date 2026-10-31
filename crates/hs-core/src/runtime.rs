//! One tokio runtime for the whole app (ADR-013) and the handle background
//! work uses to ask the UI for a repaint.

use std::sync::Arc;

use tokio::runtime::Runtime;

/// Wakes the UI thread. Implemented by the app over `egui::Context`;
/// a no-op implementation serves tests and `--self-test`.
pub trait UiWaker: Send + Sync {
    fn request_repaint(&self);
}

pub struct NoopWaker;
impl UiWaker for NoopWaker {
    fn request_repaint(&self) {}
}

/// Wake-up through an egui context.
pub struct EguiWaker(pub egui::Context);
impl UiWaker for EguiWaker {
    fn request_repaint(&self) {
        self.0.request_repaint();
    }
}

#[derive(Clone)]
pub struct AppRuntime {
    rt: Arc<Runtime>,
    waker: Arc<dyn UiWaker>,
}

impl AppRuntime {
    pub fn new(waker: Arc<dyn UiWaker>) -> std::io::Result<Self> {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_name("hs-rt")
            .worker_threads(
                std::thread::available_parallelism()
                    .map(|n| n.get().clamp(2, 8))
                    .unwrap_or(4),
            )
            .build()?;
        Ok(Self {
            rt: Arc::new(rt),
            waker,
        })
    }

    pub fn handle(&self) -> tokio::runtime::Handle {
        self.rt.handle().clone()
    }

    pub fn waker(&self) -> Arc<dyn UiWaker> {
        self.waker.clone()
    }

    /// Replace the waker once the egui context exists.
    pub fn with_waker(&self, waker: Arc<dyn UiWaker>) -> Self {
        Self {
            rt: self.rt.clone(),
            waker,
        }
    }

    /// Spawn a future; the UI is repainted when it completes.
    pub fn spawn<F>(&self, fut: F) -> tokio::task::JoinHandle<F::Output>
    where
        F: std::future::Future + Send + 'static,
        F::Output: Send + 'static,
    {
        let waker = self.waker.clone();
        self.rt.spawn(async move {
            let out = fut.await;
            waker.request_repaint();
            out
        })
    }

    /// Run blocking work on the blocking pool (drivers that are not async).
    pub fn spawn_blocking<F, R>(&self, f: F) -> tokio::task::JoinHandle<R>
    where
        F: FnOnce() -> R + Send + 'static,
        R: Send + 'static,
    {
        let waker = self.waker.clone();
        self.rt.spawn_blocking(move || {
            let out = f();
            waker.request_repaint();
            out
        })
    }
}

/// The next command for a runner task, with repeated `coalesce` commands
/// (polls that piled up behind a slow server) merged into one.
///
/// Reading past the repeats takes one more message off the channel; when
/// that message is something else it is kept in `pending` and returned
/// next, never dropped. (A `while let Ok(Refresh) = rx.try_recv()` loop
/// dropped it: a poll's Refresh-then-Runs lost the Runs, found
/// 2026-10-31.)
pub async fn next_coalesced<T: PartialEq>(
    rx: &mut tokio::sync::mpsc::Receiver<T>,
    pending: &mut Option<T>,
    coalesce: &T,
) -> Option<T> {
    let cmd = match pending.take() {
        Some(c) => c,
        None => rx.recv().await?,
    };
    if &cmd == coalesce {
        loop {
            match rx.try_recv() {
                Ok(next) if &next == coalesce => continue,
                Ok(next) => {
                    *pending = Some(next);
                    break;
                }
                Err(_) => break,
            }
        }
    }
    Some(cmd)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_polls_merge_and_the_command_behind_them_is_kept() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("runtime");
        rt.block_on(async {
            let (tx, mut rx) = tokio::sync::mpsc::channel::<&str>(8);
            for c in ["refresh", "refresh", "runs", "refresh", "log"] {
                tx.send(c).await.expect("send");
            }
            drop(tx);
            let mut pending = None;
            let mut got = Vec::new();
            while let Some(c) = next_coalesced(&mut rx, &mut pending, &"refresh").await {
                got.push(c);
            }
            assert_eq!(got, ["refresh", "runs", "refresh", "log"]);
        });
    }
}
