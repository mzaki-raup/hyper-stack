//! `hs_core::tunnels::Tunnels` on russh: SSH local forwards (`ssh -L`).
//!
//! One SSH session per connection profile is shared by all forwards through
//! that host. Each forward listens on `127.0.0.1` (never other interfaces)
//! and opens a `direct-tcpip` channel per accepted connection, counting
//! bytes both ways. Background forwards cannot ask about host keys, so an
//! untrusted host fails with [`TunnelError::UnknownHostKey`] until the user
//! has connected once in the SSH tool.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use hs_core::connections::Connections;
use hs_core::runtime::AppRuntime;
use hs_core::secrets::SecretStore;
use hs_core::tunnels::{ForwardResult, ForwardSpec, TunnelError, TunnelInfo, TunnelState, Tunnels};
use parking_lot::{Mutex, RwLock};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

use crate::config::{ConfigError, config_from_fields};
use crate::{KnownHosts, SshClient, SshError, SshFields};

#[derive(Default)]
struct Counters {
    open: AtomicU32,
    total: AtomicU64,
    up: AtomicU64,
    down: AtomicU64,
}

struct Entry {
    id: u64,
    spec: ForwardSpec,
    via: String,
    state: TunnelState,
    counters: Arc<Counters>,
    cancel: CancellationToken,
    waiters: Vec<oneshot::Sender<ForwardResult>>,
}

#[derive(Default)]
struct State {
    next_id: u64,
    entries: Vec<Entry>,
    clients: HashMap<String, Arc<SshClient>>,
}

struct Inner {
    runtime: AppRuntime,
    connections: Arc<RwLock<Connections>>,
    secrets: Arc<dyn SecretStore>,
    /// Re-read at every connect: the SSH tool may have trusted a key since.
    known_hosts: PathBuf,
    state: Mutex<State>,
}

pub struct TunnelManager {
    inner: Arc<Inner>,
}

impl TunnelManager {
    pub fn new(
        runtime: AppRuntime,
        connections: Arc<RwLock<Connections>>,
        secrets: Arc<dyn SecretStore>,
        known_hosts: PathBuf,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                runtime,
                connections,
                secrets,
                known_hosts,
                state: Mutex::new(State::default()),
            }),
        }
    }

    fn start(&self, spec: ForwardSpec, waiter: Option<oneshot::Sender<ForwardResult>>) -> u64 {
        let via = self
            .inner
            .connections
            .read()
            .by_id(&spec.ssh_id)
            .map(|c| c.name.clone())
            .unwrap_or_else(|| spec.ssh_id.clone());
        let counters = Arc::new(Counters::default());
        let cancel = CancellationToken::new();
        let id = {
            let mut st = self.inner.state.lock();
            st.next_id += 1;
            let id = st.next_id;
            st.entries.push(Entry {
                id,
                spec: spec.clone(),
                via,
                state: TunnelState::Starting,
                counters: counters.clone(),
                cancel: cancel.clone(),
                waiters: waiter.into_iter().collect(),
            });
            id
        };
        let inner = self.inner.clone();
        self.inner.runtime.handle().spawn(async move {
            tokio::select! {
                _ = cancel.cancelled() => {}
                _ = run(inner.clone(), id, spec, counters, cancel.clone()) => {}
            }
        });
        self.inner.runtime.waker().request_repaint();
        id
    }
}

impl Inner {
    fn set_state(&self, id: u64, state: TunnelState) {
        let waiters = {
            let mut st = self.state.lock();
            let Some(e) = st.entries.iter_mut().find(|e| e.id == id) else {
                return;
            };
            e.state = state.clone();
            std::mem::take(&mut e.waiters)
        };
        let reply = match &state {
            TunnelState::Listening { local_port } => Some(Ok(*local_port)),
            TunnelState::Failed(e) => Some(Err(e.clone())),
            TunnelState::Starting => None,
        };
        if let Some(reply) = reply {
            for w in waiters {
                let _ = w.send(reply.clone());
            }
        }
        self.runtime.waker().request_repaint();
    }

    /// The shared session for `ssh_id`, connecting when there is none.
    async fn client(&self, ssh_id: &str) -> Result<Arc<SshClient>, TunnelError> {
        if let Some(c) = self.state.lock().clients.get(ssh_id)
            && !c.is_closed()
        {
            return Ok(c.clone());
        }
        let fields = {
            let conns = self.connections.read();
            let c = conns
                .by_id(ssh_id)
                .filter(|c| c.kind == "ssh")
                .ok_or(TunnelError::NoProfile)?;
            SshFields::from_table(&c.fields)
        };
        let config = config_from_fields(&fields, self.secrets.as_ref()).map_err(|e| match e {
            ConfigError::VaultLocked => TunnelError::VaultLocked,
            ConfigError::NoPassword => TunnelError::NoPassword,
            ConfigError::Secret(s) => TunnelError::Other(s),
        })?;
        let client =
            SshClient::connect(config, Arc::new(KnownHosts::load(&self.known_hosts)), None)
                .await
                .map_err(|e| match e {
                    SshError::HostKeyRejected { .. } => TunnelError::UnknownHostKey,
                    other => TunnelError::Other(other.to_string()),
                })?;
        let client = Arc::new(client);
        self.state
            .lock()
            .clients
            .insert(ssh_id.to_string(), client.clone());
        Ok(client)
    }
}

async fn run(
    inner: Arc<Inner>,
    id: u64,
    spec: ForwardSpec,
    counters: Arc<Counters>,
    cancel: CancellationToken,
) {
    let client = match inner.client(&spec.ssh_id).await {
        Ok(c) => c,
        Err(e) => return inner.set_state(id, TunnelState::Failed(e)),
    };
    let listener = match TcpListener::bind(("127.0.0.1", spec.local_port)).await {
        Ok(l) => l,
        Err(e) => {
            return inner.set_state(
                id,
                TunnelState::Failed(TunnelError::Other(format!(
                    "cannot listen on 127.0.0.1:{}: {e}",
                    spec.local_port
                ))),
            );
        }
    };
    let local_port = listener.local_addr().map(|a| a.port()).unwrap_or(0);
    inner.set_state(id, TunnelState::Listening { local_port });
    let mut client = client;
    loop {
        let Ok((socket, peer)) = listener.accept().await else {
            continue;
        };
        let _ = socket.set_nodelay(true);
        // The shared session may have dropped (server restart, network).
        if client.is_closed() {
            match inner.client(&spec.ssh_id).await {
                Ok(c) => client = c,
                Err(e) => {
                    tracing::warn!("tunnel {id}: reconnect failed: {e}");
                    continue;
                }
            }
        }
        let channel = match client
            .open_direct_tcpip(
                &spec.remote_host,
                spec.remote_port,
                &peer.ip().to_string(),
                peer.port(),
            )
            .await
        {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(
                    "tunnel {id}: {}:{} refused: {e}",
                    spec.remote_host,
                    spec.remote_port
                );
                continue;
            }
        };
        counters.open.fetch_add(1, Ordering::Relaxed);
        counters.total.fetch_add(1, Ordering::Relaxed);
        let counters = counters.clone();
        let cancel = cancel.clone();
        let waker = inner.runtime.waker();
        tokio::spawn(async move {
            let (mut cr, mut cw) = tokio::io::split(channel);
            let (mut sr, mut sw) = socket.into_split();
            tokio::select! {
                _ = cancel.cancelled() => {}
                _ = async {
                    tokio::join!(
                        pump(&mut sr, &mut cw, &counters.up),
                        pump(&mut cr, &mut sw, &counters.down),
                    )
                } => {}
            }
            counters.open.fetch_sub(1, Ordering::Relaxed);
            waker.request_repaint();
        });
        inner.runtime.waker().request_repaint();
    }
}

/// Copy until EOF or an error, counting bytes, then close the write side.
async fn pump<R, W>(r: &mut R, w: &mut W, count: &AtomicU64)
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut buf = vec![0u8; 32 * 1024];
    loop {
        match r.read(&mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                if w.write_all(&buf[..n]).await.is_err() {
                    break;
                }
                count.fetch_add(n as u64, Ordering::Relaxed);
            }
        }
    }
    let _ = w.shutdown().await;
}

impl Tunnels for TunnelManager {
    fn ensure_local_forward(
        &self,
        ssh_id: &str,
        remote_host: &str,
        remote_port: u16,
    ) -> oneshot::Receiver<ForwardResult> {
        let (tx, rx) = oneshot::channel();
        {
            let mut st = self.inner.state.lock();
            let same = |e: &Entry| {
                e.spec.ssh_id == ssh_id
                    && e.spec.remote_host == remote_host
                    && e.spec.remote_port == remote_port
            };
            // A failed attempt is retried; a live or starting one is shared.
            st.entries
                .retain(|e| !(same(e) && matches!(e.state, TunnelState::Failed(_))));
            if let Some(e) = st.entries.iter_mut().find(|e| same(e)) {
                match e.state {
                    TunnelState::Listening { local_port } => {
                        let _ = tx.send(Ok(local_port));
                    }
                    _ => e.waiters.push(tx),
                }
                return rx;
            }
        }
        self.start(
            ForwardSpec {
                ssh_id: ssh_id.to_string(),
                remote_host: remote_host.to_string(),
                remote_port,
                local_port: 0,
            },
            Some(tx),
        );
        rx
    }

    fn open(&self, spec: ForwardSpec) -> u64 {
        self.start(spec, None)
    }

    fn stop(&self, id: u64) {
        let (entry, unused_client) = {
            let mut st = self.inner.state.lock();
            let Some(pos) = st.entries.iter().position(|e| e.id == id) else {
                return;
            };
            let e = st.entries.remove(pos);
            // Close the SSH session when no other forward uses it.
            let still_used = st.entries.iter().any(|o| o.spec.ssh_id == e.spec.ssh_id);
            let client = (!still_used)
                .then(|| st.clients.remove(&e.spec.ssh_id))
                .flatten();
            (e, client)
        };
        entry.cancel.cancel();
        for w in entry.waiters {
            let _ = w.send(Err(TunnelError::Other("stopped".into())));
        }
        if let Some(c) = unused_client {
            self.inner.runtime.handle().spawn(async move {
                c.disconnect().await;
            });
        }
        self.inner.runtime.waker().request_repaint();
    }

    fn list(&self) -> Vec<TunnelInfo> {
        self.inner
            .state
            .lock()
            .entries
            .iter()
            .map(|e| TunnelInfo {
                id: e.id,
                spec: e.spec.clone(),
                via: e.via.clone(),
                state: e.state.clone(),
                open: e.counters.open.load(Ordering::Relaxed),
                total: e.counters.total.load(Ordering::Relaxed),
                bytes_up: e.counters.up.load(Ordering::Relaxed),
                bytes_down: e.counters.down.load(Ordering::Relaxed),
            })
            .collect()
    }
}
