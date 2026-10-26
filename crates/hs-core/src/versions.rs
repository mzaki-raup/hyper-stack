//! "What version of that is installed?", asked once and remembered.
//!
//! Half the header bands want a version — the shell in a Terminal tab,
//! the engine behind a Container host, the language toolchain of a
//! project. Each answer costs a process spawn, and `Tool::header()` is
//! asked every frame, so nothing may ask the system there.
//!
//! The rule here is: a tool calls [`ensure`] every frame (it is cheap
//! and idempotent) and reads [`cached`] in `header()`. The first call
//! spawns one probe on the blocking pool; every later call does nothing.
//! A command that is not installed is remembered as "no answer" rather
//! than retried for ever.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// The first thing in `text` that looks like a version number.
///
/// Every tool announces itself differently — `rustc 1.89.0 (abc …)`,
/// `go version go1.24.2 windows/amd64`, `openjdk version "21.0.3"`,
/// `v22.11.0`, `Docker version 27.1.1, build …`, `podman version 5.1.2`
/// — so rather than a parser per tool, take the first run of digits and
/// dots that contains at least one dot. A bare `5` is not a version; a
/// date like `2026` would be, which is why the dot is required.
pub fn extract(text: &str) -> Option<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if !chars[i].is_ascii_digit() {
            i += 1;
            continue;
        }
        let start = i;
        let mut dots = 0;
        while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
            if chars[i] == '.' {
                dots += 1;
            }
            i += 1;
        }
        let found: String = chars[start..i].iter().collect();
        let found = found.trim_end_matches('.').to_string();
        if dots >= 1 && !found.is_empty() {
            return Some(found);
        }
    }
    None
}

type Cache = HashMap<String, Option<String>>;

fn cache() -> &'static Mutex<Cache> {
    static C: OnceLock<Mutex<Cache>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The answer for `key`, if one has been worked out. Never blocks and
/// never spawns: this is what `header()` calls.
pub fn cached(key: &str) -> Option<String> {
    cache().lock().ok()?.get(key)?.clone()
}

/// Remember an answer worked out some other way — a server that
/// reported its own version over the wire, say.
pub fn remember(key: &str, version: Option<String>) {
    if let Ok(mut c) = cache().lock() {
        c.insert(key.to_string(), version);
    }
}

/// Whether `key` has been asked about yet, however it was answered.
pub fn asked(key: &str) -> bool {
    cache().lock().map(|c| c.contains_key(key)).unwrap_or(true)
}

/// Run `program args` once, on the blocking pool, and keep the version
/// it prints under `key`.
///
/// Idempotent and cheap to call every frame. Both streams are read and
/// joined before parsing, because several tools print their version to
/// **stderr** (`java -version` famously).
pub fn ensure(runtime: &crate::runtime::AppRuntime, key: &str, program: &str, args: &[&str]) {
    if key.is_empty() || program.is_empty() {
        return;
    }
    {
        let mut c = match cache().lock() {
            Ok(c) => c,
            Err(_) => return,
        };
        if c.contains_key(key) {
            return;
        }
        // Claim it, so the next frame does not spawn a second probe.
        c.insert(key.to_string(), None);
    }
    let key = key.to_string();
    let program = program.to_string();
    let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
    runtime.spawn_blocking(move || {
        // `background_command` keeps a console window from flashing on
        // Windows, which a bare `Command::new` would do (CLAUDE.md).
        let out = crate::paths::background_command(&program)
            .args(&args)
            .output()
            .ok();
        let text = out
            .map(|o| {
                let mut s = String::from_utf8_lossy(&o.stdout).to_string();
                s.push(' ');
                s.push_str(&String::from_utf8_lossy(&o.stderr));
                s
            })
            .unwrap_or_default();
        if let Some(v) = extract(&text) {
            remember(&key, Some(v));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every tool announces itself differently; one rule has to cope.
    #[test]
    fn a_version_is_found_in_whatever_the_tool_prints() {
        for (text, want) in [
            ("rustc 1.89.0 (29483883e 2026-01-04)", "1.89.0"),
            ("go version go1.24.2 windows/amd64", "1.24.2"),
            ("openjdk version \"21.0.3\" 2026-04-16", "21.0.3"),
            ("v22.11.0", "22.11.0"),
            ("Python 3.12.4", "3.12.4"),
            ("Docker version 27.1.1, build 6312585", "27.1.1"),
            ("podman version 5.1.2", "5.1.2"),
            ("PowerShell 7.4.5", "7.4.5"),
            ("PHP 8.3.6 (cli) (built: ...)", "8.3.6"),
        ] {
            assert_eq!(extract(text).as_deref(), Some(want), "from {text:?}");
        }
    }

    /// A number with no dot is not a version: a build counter or a year
    /// would otherwise be reported as one.
    #[test]
    fn something_that_is_not_a_version_gets_no_answer() {
        assert_eq!(extract("command not found"), None);
        assert_eq!(extract(""), None);
        assert_eq!(extract("build 6312585"), None);
    }
}
