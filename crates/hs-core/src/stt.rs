//! Speech to text through the operating system's own dictation engine
//! (the microphone toggle in the AI assistants). Nothing is bundled and
//! nothing leaves the machine: Windows listens through
//! `System.Speech.Recognition` (Windows PowerShell) with its dictation
//! grammar in continuous mode, writing each recognised phrase as one line
//! on stdout. Other platforms report no engine for now; the button stays
//! hidden there.
//!
//! The engine runs as a child process so a hung or crashed recogniser can
//! never take the UI thread with it. Stopping is graceful: the host writes
//! `stop` to the child's stdin, the engine finishes the phrase in flight
//! and prints it, then exits; what it printed stays readable until the
//! panel has taken it. A session also ends on its own after `max_secs`.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

/// The line the engine prints after its last phrase.
const END_MARK: &str = "<<end>>";
/// How long a graceful stop may take before the child is killed.
const STOP_GRACE: Duration = Duration::from_millis(2500);

#[derive(Default)]
struct Shared {
    /// Phrases heard since the panel last took them.
    heard: Vec<String>,
    /// What the engine said on stderr, if it failed.
    error: Option<String>,
    /// The engine printed its end mark (or its stdout closed).
    ended: bool,
}

struct Running {
    child: Child,
    stdin: Option<ChildStdin>,
    shared: Arc<Mutex<Shared>>,
    started: Instant,
    max: Duration,
    stop_asked: Option<Instant>,
}

/// One listener. A panel owns one; starting it again stops the previous
/// child first, so at most one microphone session runs per panel.
#[derive(Default)]
pub struct Stt {
    running: Mutex<Option<Running>>,
}

impl Stt {
    pub fn new() -> Self {
        Self::default()
    }

    /// Start listening. `language` is a culture name such as `en-US`
    /// (empty picks the engine's default recogniser); the session ends on
    /// its own after `max_secs`.
    pub fn start(&self, language: &str, max_secs: u32) -> Result<(), String> {
        self.stop();
        let max_secs = max_secs.clamp(5, 600);
        let mut cmd = command(language.trim(), max_secs)
            .ok_or_else(|| "no speech recognition engine on this platform".to_string())?;
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        let mut child = cmd.spawn().map_err(|e| e.to_string())?;
        let shared = Arc::new(Mutex::new(Shared::default()));
        if let Some(out) = child.stdout.take() {
            let sink = shared.clone();
            std::thread::spawn(move || {
                for line in BufReader::new(out).lines().map_while(Result::ok) {
                    let line = line.trim().to_string();
                    if line == END_MARK {
                        break;
                    }
                    if !line.is_empty() {
                        sink.lock().heard.push(line);
                    }
                }
                sink.lock().ended = true;
            });
        }
        if let Some(err) = child.stderr.take() {
            let sink = shared.clone();
            std::thread::spawn(move || {
                let mut text = String::new();
                for line in BufReader::new(err).lines().map_while(Result::ok) {
                    if !line.trim().is_empty() {
                        text.push_str(line.trim());
                        text.push('\n');
                    }
                }
                if !text.trim().is_empty() {
                    sink.lock().error = Some(first_error_line(&text));
                }
            });
        }
        let stdin = child.stdin.take();
        *self.running.lock() = Some(Running {
            child,
            stdin,
            shared,
            started: Instant::now(),
            max: Duration::from_secs(u64::from(max_secs)),
            stop_asked: None,
        });
        Ok(())
    }

    /// Ask the engine to finish the phrase in flight and exit. Phrases it
    /// still prints stay readable through [`Self::take_heard`]; the child
    /// is killed if it has not exited within a short grace period.
    pub fn stop(&self) {
        let mut guard = self.running.lock();
        let Some(r) = guard.as_mut() else { return };
        if r.stop_asked.is_none() {
            r.stop_asked = Some(Instant::now());
            if let Some(mut stdin) = r.stdin.take() {
                let _ = stdin.write_all(b"stop\n");
                let _ = stdin.flush();
                // Dropping it closes the pipe, which the engine also reads
                // as a stop.
            }
        }
    }

    /// Kill the child and forget everything (closing the panel).
    pub fn abort(&self) {
        if let Some(mut r) = self.running.lock().take() {
            let _ = r.child.kill();
            let _ = r.child.wait();
        }
    }

    /// Whether the engine is listening (not stopping, not ended).
    pub fn is_listening(&self) -> bool {
        let mut guard = self.running.lock();
        let Some(r) = guard.as_mut() else {
            return false;
        };
        if r.stop_asked.is_some() || r.shared.lock().ended {
            return false;
        }
        matches!(r.child.try_wait(), Ok(None))
    }

    /// Seconds left before the session ends on its own.
    pub fn remaining_secs(&self) -> Option<u64> {
        let guard = self.running.lock();
        let r = guard.as_ref()?;
        Some(r.max.saturating_sub(r.started.elapsed()).as_secs())
    }

    /// Whether anything may still arrive (listening, or stopping and
    /// draining). The panel keeps polling while this is true.
    pub fn is_active(&self) -> bool {
        self.running.lock().is_some()
    }

    /// The phrases heard since the last call, oldest first. Also reaps a
    /// finished session once it has nothing left to say, and kills one
    /// that ignored the stop request.
    pub fn take_heard(&self) -> Vec<String> {
        let mut guard = self.running.lock();
        let Some(r) = guard.as_mut() else {
            return Vec::new();
        };
        let heard = std::mem::take(&mut r.shared.lock().heard);
        let exited = r.child.try_wait().map(|s| s.is_some()).unwrap_or(true);
        let ended = r.shared.lock().ended;
        let overdue = r.stop_asked.is_some_and(|t| t.elapsed() > STOP_GRACE);
        let timed_out = r.started.elapsed() > r.max + STOP_GRACE;
        if overdue || timed_out {
            let _ = r.child.kill();
        }
        if (exited || ended || overdue || timed_out) && heard.is_empty() {
            let has_error = r.shared.lock().error.is_some();
            if !has_error {
                let _ = r.child.wait();
                *guard = None;
            }
        }
        heard
    }

    /// The engine's error, when it stopped on its own (no microphone, no
    /// recogniser for the language, …). Taken once; the session is gone
    /// afterwards.
    pub fn take_error(&self) -> Option<String> {
        let mut guard = self.running.lock();
        let r = guard.as_mut()?;
        let exited = r.child.try_wait().map(|s| s.is_some()).unwrap_or(true);
        if !exited {
            return None;
        }
        let err = r.shared.lock().error.take();
        if err.is_some() {
            let _ = r.child.wait();
            *guard = None;
        }
        err
    }

    /// Whether a dictation engine exists on this platform.
    pub fn available() -> bool {
        command("", 60).is_some()
    }
}

impl Drop for Stt {
    fn drop(&mut self) {
        self.abort();
    }
}

/// PowerShell prints a stack of context after the message; the first
/// line is the one worth showing.
fn first_error_line(text: &str) -> String {
    let line = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    let line = line.trim().trim_start_matches(
        "Exception calling \"SetInputToDefaultAudioDevice\" with \"0\" argument(s): ",
    );
    let line = line.trim_matches('"');
    if line.is_empty() {
        "speech recognition stopped".to_string()
    } else {
        line.chars().take(160).collect()
    }
}

/// The dictation script. Continuous recognition raises `SpeechRecognized`
/// events, which the loop drains with `Wait-Event`; a line on stdin (or
/// stdin closing), or the time cap, stops recognition gracefully so the
/// phrase in flight is still printed before the end mark.
#[cfg(windows)]
fn script(ctor: &str, max_secs: u32) -> String {
    format!(
        "Add-Type -AssemblyName System.Speech; \
         [Console]::OutputEncoding = [System.Text.Encoding]::UTF8; \
         $r = New-Object System.Speech.Recognition.{ctor}; \
         $r.LoadGrammar((New-Object System.Speech.Recognition.DictationGrammar)); \
         $r.SetInputToDefaultAudioDevice(); \
         $null = Register-ObjectEvent -InputObject $r -EventName SpeechRecognized -SourceIdentifier hsHeard; \
         $r.RecognizeAsync([System.Speech.Recognition.RecognizeMode]::Multiple); \
         $stdin = [Console]::OpenStandardInput(); \
         $buf = New-Object byte[] 64; \
         $read = $stdin.ReadAsync($buf, 0, 64); \
         $deadline = (Get-Date).AddSeconds({max_secs}); \
         $stopping = $false; \
         while ($true) {{ \
           $ev = Wait-Event -SourceIdentifier hsHeard -Timeout 1; \
           if ($ev) {{ \
             $text = $ev.SourceEventArgs.Result.Text; \
             Remove-Event -EventIdentifier $ev.EventIdentifier; \
             if ($text) {{ [Console]::Out.WriteLine($text); [Console]::Out.Flush() }}; \
             continue \
           }}; \
           if ($stopping) {{ break }}; \
           if ($read.IsCompleted -or (Get-Date) -gt $deadline) {{ $stopping = $true; $r.RecognizeAsyncStop() }} \
         }}; \
         [Console]::Out.WriteLine('{END_MARK}'); [Console]::Out.Flush()"
    )
}

#[cfg(windows)]
fn command(language: &str, max_secs: u32) -> Option<Command> {
    // A culture name is letters and dashes only; anything else falls
    // back to the default recogniser rather than reaching the script.
    // `New-Object Type()` with empty parentheses is a PowerShell parse
    // error, so the default recogniser is constructed without them.
    let ctor = if !language.is_empty()
        && language
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-')
    {
        format!("SpeechRecognitionEngine([System.Globalization.CultureInfo]'{language}')")
    } else {
        "SpeechRecognitionEngine".to_string()
    };
    let mut c = Command::new("powershell.exe");
    c.args([
        "-NoProfile",
        "-NonInteractive",
        "-ExecutionPolicy",
        "Bypass",
        "-Command",
        &script(&ctor, max_secs),
    ]);
    Some(c)
}

#[cfg(not(windows))]
fn command(_language: &str, _max_secs: u32) -> Option<Command> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_listener_has_nothing_to_say() {
        let s = Stt::new();
        assert!(!s.is_listening());
        assert!(!s.is_active());
        assert!(s.take_heard().is_empty());
        assert!(s.take_error().is_none());
        assert_eq!(s.remaining_secs(), None);
        s.stop();
        s.abort();
    }

    #[test]
    fn error_text_keeps_the_first_meaningful_line() {
        let text = "\nException calling \"SetInputToDefaultAudioDevice\" with \"0\" argument(s): \"No audio device is installed.\"\nAt line:1 char:5\n";
        assert_eq!(first_error_line(text), "No audio device is installed.");
        assert_eq!(first_error_line("  \n"), "speech recognition stopped");
    }

    #[cfg(windows)]
    #[test]
    fn language_is_validated_before_it_reaches_the_script() {
        let args = |c: Command| -> Vec<String> {
            c.get_args()
                .map(|a| a.to_string_lossy().to_string())
                .collect()
        };
        let bad = args(command("en-US'; Remove-Item x #", 30).unwrap());
        assert!(bad.iter().any(|a| a.contains("SpeechRecognitionEngine;")));
        assert!(!bad.iter().any(|a| a.contains("Remove-Item")));
        let good = args(command("en-US", 30).unwrap());
        assert!(good.iter().any(|a| a.contains("CultureInfo]'en-US'")));
        assert!(good.iter().any(|a| a.contains("AddSeconds(30)")));
        assert!(good.iter().any(|a| a.contains(END_MARK)));
    }
}
