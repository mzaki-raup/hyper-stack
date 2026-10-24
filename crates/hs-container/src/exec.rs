//! A pod's shell as an `hs_term` transport (`docs/tools/containers.md`
//! §10.6, phase 7).
//!
//! The same arrangement `hs-ssh` uses for a shell channel: a tokio task
//! owns the connection, pushes its output into a blocking reader the
//! terminal can read, and applies the writes, resizes and close
//! requests the UI thread sends. The Terminal activity therefore runs a
//! pod's shell with no idea that Kubernetes is involved - exactly as it
//! runs an SSH shell.
//!
//! There is no `kubectl exec` behind this. The API server's `exec`
//! subresource is a WebSocket, which is why `kube` is built with its
//! `ws` feature.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use futures_util::SinkExt;
use k8s_openapi::api::core::v1::Pod;
use kube::Client;
use kube::api::{Api, AttachParams, TerminalSize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::k8s::{Error, Result};

pub type ExecReader = Box<dyn std::io::Read + Send>;

enum Cmd {
    Data(Vec<u8>),
    Resize(u16, u16),
    Close,
}

/// The writing half, as `hs_term` wants it.
pub struct ExecTransport {
    tx: tokio::sync::mpsc::UnboundedSender<Cmd>,
    exited: Arc<AtomicBool>,
    exit_code: Arc<AtomicU32>,
}

impl hs_term::transport::Transport for ExecTransport {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        self.tx
            .send(Cmd::Data(bytes.to_vec()))
            .map_err(|_| std::io::Error::other("the exec session has ended"))
    }
    fn resize(&mut self, cols: u16, rows: u16) {
        let _ = self.tx.send(Cmd::Resize(cols, rows));
    }
    fn try_wait(&mut self) -> Option<u32> {
        self.exited
            .load(Ordering::Relaxed)
            .then(|| self.exit_code.load(Ordering::Relaxed))
    }
    fn kill(&mut self) {
        let _ = self.tx.send(Cmd::Close);
    }
    fn close(&mut self) {
        let _ = self.tx.send(Cmd::Close);
    }
}

/// Start a shell inside a pod's container.
///
/// `shell` is the command line to run, which is the user's to choose
/// (Settings → Containers → Shell for Exec): a minimal image has no
/// bash and a distroless one has no shell at all, and a wrong answer
/// must be visible and editable rather than guessed at.
///
/// `container` may be empty for a single-container pod; the API
/// requires it the moment there are two.
pub async fn exec(
    client: &Client,
    ns: &str,
    pod: &str,
    container: &str,
    shell: &[String],
) -> Result<(ExecTransport, ExecReader)> {
    let api: Api<Pod> = if ns.trim().is_empty() {
        Api::default_namespaced(client.clone())
    } else {
        Api::namespaced(client.clone(), ns.trim())
    };
    // A TTY, because this is a terminal: the shell then line-edits,
    // colours its output and sends its own control sequences, and
    // stderr arrives merged into stdout as a terminal expects.
    let mut params = AttachParams::interactive_tty();
    if !container.trim().is_empty() {
        params = params.container(container.trim().to_string());
    }
    let mut process = api
        .exec(pod, shell.iter().map(String::as_str), &params)
        .await
        .map_err(crate::k8s::classify)?;
    let mut stdin = process
        .stdin()
        .ok_or_else(|| Error::Refused("the cluster opened no standard input".into()))?;
    let mut stdout = process
        .stdout()
        .ok_or_else(|| Error::Refused("the cluster opened no standard output".into()))?;
    let mut sizes = process.terminal_size();
    let status = process.take_status();

    let (out_tx, out_rx) = std::sync::mpsc::channel::<Vec<u8>>();
    let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::unbounded_channel::<Cmd>();
    let exited = Arc::new(AtomicBool::new(false));
    let exit_code = Arc::new(AtomicU32::new(0));
    let transport = ExecTransport {
        tx: cmd_tx,
        exited: exited.clone(),
        exit_code: exit_code.clone(),
    };

    // The status future carries the exit code in the way the API
    // reports one: a `Status` object with the code among its details
    // rather than a number.
    //
    // **A status of `None` is not an exit.** It means the channel ended
    // without the cluster saying anything, which is what happens with a
    // TTY session - and treating it as an exit closed the shell the
    // instant it opened, with "exited with code 0" and no output
    // (measured against k3s 1.34, 2026-10-25). The pump below decides
    // when there is no status: end of output is end of session.
    let (done, done_code) = (exited.clone(), exit_code.clone());
    if let Some(status) = status {
        tokio::spawn(async move {
            if let Some(status) = status.await {
                done_code.store(exit_code_of(status), Ordering::Relaxed);
                done.store(true, Ordering::Relaxed);
            }
        });
    }

    tokio::spawn(async move {
        // **`process` is moved in here on purpose.** It owns the task
        // driving the WebSocket, so dropping it when `exec` returns
        // ends the session the moment it starts: the streams answer
        // EOF, the pump stops, and the tab reads "the shell exited with
        // code 0" with nothing in it (measured against k3s 1.34,
        // 2026-10-25). It lives exactly as long as the pump does.
        let process = process;
        let mut buf = vec![0u8; 8192];
        loop {
            tokio::select! {
                read = stdout.read(&mut buf) => {
                    match read {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            if out_tx.send(buf[..n].to_vec()).is_err() {
                                break;
                            }
                        }
                    }
                }
                cmd = cmd_rx.recv() => {
                    match cmd {
                        Some(Cmd::Data(bytes)) => {
                            if stdin.write_all(&bytes).await.is_err() {
                                break;
                            }
                            let _ = stdin.flush().await;
                        }
                        Some(Cmd::Resize(cols, rows)) => {
                            if let Some(sizes) = sizes.as_mut() {
                                let _ = sizes
                                    .send(TerminalSize {
                                        width: cols,
                                        height: rows,
                                    })
                                    .await;
                            }
                        }
                        Some(Cmd::Close) | None => break,
                    }
                }
            }
        }
        let _ = stdin.shutdown().await;
        // Let the connection finish tidily rather than being cut off
        // by the drop at the end of this task.
        let _ = process.join().await;
        exited.store(true, Ordering::Relaxed);
        // Dropping `out_tx` ends the reader with EOF.
    });

    Ok((
        transport,
        Box::new(hs_term::transport::ChannelReader::new(out_rx)),
    ))
}

/// The exit code hidden in a `Status`.
///
/// Kubernetes reports the command's exit status as a `Status` object:
/// `Success` for zero, and for anything else a `Failure` whose causes
/// carry an `ExitCode`. A failure with no code at all still means
/// "not zero", so it reads as 1 rather than as success.
fn exit_code_of(status: k8s_openapi::apimachinery::pkg::apis::meta::v1::Status) -> u32 {
    if status.status.as_deref() == Some("Success") {
        return 0;
    }
    status
        .details
        .as_ref()
        .and_then(|d| d.causes.as_ref())
        .and_then(|causes| {
            causes
                .iter()
                .find(|c| c.reason.as_deref() == Some("ExitCode"))
                .and_then(|c| c.message.as_ref())
                .and_then(|m| m.parse::<u32>().ok())
        })
        .unwrap_or(1)
}
