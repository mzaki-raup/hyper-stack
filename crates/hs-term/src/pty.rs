//! Minimal PTY spawn used by the Phase 0 verification test and `--self-test`.
//!
//! ConPTY gotcha (Windows): the pseudo console sends a Device Status Report
//! request (`ESC [ 6 n`) and stalls the child until the "terminal" answers
//! with a cursor position (`ESC [ row ; col R`). A real emulator does that
//! automatically; this helper does it by hand. The full terminal in M1 lets
//! `alacritty_terminal` answer such queries.

use std::io::{Read, Write};
use std::time::{Duration, Instant};

use portable_pty::{CommandBuilder, PtySize, native_pty_system};

const DSR_REQUEST: &[u8] = b"\x1b[6n";
const DSR_REPLY: &[u8] = b"\x1b[1;1R";

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
            // Drain what is left after exit.
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
