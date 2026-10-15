//! Speech to text through the operating system's own dictation engine
//! (the microphone toggle in the AI assistants). Nothing is bundled and
//! nothing leaves the machine: Windows listens through
//! `System.Speech.Recognition` (Windows PowerShell) with its dictation
//! grammar, writing each recognised phrase as one line on stdout. Other
//! platforms report no engine for now; the button stays hidden there.
//!
//! The engine runs as a child process so a hung or crashed recogniser can
//! never take the UI thread with it; `stop` kills it.

use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;

use parking_lot::Mutex;

#[derive(Default)]
struct Shared {
    /// Phrases heard since the panel last took them.
    heard: Vec<String>,
    /// What the engine said on stderr, if it failed.
    error: Option<String>,
    /// The reader thread saw the child's stdout close.
    ended: bool,
}

struct Running {
    child: Child,
    shared: Arc<Mutex<Shared>>,
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

    /// Start listening. `language` is a culture name such as `en-US`;
    /// empty picks the engine's default recogniser.
    pub fn start(&self, language: &str) -> Result<(), String> {
        self.stop();
        let mut cmd = command(language.trim())
            .ok_or_else(|| "no speech recognition engine on this platform".to_string())?;
        cmd.stdin(Stdio::null())
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
        *self.running.lock() = Some(Running { child, shared });
        Ok(())
    }

    pub fn stop(&self) {
        if let Some(mut r) = self.running.lock().take() {
            let _ = r.child.kill();
            let _ = r.child.wait();
        }
    }

    /// Whether the engine is still running (reaps a finished child).
    pub fn is_listening(&self) -> bool {
        let mut guard = self.running.lock();
        match guard.as_mut() {
            Some(r) => match r.child.try_wait() {
                Ok(Some(_)) | Err(_) => {
                    // Keep what it said until the panel takes it.
                    let shared = r.shared.clone();
                    let mut s = shared.lock();
                    s.ended = true;
                    drop(s);
                    false
                }
                Ok(None) => true,
            },
            None => false,
        }
    }

    /// The phrases heard since the last call, oldest first.
    pub fn take_heard(&self) -> Vec<String> {
        self.running
            .lock()
            .as_ref()
            .map(|r| std::mem::take(&mut r.shared.lock().heard))
            .unwrap_or_default()
    }

    /// The engine's error, when it stopped on its own (no microphone, no
    /// recogniser for the language, …). Taken once.
    pub fn take_error(&self) -> Option<String> {
        let mut guard = self.running.lock();
        let r = guard.as_mut()?;
        let ended = r.child.try_wait().map(|s| s.is_some()).unwrap_or(true);
        if !ended {
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
        command("").is_some()
    }
}

impl Drop for Stt {
    fn drop(&mut self) {
        self.stop();
    }
}

/// PowerShell prints a stack of context after the message; the first
/// line is the one worth a toast.
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

#[cfg(windows)]
fn command(language: &str) -> Option<Command> {
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
        &format!(
            "Add-Type -AssemblyName System.Speech; \
             [Console]::OutputEncoding = [System.Text.Encoding]::UTF8; \
             $r = New-Object System.Speech.Recognition.{ctor}; \
             $r.LoadGrammar((New-Object System.Speech.Recognition.DictationGrammar)); \
             $r.SetInputToDefaultAudioDevice(); \
             while ($true) {{ \
               $res = $r.Recognize([TimeSpan]::FromSeconds(30)); \
               if ($res -and $res.Text) {{ [Console]::Out.WriteLine($res.Text); [Console]::Out.Flush() }} \
             }}"
        ),
    ]);
    Some(c)
}

#[cfg(not(windows))]
fn command(_language: &str) -> Option<Command> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_listener_has_nothing_to_say() {
        let s = Stt::new();
        assert!(!s.is_listening());
        assert!(s.take_heard().is_empty());
        assert!(s.take_error().is_none());
        s.stop();
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
        let bad = command("en-US'; Remove-Item x #").unwrap();
        let args: Vec<String> = bad
            .get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect();
        assert!(args.iter().any(|a| a.contains("SpeechRecognitionEngine;")));
        assert!(!args.iter().any(|a| a.contains("Remove-Item")));
        let good = command("en-US").unwrap();
        let args: Vec<String> = good
            .get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect();
        assert!(args.iter().any(|a| a.contains("CultureInfo]'en-US'")));
    }
}
