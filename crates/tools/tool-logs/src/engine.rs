//! The reading side (docs/tools/logs.md §5): a task per open source feeds
//! parsed records into [`Shared`], which the UI reads under a short lock.
//! A local file is followed like `tail -F` (appends, truncation and
//! rotation by size shrink or inode change); an SSH file through `tail -F`
//! on an exec channel; a command through its stdout; pasted text once.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use hs_core::Services;
use hs_ssh::config::{ConfigError, config_from_fields};
use hs_ssh::{ExecChunk, KnownHosts, SshClient, SshFields};
use parking_lot::Mutex;

use crate::parse::{self, Format, Record};
use crate::source::SourceSpec;

/// How many records a tab keeps; older ones are dropped from the front.
pub const WINDOW: usize = 200_000;
/// How much of a large local file is read on open (the tail of it).
pub const OPEN_BYTES: u64 = 64 * 1024 * 1024;
/// How often a local file is checked for growth.
const POLL: Duration = Duration::from_millis(400);

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Status {
    #[default]
    Idle,
    Connecting,
    Reading,
    /// Live: new lines arrive as they are written.
    Following,
    /// The source ended (a command exited, pasted text).
    Done,
    Failed(String),
}

/// What the UI reads: the record window and the counters around it.
#[derive(Default)]
pub struct Shared {
    pub records: Vec<Record>,
    /// Records dropped from the front to keep the window bounded.
    pub dropped: u64,
    /// Lines read so far (records plus continuation lines).
    pub lines: u64,
    pub bytes: u64,
    pub status: Status,
    /// The parser in use (detected when the saved one is Auto).
    pub format: Format,
    /// Bumped on every change, so the UI knows when to re-filter.
    pub version: u64,
    /// The file was truncated or replaced since it was opened.
    pub rotations: u32,
    /// Stderr of a command, for the status line.
    pub stderr: Vec<String>,
    /// The source's first 64 lines were larger than OPEN_BYTES, so the view
    /// starts part-way through the file.
    pub truncated_start: bool,
    /// How this source spells each level, learned as records arrive.
    pub aliases: parse::LevelAliases,
    /// The longest line seen, in characters. The list is monospaced, so
    /// this is all it needs to allocate a width wide enough to scroll to
    /// the end of the longest line rather than clipping it.
    pub max_chars: usize,
}

impl Shared {
    fn push_line(&mut self, format: Format, line: &str) {
        self.lines += 1;
        if let Some(last) = self.records.last_mut()
            && parse::is_continuation(format, line)
        {
            last.raw.push('\n');
            last.raw.push_str(line);
            if last.message.is_empty() {
                last.message = line.trim().to_string();
            }
            // A thread dump carries its state and its monitors on the
            // lines *below* the thread header, so the fields it is
            // searched by are read as those lines are folded in.
            parse::absorb_continuation(format, last, line);
            self.version += 1;
            return;
        }
        // Capped: a 4 k line is already past the point of reading, and
        // the row truncates there too.
        self.max_chars = self.max_chars.max(line.chars().take(4000).count());
        let rec = parse::parse(format, self.lines, line);
        // Learn the source's own spelling of each level, at most six
        // lookups for the whole log.
        if let Some(l) = rec.level
            && self.aliases.wants(l)
            && let Some(token) = parse::level_token(&rec)
        {
            let token = token.to_string();
            self.aliases.learn(l, &token);
        }
        self.records.push(rec);
        if self.records.len() > WINDOW {
            let extra = self.records.len() - WINDOW;
            self.records.drain(..extra);
            self.dropped += extra as u64;
        }
        self.version += 1;
    }

    /// Re-read the sample with a different parser. While the format is
    /// still being detected the records on screen were parsed with a
    /// running guess; when the verdict changes they have to be rebuilt,
    /// or a journalctl tail keeps the plain parsing of its first line
    /// for ever. The sample is capped at `SAMPLE_LINES`, so this is a
    /// few dozen records at most and never runs once the format settles.
    fn reparse(&mut self, format: Format, sample: &[String]) {
        self.records.clear();
        self.lines = 0;
        // A different parser may name levels differently.
        self.aliases.clear();
        self.max_chars = 0;
        for l in sample {
            self.push_line(format, l);
        }
        self.version += 1;
    }
}

/// How many lines the detector looks at before fixing the parser.
pub const SAMPLE_LINES: usize = 64;

/// Split a chunk into lines, holding back an unterminated last line.
#[derive(Default)]
pub struct LineSplitter {
    pending: Vec<u8>,
}

impl LineSplitter {
    pub fn push(&mut self, bytes: &[u8]) -> Vec<String> {
        self.pending.extend_from_slice(bytes);
        let mut out = Vec::new();
        while let Some(i) = self.pending.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = self.pending.drain(..=i).collect();
            out.push(line_text(&line[..line.len() - 1]));
        }
        out
    }

    /// Whatever is left (the last line of a file without a newline).
    pub fn flush(&mut self) -> Option<String> {
        if self.pending.is_empty() {
            None
        } else {
            Some(line_text(&std::mem::take(&mut self.pending)))
        }
    }
}

fn line_text(bytes: &[u8]) -> String {
    let bytes = bytes.strip_suffix(b"\r").unwrap_or(bytes);
    String::from_utf8_lossy(bytes).into_owned()
}

/// A local file read incrementally: `poll` returns the lines written since
/// the last call and notices truncation and rotation.
pub struct FileFollower {
    path: PathBuf,
    pos: u64,
    /// The file's first bytes when it was last opened: a rename+create
    /// leaves a file that starts differently. NTFS tunnels creation times
    /// (a file re-created under the same name within 15 s inherits the old
    /// one) and the inode is not portable, so the content is the identity.
    head: Vec<u8>,
    splitter: LineSplitter,
    pub rotations: u32,
}

/// How many leading bytes identify a file.
const HEAD: usize = 512;

fn head_of(path: &Path, len: u64) -> std::io::Result<Vec<u8>> {
    let mut f = File::open(path)?;
    let mut buf = vec![0u8; (len.min(HEAD as u64)) as usize];
    f.read_exact(&mut buf)?;
    Ok(buf)
}

impl FileFollower {
    pub fn new(path: &Path) -> Self {
        Self {
            path: path.to_path_buf(),
            pos: 0,
            head: Vec::new(),
            splitter: LineSplitter::default(),
            rotations: 0,
        }
    }

    /// Read from `from` (a byte offset) to the end now; `pos` follows.
    pub fn read_from(&mut self, from: u64) -> std::io::Result<Vec<String>> {
        let mut f = File::open(&self.path)?;
        let len = f.metadata()?.len();
        f.seek(SeekFrom::Start(from))?;
        let mut buf = Vec::new();
        f.read_to_end(&mut buf)?;
        self.pos = from + buf.len() as u64;
        // The head is what identifies the file next time; a file read
        // from its start supplies it directly.
        self.head = if from == 0 {
            buf.iter().take(HEAD).copied().collect()
        } else {
            head_of(&self.path, len).unwrap_or_default()
        };
        // A file read to its end: the last line counts even without a
        // newline, and a later append continues it.
        let mut lines = self.splitter.push(&buf);
        if let Some(rest) = self.splitter.flush() {
            lines.push(rest);
        }
        Ok(lines)
    }

    /// Open at the end of a large file (the last OPEN_BYTES), or at the
    /// start of a small one. Returns the lines and whether the start was
    /// cut.
    pub fn open(&mut self) -> std::io::Result<(Vec<String>, bool)> {
        let len = std::fs::metadata(&self.path)?.len();
        let from = len.saturating_sub(OPEN_BYTES);
        let mut lines = self.read_from(from)?;
        if from > 0 && !lines.is_empty() {
            // The first line is almost certainly a fragment.
            lines.remove(0);
        }
        Ok((lines, from > 0))
    }

    /// New lines since the last call. A shorter file or a new identity is
    /// a rotation: start over from the beginning.
    pub fn poll(&mut self) -> std::io::Result<Vec<String>> {
        let meta = match std::fs::metadata(&self.path) {
            Ok(m) => m,
            // Mid-rotation the file may be gone for a moment.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e),
        };
        let len = meta.len();
        // Shrunk (copytruncate), or the same path now starts differently
        // (rename + create). An untouched head of the same length is the
        // same file growing.
        let rotated = len < self.pos || {
            let want = self.head.len().min(len as usize);
            want > 0 && head_of(&self.path, want as u64).is_ok_and(|h| h != self.head[..want])
        };
        if rotated {
            self.rotations += 1;
            self.pos = 0;
            self.splitter = LineSplitter::default();
            return self.read_from(0);
        }
        if len == self.pos {
            return Ok(Vec::new());
        }
        self.read_from(self.pos)
    }

    pub fn position(&self) -> u64 {
        self.pos
    }
}

/// The parser for a source: the saved one, or detected from a sample.
/// Why a log file would not open, in terms the reader can act on.
///
/// A database's log is the case that bites: MySQL and PostgreSQL on
/// Windows keep theirs under `ProgramData` with an ACL that grants only
/// the service account and the administrators group, so a normal user's
/// process is refused with a bare "Access is denied. (os error 5)" that
/// says nothing about what to do (owner's report, 2026-10-24). The raw
/// error is kept - it is the ground truth - and the remedy added after
/// it.
pub fn explain_open(path: &std::path::Path, e: &std::io::Error) -> String {
    let shown = path.display();
    match e.kind() {
        std::io::ErrorKind::PermissionDenied => format!(
            "{shown}: {e} - the file's permissions do not let this user read it. \
             Grant your account Read on the file, or start HyperStack as an \
             administrator. For a server running in a container or as a unit, \
             follow its own log instead: Add Source, source type Command."
        ),
        std::io::ErrorKind::NotFound => format!(
            "{shown}: {e} - check the path, and whether the server logs to a \
             file at all rather than to stdout or the Event Log."
        ),
        _ => format!("{shown}: {e}"),
    }
}

fn pick_format(saved: Format, sample: &[String]) -> Format {
    if saved != Format::Auto {
        return saved;
    }
    let refs: Vec<&str> = sample.iter().map(String::as_str).collect();
    parse::detect(&refs)
}

/// A running source. Dropping it stops the task.
pub struct Runner {
    stop: Arc<AtomicBool>,
}

impl Drop for Runner {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn feed(shared: &Mutex<Shared>, format: Format, lines: &[String], bytes: u64) {
    let mut sh = shared.lock();
    for l in lines {
        sh.push_line(format, l);
    }
    sh.bytes += bytes;
}

fn set_status(shared: &Mutex<Shared>, status: Status) {
    let mut sh = shared.lock();
    sh.status = status;
    sh.version += 1;
}

/// Start reading `spec` into `shared`. `text` is the pasted content for a
/// text source.
pub fn start(
    services: &Services,
    spec: &SourceSpec,
    saved_format: Format,
    tail_lines: u32,
    text: Option<String>,
    shared: Arc<Mutex<Shared>>,
) -> Runner {
    let stop = Arc::new(AtomicBool::new(false));
    let waker = services.runtime.waker();
    let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(move || waker.request_repaint());
    {
        let mut sh = shared.lock();
        *sh = Shared::default();
        sh.status = Status::Connecting;
    }
    let runner = Runner { stop: stop.clone() };
    match spec {
        SourceSpec::Text { .. } => {
            let text = text.unwrap_or_default();
            let lines: Vec<String> = text.lines().map(|l| l.to_string()).collect();
            let format = pick_format(saved_format, &lines);
            let mut sh = shared.lock();
            sh.format = format;
            for l in &lines {
                sh.push_line(format, l);
            }
            sh.bytes = text.len() as u64;
            sh.status = Status::Done;
            sh.version += 1;
        }
        SourceSpec::LocalFile { path } => {
            let path = PathBuf::from(path);
            services.runtime.spawn(async move {
                let r = tokio::task::spawn_blocking(move || {
                    follow_local(&path, saved_format, &shared, &stop, &wake)
                })
                .await;
                if let Err(e) = r {
                    tracing::warn!("log follower ended: {e}");
                }
            });
        }
        SourceSpec::SshFile { conn_id, path } => {
            let command = format!("tail -n {tail_lines} -F {}", shell_quote(path));
            start_remote(services, conn_id, command, saved_format, shared, stop, wake);
        }
        SourceSpec::Command {
            conn_id: Some(conn_id),
            command,
        } => {
            start_remote(
                services,
                conn_id,
                command.clone(),
                saved_format,
                shared,
                stop,
                wake,
            );
        }
        SourceSpec::Command {
            conn_id: None,
            command,
        } => {
            let command = command.clone();
            services.runtime.spawn(async move {
                let r = tokio::task::spawn_blocking(move || {
                    run_local_command(&command, saved_format, &shared, &stop, &wake)
                })
                .await;
                if let Err(e) = r {
                    tracing::warn!("log command ended: {e}");
                }
            });
        }
    }
    runner
}

fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

fn follow_local(
    path: &Path,
    saved_format: Format,
    shared: &Mutex<Shared>,
    stop: &AtomicBool,
    wake: &Arc<dyn Fn() + Send + Sync>,
) {
    let mut follower = FileFollower::new(path);
    set_status(shared, Status::Reading);
    let (lines, cut) = match follower.open() {
        Ok(v) => v,
        Err(e) => {
            set_status(shared, Status::Failed(explain_open(path, &e)));
            wake();
            return;
        }
    };
    let format = pick_format(saved_format, &lines);
    {
        let mut sh = shared.lock();
        sh.format = format;
        sh.truncated_start = cut;
    }
    feed(shared, format, &lines, follower.position());
    set_status(shared, Status::Following);
    wake();
    while !stop.load(Ordering::Relaxed) {
        std::thread::sleep(POLL);
        match follower.poll() {
            Ok(lines) => {
                if follower.rotations > shared.lock().rotations {
                    let mut sh = shared.lock();
                    sh.rotations = follower.rotations;
                    sh.version += 1;
                }
                if !lines.is_empty() {
                    let bytes: u64 = lines.iter().map(|l| l.len() as u64 + 1).sum();
                    feed(shared, format, &lines, bytes);
                    wake();
                }
            }
            Err(e) => {
                set_status(shared, Status::Failed(e.to_string()));
                wake();
                return;
            }
        }
    }
}

fn run_local_command(
    command: &str,
    saved_format: Format,
    shared: &Mutex<Shared>,
    stop: &AtomicBool,
    wake: &Arc<dyn Fn() + Send + Sync>,
) {
    use std::process::Stdio;
    // `background_command`, not `Command::new`: a GUI app that spawns a
    // console program flashes a console window on Windows, and a followed
    // command keeps one open for as long as the tab lives. HyperStack has
    // its own terminal; it should never put a stray one on the desktop
    // (owner's report, 2026-10-24).
    let mut cmd = if cfg!(windows) {
        let mut c = hs_core::paths::background_command("cmd");
        c.args(["/C", command]);
        c
    } else {
        let mut c = hs_core::paths::background_command("sh");
        c.args(["-c", command]);
        c
    };
    let mut child = match cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            set_status(shared, Status::Failed(e.to_string()));
            wake();
            return;
        }
    };
    set_status(shared, Status::Following);
    let mut out = child.stdout.take().expect("piped");
    let mut err = child.stderr.take().expect("piped");
    // Stderr on its own thread, so a chatty command cannot block stdout.
    let shared_err: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let err_sink = shared_err.clone();
    let err_thread = std::thread::spawn(move || {
        let mut split = LineSplitter::default();
        let mut buf = [0u8; 8192];
        loop {
            match err.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => err_sink.lock().extend(split.push(&buf[..n])),
            }
        }
    });
    let mut split = LineSplitter::default();
    let mut format: Option<Format> = None;
    let mut sample: Vec<String> = Vec::new();
    let mut buf = [0u8; 16384];
    loop {
        if stop.load(Ordering::Relaxed) {
            let _ = child.kill();
            break;
        }
        match out.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                let lines = split.push(&buf[..n]);
                ingest(
                    &mut format,
                    &mut sample,
                    saved_format,
                    shared,
                    &lines,
                    n as u64,
                );
                let errs: Vec<String> = std::mem::take(&mut *shared_err.lock());
                if !errs.is_empty() {
                    shared.lock().stderr.extend(errs);
                }
                wake();
            }
        }
    }
    if let Some(rest) = split.flush() {
        ingest(&mut format, &mut sample, saved_format, shared, &[rest], 0);
    }
    let _ = err_thread.join();
    let errs: Vec<String> = std::mem::take(&mut *shared_err.lock());
    if !errs.is_empty() {
        shared.lock().stderr.extend(errs);
    }
    let _ = child.wait();
    if !stop.load(Ordering::Relaxed) {
        set_status(shared, Status::Done);
    }
    wake();
}

/// Feed lines, detecting the format from the first ones when it is Auto.
fn ingest(
    format: &mut Option<Format>,
    sample: &mut Vec<String>,
    saved_format: Format,
    shared: &Mutex<Shared>,
    lines: &[String],
    bytes: u64,
) {
    // Settled: the sample is full, or the user pinned a parser.
    if let Some(f) = *format {
        feed(shared, f, lines, bytes);
        return;
    }
    if saved_format != Format::Auto {
        *format = Some(saved_format);
        shared.lock().format = saved_format;
        feed(shared, saved_format, lines, bytes);
        return;
    }
    // Auto, still sampling. Show the lines now with the best guess so far
    // rather than holding them back until 64 have arrived: a followed
    // source that is quiet - `journalctl -f -u docker` on an idle unit -
    // would otherwise look empty until the user picked a parser by hand
    // (owner's report, 2026-10-23).
    sample.extend(lines.iter().cloned());
    let guess = pick_format(Format::Auto, sample);
    let settled = sample.len() >= SAMPLE_LINES;
    let previous = shared.lock().format;
    if guess != previous {
        // The verdict changed, so what was already shown was parsed with
        // the wrong one. The sample is bounded, so re-reading it is cheap.
        let mut sh = shared.lock();
        sh.format = guess;
        sh.reparse(guess, sample);
        sh.bytes += bytes;
    } else {
        feed(shared, guess, lines, bytes);
    }
    if settled {
        *format = Some(guess);
        sample.clear();
    }
}

/// Lock the parser in when a stream ends before the sample filled. The
/// records are already on screen (see `ingest`); this only re-reads them
/// if the final verdict differs from the running guess.
fn settle(
    format: &mut Option<Format>,
    sample: &mut Vec<String>,
    saved_format: Format,
    shared: &Mutex<Shared>,
) {
    if format.is_some() {
        return;
    }
    let f = pick_format(saved_format, sample);
    let mut sh = shared.lock();
    if sh.format != f {
        sh.format = f;
        sh.reparse(f, sample);
    }
    drop(sh);
    *format = Some(f);
    sample.clear();
}

#[allow(clippy::too_many_arguments)]
fn start_remote(
    services: &Services,
    conn_id: &str,
    command: String,
    saved_format: Format,
    shared: Arc<Mutex<Shared>>,
    stop: Arc<AtomicBool>,
    wake: Arc<dyn Fn() + Send + Sync>,
) {
    let profile = services.connections.read().by_id(conn_id).cloned();
    let Some(profile) = profile else {
        set_status(&shared, Status::Failed(services.t("logs-no-profile")));
        wake();
        return;
    };
    let fields = SshFields::from_table(&profile.fields);
    let cfg = match config_from_fields(&fields, services.secrets.as_ref()) {
        Ok(c) => c,
        Err(e) => {
            let msg = match e {
                ConfigError::VaultLocked => services.t("logs-vault-locked"),
                ConfigError::NoPassword => services.t("logs-no-password"),
                ConfigError::Secret(e) => e,
            };
            set_status(&shared, Status::Failed(msg));
            wake();
            return;
        }
    };
    let known_hosts = Arc::new(KnownHosts::load(&services.paths.known_hosts_file()));
    let unknown_host = services.t("logs-unknown-host");
    services.runtime.spawn(async move {
        // No host-key prompt here: the host must already be trusted (open
        // it once in the SSH activity), which is what a log viewer that
        // may start at launch with a saved source should expect.
        let client = match SshClient::connect(cfg, known_hosts, None).await {
            Ok(c) => c,
            Err(hs_ssh::SshError::HostKeyRejected { .. }) => {
                set_status(&shared, Status::Failed(unknown_host));
                wake();
                return;
            }
            Err(e) => {
                set_status(&shared, Status::Failed(e.to_string()));
                wake();
                return;
            }
        };
        let mut stream = match client.exec(&command).await {
            Ok(s) => s,
            Err(e) => {
                set_status(&shared, Status::Failed(e.to_string()));
                wake();
                return;
            }
        };
        set_status(&shared, Status::Following);
        wake();
        let mut split = LineSplitter::default();
        let mut err_split = LineSplitter::default();
        let mut format: Option<Format> = None;
        let mut sample: Vec<String> = Vec::new();
        loop {
            if stop.load(Ordering::Relaxed) {
                stream.stop();
                break;
            }
            let chunk = tokio::select! {
                c = stream.next() => c,
                _ = tokio::time::sleep(Duration::from_millis(250)) => continue,
            };
            match chunk {
                Some(ExecChunk::Stdout(bytes)) => {
                    let lines = split.push(&bytes);
                    ingest(
                        &mut format,
                        &mut sample,
                        saved_format,
                        &shared,
                        &lines,
                        bytes.len() as u64,
                    );
                    wake();
                }
                Some(ExecChunk::Stderr(bytes)) => {
                    let lines = err_split.push(&bytes);
                    if !lines.is_empty() {
                        let mut sh = shared.lock();
                        sh.stderr.extend(lines);
                        sh.version += 1;
                        wake();
                    }
                }
                other => {
                    let code = match other {
                        Some(ExecChunk::Exit(c)) => c,
                        _ => 0,
                    };
                    if let Some(rest) = split.flush() {
                        ingest(&mut format, &mut sample, saved_format, &shared, &[rest], 0);
                    }
                    settle(&mut format, &mut sample, saved_format, &shared);
                    if !stop.load(Ordering::Relaxed) {
                        let stderr_tail = shared.lock().stderr.last().cloned();
                        set_status(
                            &shared,
                            match (code, stderr_tail) {
                                (0, _) => Status::Done,
                                (c, Some(e)) => Status::Failed(format!("exit {c}: {e}")),
                                (c, None) => Status::Failed(format!("exit {c}")),
                            },
                        );
                    }
                    wake();
                    break;
                }
            }
        }
        client.disconnect().await;
    });
}

#[cfg(test)]
mod open_error_tests {
    use super::explain_open;
    use std::io::{Error, ErrorKind};
    use std::path::Path;

    /// The bare "Access is denied. (os error 5)" a database log gives on
    /// Windows says nothing about what to do; the remedy has to be in
    /// the message, because this is the one error a reader cannot
    /// diagnose from the log tool (owner's report, 2026-10-24).
    #[test]
    fn a_refused_log_file_says_how_to_get_at_it() {
        let e = Error::new(
            ErrorKind::PermissionDenied,
            "Access is denied. (os error 5)",
        );
        let msg = explain_open(Path::new(r"C:\ProgramData\MySQL\data\host.err"), &e);
        assert!(msg.contains("host.err"), "the path is named: {msg}");
        assert!(
            msg.contains("Access is denied"),
            "the real error is kept: {msg}"
        );
        assert!(msg.contains("Read"), "it says to grant Read: {msg}");
        assert!(msg.contains("administrator"), "and the other way: {msg}");
        assert!(msg.contains("Command"), "and the way round it: {msg}");
    }

    #[test]
    fn a_missing_log_file_asks_whether_the_server_writes_one() {
        let e = Error::new(
            ErrorKind::NotFound,
            "The system cannot find the file specified.",
        );
        let msg = explain_open(Path::new("/var/log/nope.log"), &e);
        assert!(msg.contains("nope.log"));
        assert!(msg.contains("stdout"), "it names the likely reason: {msg}");
    }

    /// Any other error is passed through unchanged: inventing advice for
    /// an error we have not seen would be worse than the error.
    #[test]
    fn an_unrecognised_error_is_left_alone() {
        let e = Error::other("disk on fire");
        let msg = explain_open(Path::new("/var/log/a.log"), &e);
        assert_eq!(msg, "/var/log/a.log: disk on fire");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn splitter_holds_back_a_partial_line_and_strips_cr() {
        let mut s = LineSplitter::default();
        assert_eq!(s.push(b"a\r\nb\nc"), vec!["a", "b"]);
        assert_eq!(s.push(b"d\n"), vec!["cd"]);
        assert_eq!(s.flush(), None);
        s.push(b"tail");
        assert_eq!(s.flush().as_deref(), Some("tail"));
    }

    #[test]
    fn follower_reads_appends_and_notices_truncation_and_rotation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("app.log");
        std::fs::write(&path, "one\ntwo\n").unwrap();
        let mut f = FileFollower::new(&path);
        let (lines, cut) = f.open().unwrap();
        assert_eq!(lines, vec!["one", "two"]);
        assert!(!cut);
        assert_eq!(f.poll().unwrap(), Vec::<String>::new());
        // Append, including a line without a newline that is completed later.
        let mut h = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        h.write_all(b"three\nfou").unwrap();
        drop(h);
        assert_eq!(f.poll().unwrap(), vec!["three", "fou"]);
        let mut h = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        h.write_all(b"r\nfive\n").unwrap();
        drop(h);
        assert_eq!(f.poll().unwrap(), vec!["r", "five"]);
        // Truncation (copytruncate): the file shrinks, reading starts over.
        std::fs::write(&path, "fresh\n").unwrap();
        assert_eq!(f.poll().unwrap(), vec!["fresh"]);
        assert_eq!(f.rotations, 1);
        // Rename + create (logrotate): a new file at the same path.
        let rotated = dir.path().join("app.log.1");
        std::fs::rename(&path, &rotated).unwrap();
        std::thread::sleep(Duration::from_millis(20));
        std::fs::write(&path, "newfile\nmore\n").unwrap();
        let after = f.poll().unwrap();
        assert!(
            after.ends_with(&["newfile".to_string(), "more".to_string()]),
            "{after:?}"
        );
        assert!(f.rotations >= 1);
        // The file vanishing for a moment is not an error.
        std::fs::remove_file(&path).unwrap();
        assert_eq!(f.poll().unwrap(), Vec::<String>::new());
    }

    #[test]
    fn records_window_is_bounded_and_continuations_glue() {
        let mut sh = Shared::default();
        sh.push_line(Format::Plain, "2026-10-22 14:32:01 ERROR boom");
        sh.push_line(
            Format::Plain,
            "    at com.shop.OrderSvc.charge(OrderSvc.java:212)",
        );
        sh.push_line(Format::Plain, "2026-10-22 14:32:02 INFO ok");
        assert_eq!(sh.records.len(), 2);
        assert!(sh.records[0].raw.contains("OrderSvc.java:212"));
        assert_eq!(sh.records[1].line, 3);
        for i in 0..(WINDOW + 10) {
            sh.push_line(Format::Plain, &format!("line {i}"));
        }
        assert_eq!(sh.records.len(), WINDOW);
        assert_eq!(sh.dropped, 12);
    }
}
