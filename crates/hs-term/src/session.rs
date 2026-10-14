//! A terminal session: `alacritty_terminal::Term` driven by a transport.
//! The reader thread parses bytes into the grid under a `FairMutex`; the UI
//! locks the same mutex briefly to render.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::term::{Config, Term};
use alacritty_terminal::vte::ansi::Processor;
use parking_lot::Mutex;

use crate::pty::Pty;
use crate::shells::ShellProfile;
use crate::transport::Transport;

/// Events the UI reacts to between frames.
#[derive(Debug, Clone)]
pub enum TermEvent {
    Title(String),
    Bell,
    Exit,
    ClipboardStore(String),
}

/// Receives events from the grid: answers queries that need a reply, queues
/// the rest for the UI, and wakes the UI.
#[derive(Clone)]
pub struct Listener {
    events: Arc<Mutex<Vec<TermEvent>>>,
    pty_writes: Arc<Mutex<Vec<Vec<u8>>>>,
    waker: Arc<dyn Fn() + Send + Sync>,
}

impl EventListener for Listener {
    fn send_event(&self, event: Event) {
        match event {
            Event::Title(t) => self.events.lock().push(TermEvent::Title(t)),
            Event::ResetTitle => self.events.lock().push(TermEvent::Title(String::new())),
            Event::Bell => self.events.lock().push(TermEvent::Bell),
            Event::Exit => self.events.lock().push(TermEvent::Exit),
            Event::ClipboardStore(_, text) => {
                self.events.lock().push(TermEvent::ClipboardStore(text))
            }
            // DSR, DA and friends: the grid wants to answer the application.
            Event::PtyWrite(text) => self.pty_writes.lock().push(text.into_bytes()),
            Event::ColorRequest(index, format) => {
                // Answer with the default palette entry so OSC 4/10/11 queries do not hang.
                let rgb = crate::widget::default_palette_rgb(index);
                let reply = format(rgb);
                self.pty_writes.lock().push(reply.into_bytes());
            }
            Event::Wakeup | Event::MouseCursorDirty | Event::CursorBlinkingChange => {}
            _ => {}
        }
        (self.waker)();
    }
}

pub type SharedTerm = Arc<FairMutex<Term<Listener>>>;

pub struct Session {
    pub term: SharedTerm,
    listener: Listener,
    pty: Arc<Mutex<Box<dyn Transport>>>,
    exited: Arc<AtomicBool>,
    exit_code: Arc<AtomicU32>,
    pub profile: ShellProfile,
    pub title: String,
    cols: u16,
    rows: u16,
}

impl Session {
    /// Spawn the profile's shell and start parsing its output.
    pub fn spawn(
        profile: ShellProfile,
        cwd: Option<&Path>,
        cols: u16,
        rows: u16,
        scrollback: usize,
        waker: Arc<dyn Fn() + Send + Sync>,
    ) -> Result<Self, String> {
        let (pty, reader) = Pty::spawn(&profile, cwd, cols, rows)?;
        Self::with_transport(
            profile,
            Box::new(pty),
            reader,
            cols,
            rows,
            scrollback,
            waker,
        )
    }

    /// Drive the grid from any transport (an SSH channel, a serial port, …).
    /// `profile` only supplies the default title and id.
    pub fn with_transport(
        profile: ShellProfile,
        transport: Box<dyn Transport>,
        mut reader: Box<dyn std::io::Read + Send>,
        cols: u16,
        rows: u16,
        scrollback: usize,
        waker: Arc<dyn Fn() + Send + Sync>,
    ) -> Result<Self, String> {
        let pty = transport;
        let listener = Listener {
            events: Arc::new(Mutex::new(Vec::new())),
            pty_writes: Arc::new(Mutex::new(Vec::new())),
            waker,
        };
        let config = Config {
            scrolling_history: scrollback,
            ..Config::default()
        };
        let term = Term::new(
            config,
            &TermSize::new(cols as usize, rows as usize),
            listener.clone(),
        );
        let term = Arc::new(FairMutex::new(term));
        let pty = Arc::new(Mutex::new(pty));
        let exited = Arc::new(AtomicBool::new(false));
        let exit_code = Arc::new(AtomicU32::new(0));

        // Reader thread: bytes → parser → grid. Replies the grid wants to send
        // (DSR etc.) are flushed right after each chunk.
        {
            let term = term.clone();
            let listener = listener.clone();
            let pty = pty.clone();
            let exited = exited.clone();
            let exit_code = exit_code.clone();
            std::thread::Builder::new()
                .name("hs-term-reader".into())
                .spawn(move || {
                    let mut processor: Processor<alacritty_terminal::vte::ansi::StdSyncHandler> =
                        Processor::new();
                    let mut buf = [0u8; 8192];
                    loop {
                        match reader.read(&mut buf) {
                            Ok(0) | Err(_) => break,
                            Ok(n) => {
                                {
                                    let mut t = term.lock();
                                    processor.advance(&mut *t, &buf[..n]);
                                }
                                let replies: Vec<Vec<u8>> =
                                    std::mem::take(&mut *listener.pty_writes.lock());
                                if !replies.is_empty() {
                                    let mut p = pty.lock();
                                    for r in replies {
                                        let _ = p.write(&r);
                                    }
                                }
                                (listener.waker)();
                            }
                        }
                    }
                    if !exited.load(Ordering::Relaxed) {
                        let code = pty.lock().try_wait().unwrap_or(0);
                        exit_code.store(code, Ordering::Relaxed);
                        exited.store(true, Ordering::Relaxed);
                        listener.events.lock().push(TermEvent::Exit);
                        (listener.waker)();
                    }
                })
                .map_err(|e| e.to_string())?;
        }
        // Waiter thread: ConPTY does not deliver EOF when the child exits, so
        // poll the child and close the PTY to unblock the reader.
        {
            let listener = listener.clone();
            let pty = pty.clone();
            let exited = exited.clone();
            let exit_code = exit_code.clone();
            std::thread::Builder::new()
                .name("hs-term-waiter".into())
                .spawn(move || {
                    loop {
                        if exited.load(Ordering::Relaxed) {
                            break;
                        }
                        let code = pty.lock().try_wait();
                        if let Some(code) = code {
                            exit_code.store(code, Ordering::Relaxed);
                            exited.store(true, Ordering::Relaxed);
                            pty.lock().close();
                            listener.events.lock().push(TermEvent::Exit);
                            (listener.waker)();
                            break;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(100));
                    }
                })
                .map_err(|e| e.to_string())?;
        }
        let title = profile.name.clone();
        Ok(Self {
            term,
            listener,
            pty,
            exited,
            exit_code,
            profile,
            title,
            cols,
            rows,
        })
    }

    pub fn write(&self, bytes: &[u8]) {
        if !self.exited.load(Ordering::Relaxed) {
            let _ = self.pty.lock().write(bytes);
        }
    }

    pub fn resize(&mut self, cols: u16, rows: u16) {
        if cols == self.cols && rows == self.rows || cols < 2 || rows < 1 {
            return;
        }
        self.cols = cols;
        self.rows = rows;
        self.term
            .lock()
            .resize(TermSize::new(cols as usize, rows as usize));
        self.pty.lock().resize(cols, rows);
    }

    pub fn size(&self) -> (u16, u16) {
        (self.cols, self.rows)
    }

    /// Plain-text view of visible terminal rows for an explicitly requested
    /// assistant context. Control sequences and styling are omitted.
    pub fn visible_text(&self, max_rows: usize) -> String {
        let term = self.term.lock();
        let grid = term.grid();
        let rows = (0..self.rows as usize).take(max_rows);
        rows.map(|row| {
            let line = alacritty_terminal::index::Line(row as i32);
            (0..self.cols as usize)
                .map(|col| {
                    grid[alacritty_terminal::index::Point::new(
                        line,
                        alacritty_terminal::index::Column(col),
                    )]
                    .c
                })
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
    }

    pub fn exited(&self) -> Option<u32> {
        self.exited
            .load(Ordering::Relaxed)
            .then(|| self.exit_code.load(Ordering::Relaxed))
    }

    pub fn kill(&self) {
        self.pty.lock().kill();
    }

    /// Events since the last call; titles are applied to `self.title`.
    pub fn drain_events(&mut self) -> Vec<TermEvent> {
        let events = std::mem::take(&mut *self.listener.events.lock());
        for e in &events {
            if let TermEvent::Title(t) = e {
                self.title = if t.is_empty() {
                    self.profile.name.clone()
                } else {
                    t.clone()
                };
            }
        }
        events
    }

    pub fn app_cursor(&self) -> bool {
        self.term
            .lock()
            .mode()
            .contains(alacritty_terminal::term::TermMode::APP_CURSOR)
    }

    pub fn bracketed_paste(&self) -> bool {
        self.term
            .lock()
            .mode()
            .contains(alacritty_terminal::term::TermMode::BRACKETED_PASTE)
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.pty.lock().kill();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn a_shell_session_echoes_into_the_grid_and_exits() {
        let (cmd, args) = crate::pty::probe_command();
        let profile = ShellProfile {
            id: "test".into(),
            name: "test".into(),
            icon: String::new(),
            exe: cmd.into(),
            args: args.iter().map(|a| a.to_string()).collect(),
            env: Vec::new(),
        };
        let mut s = Session::spawn(profile, None, 80, 24, 100, Arc::new(|| {})).unwrap();
        let start = Instant::now();
        while s.exited().is_none() {
            assert!(
                start.elapsed() < Duration::from_secs(15),
                "shell did not exit"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        let text = {
            let t = s.term.lock();
            let content = t.renderable_content();
            let mut out = String::new();
            let mut last_line = None;
            for cell in content.display_iter {
                if last_line != Some(cell.point.line) {
                    out.push('\n');
                    last_line = Some(cell.point.line);
                }
                out.push(cell.c);
            }
            out
        };
        assert!(
            text.contains("HS_PTY_OK"),
            "grid did not contain the echo: {text:?}"
        );
        let _ = s.drain_events();
    }
}
