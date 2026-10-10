//! Local PTY: spawn a shell profile and hand back reader, writer and resize.
//!
//! ConPTY gotcha (Windows): the pseudo console sends a Device Status Report
//! request (`ESC [ 6 n`) at startup and stalls the child until the "terminal"
//! answers with a cursor position. The session answers it through the
//! `alacritty_terminal` handler (`Event::PtyWrite`); [`run_capture`] answers
//! it by hand because it has no terminal.

use std::io::{Read, Write};
use std::path::Path;
use std::time::{Duration, Instant};

use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};

use crate::shells::ShellProfile;

const DSR_REQUEST: &[u8] = b"\x1b[6n";
const DSR_REPLY: &[u8] = b"\x1b[1;1R";

/// A spawned local shell.
pub struct Pty {
    master: Option<Box<dyn MasterPty + Send>>,
    child: Box<dyn Child + Send + Sync>,
    writer: Option<Box<dyn Write + Send>>,
}

impl Pty {
    pub fn spawn(
        profile: &ShellProfile,
        cwd: Option<&Path>,
        cols: u16,
        rows: u16,
    ) -> Result<(Self, Box<dyn Read + Send>), String> {
        let pty = native_pty_system();
        let pair = pty
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| e.to_string())?;
        let mut cmd = CommandBuilder::new(&profile.exe);
        cmd.args(&profile.args);
        for (k, v) in &profile.env {
            cmd.env(k, v);
        }
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        cmd.env("TERM_PROGRAM", "HyperStack");
        if let Some(d) = cwd.filter(|d| d.is_dir()) {
            cmd.cwd(d);
        }
        let child = pair.slave.spawn_command(cmd).map_err(|e| e.to_string())?;
        drop(pair.slave);
        let reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
        let writer = pair.master.take_writer().map_err(|e| e.to_string())?;
        Ok((
            Self {
                master: Some(pair.master),
                child,
                writer: Some(writer),
            },
            reader,
        ))
    }

    pub fn write(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        match self.writer.as_mut() {
            Some(w) => {
                w.write_all(bytes)?;
                w.flush()
            }
            None => Err(std::io::Error::other("pty closed")),
        }
    }

    pub fn resize(&self, cols: u16, rows: u16) {
        if let Some(m) = &self.master {
            let _ = m.resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            });
        }
    }

    /// Drop the master side so a blocked reader returns. On ConPTY the read
    /// never returns EOF by itself after the child exits.
    pub fn close(&mut self) {
        self.writer = None;
        self.master = None;
    }

    /// Exit code when the child has finished.
    pub fn try_wait(&mut self) -> Option<u32> {
        match self.child.try_wait() {
            Ok(Some(status)) => Some(status.exit_code()),
            _ => None,
        }
    }

    pub fn kill(&mut self) {
        let _ = self.child.kill();
    }

    pub fn process_id(&self) -> Option<u32> {
        self.child.process_id()
    }
}

/// Spawn `cmd args` in a PTY and return everything it printed within `timeout`.
pub fn run_capture(cmd: &str, args: &[&str], timeout: Duration) -> Result<String, String> {
    let pty = native_pty_system();
    let pair = pty
        .openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| e.to_string())?;
    let mut builder = CommandBuilder::new(cmd);
    builder.args(args);
    let mut child = pair
        .slave
        .spawn_command(builder)
        .map_err(|e| e.to_string())?;
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
    let mut writer = pair.master.take_writer().map_err(|e| e.to_string())?;
    let (tx, rx) = std::sync::mpsc::channel::<Vec<u8>>();
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 || tx.send(buf[..n].to_vec()).is_err() {
                break;
            }
        }
    });
    let start = Instant::now();
    let mut out = Vec::new();
    let mut answered_dsr = false;
    loop {
        while let Ok(chunk) = rx.try_recv() {
            out.extend_from_slice(&chunk);
        }
        if !answered_dsr && out.windows(DSR_REQUEST.len()).any(|w| w == DSR_REQUEST) {
            writer
                .write_all(DSR_REPLY)
                .and_then(|()| writer.flush())
                .map_err(|e| e.to_string())?;
            answered_dsr = true;
        }
        if let Ok(Some(_)) = child.try_wait() {
            while let Ok(chunk) = rx.recv_timeout(Duration::from_millis(200)) {
                out.extend_from_slice(&chunk);
            }
            break;
        }
        if start.elapsed() > timeout {
            let _ = child.kill();
            return Err(format!(
                "timed out after {timeout:?}; got {} bytes",
                out.len()
            ));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    Ok(String::from_utf8_lossy(&out).to_string())
}

/// The shell and arguments that print `HS_PTY_OK` on this platform.
pub fn probe_command() -> (&'static str, Vec<&'static str>) {
    if cfg!(windows) {
        ("cmd.exe", vec!["/d", "/c", "echo HS_PTY_OK"])
    } else {
        ("sh", vec!["-c", "echo HS_PTY_OK"])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pty_spawn_echo_round_trip() {
        let (cmd, args) = probe_command();
        let out = run_capture(cmd, &args, Duration::from_secs(10)).unwrap();
        assert!(out.contains("HS_PTY_OK"), "unexpected PTY output: {out:?}");
    }
}
