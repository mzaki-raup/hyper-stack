//! Text to speech through the operating system's own voice (Accessibility →
//! Read Aloud). Nothing is bundled and nothing leaves the machine: Windows
//! speaks through `System.Speech` (Windows PowerShell), macOS through `say`,
//! Linux through `espeak-ng`, `espeak` or `spd-say`, whichever is installed.
//! The text goes to the child's stdin, never onto a command line.

use std::io::Write;
use std::process::{Child, Command, Stdio};

use parking_lot::Mutex;

#[derive(Default)]
pub struct Tts {
    child: Mutex<Option<Child>>,
}

impl Tts {
    pub fn new() -> Self {
        Self::default()
    }

    /// Stop whatever is being read and read `text` instead. `rate` runs
    /// from -10 (slowest) to 10 (fastest), 0 being the voice's default.
    pub fn speak(&self, text: &str, rate: i32) -> Result<(), String> {
        self.stop();
        let text = text.trim();
        if text.is_empty() {
            return Err("nothing to read".into());
        }
        let mut cmd = command(rate.clamp(-10, 10)).ok_or_else(|| {
            "no speech engine found (install espeak-ng or speech-dispatcher)".to_string()
        })?;
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        let mut child = cmd.spawn().map_err(|e| e.to_string())?;
        if let Some(mut stdin) = child.stdin.take() {
            let body = text.to_string();
            // The engine reads stdin to the end before it speaks; a long
            // text must not block the UI thread while the pipe fills.
            std::thread::spawn(move || {
                let _ = stdin.write_all(body.as_bytes());
            });
        }
        *self.child.lock() = Some(child);
        Ok(())
    }

    pub fn stop(&self) {
        if let Some(mut c) = self.child.lock().take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }

    /// Whether a read-aloud is still running (reaps a finished child).
    pub fn is_speaking(&self) -> bool {
        let mut guard = self.child.lock();
        match guard.as_mut() {
            Some(c) => match c.try_wait() {
                Ok(Some(_)) | Err(_) => {
                    *guard = None;
                    false
                }
                Ok(None) => true,
            },
            None => false,
        }
    }

    /// Whether a speech engine is likely available on this machine.
    pub fn available() -> bool {
        command(0).is_some()
    }
}

impl Drop for Tts {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(windows)]
fn command(rate: i32) -> Option<Command> {
    // See `stt::command`: no console window for Read Aloud either.
    let mut c = crate::paths::background_command("powershell.exe");
    c.args([
        "-NoProfile",
        "-NonInteractive",
        "-ExecutionPolicy",
        "Bypass",
        "-Command",
        &format!(
            "Add-Type -AssemblyName System.Speech; \
             $s = New-Object System.Speech.Synthesis.SpeechSynthesizer; \
             $s.Rate = {rate}; \
             $s.Speak([Console]::In.ReadToEnd())"
        ),
    ]);
    Some(c)
}

#[cfg(target_os = "macos")]
fn command(rate: i32) -> Option<Command> {
    // `say` takes words per minute; 175 is its default.
    let wpm = 175 + rate * 12;
    let mut c = Command::new("say");
    c.args(["-r", &wpm.to_string()]);
    Some(c)
}

#[cfg(not(any(windows, target_os = "macos")))]
fn command(rate: i32) -> Option<Command> {
    let on_path = |name: &str| {
        std::env::var_os("PATH")
            .is_some_and(|p| std::env::split_paths(&p).any(|dir| dir.join(name).is_file()))
    };
    for engine in ["espeak-ng", "espeak"] {
        if on_path(engine) {
            let mut c = Command::new(engine);
            // espeak's speed is words per minute, 175 default.
            c.args(["--stdin", "-s", &(175 + rate * 12).to_string()]);
            return Some(c);
        }
    }
    if on_path("spd-say") {
        let mut c = Command::new("spd-say");
        c.args(["-e", "-w", "-r", &(rate * 10).to_string()]);
        return Some(c);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_text_is_refused_and_stop_is_idempotent() {
        let tts = Tts::new();
        assert!(tts.speak("   ", 0).is_err());
        tts.stop();
        tts.stop();
        assert!(!tts.is_speaking());
    }
}
