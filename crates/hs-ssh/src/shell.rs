//! A shell channel as an `hs_term` transport: a tokio task owns the channel,
//! forwards its output to a blocking reader and applies writes, resizes and
//! close requests sent from the UI thread.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use russh::ChannelMsg;
use russh::client::Msg;
use tokio::sync::mpsc;

pub type ShellReader = Box<dyn std::io::Read + Send>;

enum Cmd {
    Data(Vec<u8>),
    Resize(u16, u16),
    Close,
}

pub struct ShellTransport {
    tx: mpsc::UnboundedSender<Cmd>,
    exited: Arc<AtomicBool>,
    exit_code: Arc<AtomicU32>,
}

impl hs_term::transport::Transport for ShellTransport {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        self.tx
            .send(Cmd::Data(bytes.to_vec()))
            .map_err(|_| std::io::Error::other("ssh channel closed"))
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

/// Start the pump task for an opened shell channel. Must be called on the
/// tokio runtime.
pub fn spawn(mut channel: russh::Channel<Msg>) -> (ShellTransport, ShellReader) {
    let (out_tx, out_rx) = std::sync::mpsc::channel::<Vec<u8>>();
    let (cmd_tx, mut cmd_rx) = mpsc::unbounded_channel::<Cmd>();
    let exited = Arc::new(AtomicBool::new(false));
    let exit_code = Arc::new(AtomicU32::new(0));
    let transport = ShellTransport {
        tx: cmd_tx,
        exited: exited.clone(),
        exit_code: exit_code.clone(),
    };
    tokio::spawn(async move {
        loop {
            tokio::select! {
                msg = channel.wait() => {
                    match msg {
                        Some(ChannelMsg::Data { data }) => {
                            if out_tx.send(data.to_vec()).is_err() {
                                break;
                            }
                        }
                        Some(ChannelMsg::ExtendedData { data, .. }) => {
                            if out_tx.send(data.to_vec()).is_err() {
                                break;
                            }
                        }
                        Some(ChannelMsg::ExitStatus { exit_status }) => {
                            exit_code.store(exit_status, Ordering::Relaxed);
                        }
                        Some(ChannelMsg::Eof) | Some(ChannelMsg::Close) | None => break,
                        Some(_) => {}
                    }
                }
                cmd = cmd_rx.recv() => {
                    match cmd {
                        Some(Cmd::Data(bytes)) => {
                            if channel.data(&bytes[..]).await.is_err() {
                                break;
                            }
                        }
                        Some(Cmd::Resize(cols, rows)) => {
                            let _ = channel.window_change(cols as u32, rows as u32, 0, 0).await;
                        }
                        Some(Cmd::Close) | None => {
                            let _ = channel.eof().await;
                            let _ = channel.close().await;
                            break;
                        }
                    }
                }
            }
        }
        exited.store(true, Ordering::Relaxed);
        // Dropping out_tx ends the reader with EOF.
    });
    (
        transport,
        Box::new(hs_term::transport::ChannelReader::new(out_rx)),
    )
}
