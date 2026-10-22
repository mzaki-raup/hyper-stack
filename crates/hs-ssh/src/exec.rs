//! A remote command on an exec channel with its output streamed as it
//! arrives: what a `tail -F` or a `journalctl -f` needs. The channel is
//! pumped by a task on the runtime; the reader side is a tokio channel of
//! byte chunks (stdout and stderr apart), ended by the command's exit.

use russh::ChannelMsg;
use russh::client::Msg;
use tokio::sync::mpsc;

/// One piece of a running command's output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecChunk {
    Stdout(Vec<u8>),
    Stderr(Vec<u8>),
    /// The command ended with this status; nothing follows.
    Exit(u32),
}

/// A running command: read chunks, drop to stop it.
pub struct ExecStream {
    rx: mpsc::Receiver<ExecChunk>,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
}

impl ExecStream {
    /// The next chunk, `None` once the channel is closed.
    pub async fn next(&mut self) -> Option<ExecChunk> {
        self.rx.recv().await
    }

    /// Stop the command (EOF + close on the channel).
    pub fn stop(&mut self) {
        if let Some(s) = self.stop.take() {
            let _ = s.send(());
        }
    }
}

impl Drop for ExecStream {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Start the pump task for an exec channel. Must be called on the runtime.
pub fn spawn(mut channel: russh::Channel<Msg>) -> ExecStream {
    let (tx, rx) = mpsc::channel::<ExecChunk>(256);
    let (stop_tx, mut stop_rx) = tokio::sync::oneshot::channel::<()>();
    tokio::spawn(async move {
        let mut exit = 0u32;
        loop {
            tokio::select! {
                msg = channel.wait() => {
                    match msg {
                        Some(ChannelMsg::Data { data }) => {
                            if tx.send(ExecChunk::Stdout(data.to_vec())).await.is_err() {
                                break;
                            }
                        }
                        Some(ChannelMsg::ExtendedData { data, .. }) => {
                            if tx.send(ExecChunk::Stderr(data.to_vec())).await.is_err() {
                                break;
                            }
                        }
                        Some(ChannelMsg::ExitStatus { exit_status }) => exit = exit_status,
                        Some(ChannelMsg::Eof) | Some(ChannelMsg::Close) | None => break,
                        Some(_) => {}
                    }
                }
                _ = &mut stop_rx => {
                    let _ = channel.eof().await;
                    let _ = channel.close().await;
                    break;
                }
            }
        }
        let _ = tx.send(ExecChunk::Exit(exit)).await;
    });
    ExecStream {
        rx,
        stop: Some(stop_tx),
    }
}
