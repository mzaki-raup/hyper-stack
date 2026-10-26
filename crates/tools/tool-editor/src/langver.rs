//! What version of a language's toolchain is installed.
//!
//! The Editor's header band says "Version : Rust 1.89.0" rather than
//! just "rust", which means asking the toolchain. That is a process
//! spawn, so it happens **once per language** on the blocking pool and
//! the answer is kept for the life of the session: a toolchain does not
//! change while the app is open, and `header()` is asked every frame.
//!
//! A language with no command, or a command that is not installed,
//! simply has no version - the band leaves the fact out rather than
//! showing "unknown".

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// The command that reports a version, and where it prints it.
///
/// Several of these write to **stderr** (`java -version` famously), so
/// both streams are read and joined before parsing.
fn probe(language: &str) -> Option<(&'static str, &'static [&'static str])> {
    Some(match language {
        "rust" => ("rustc", &["--version"]),
        "go" => ("go", &["version"]),
        "python" => ("python", &["--version"]),
        "java" => ("java", &["-version"]),
        "kotlin" => ("kotlinc", &["-version"]),
        "scala" => ("scala", &["-version"]),
        "javascript" | "typescript" | "node" => ("node", &["--version"]),
        "dart" => ("dart", &["--version"]),
        "php" => ("php", &["--version"]),
        "ruby" => ("ruby", &["--version"]),
        "csharp" => ("dotnet", &["--version"]),
        "elixir" => ("elixir", &["--version"]),
        "haskell" => ("ghc", &["--version"]),
        "perl" => ("perl", &["--version"]),
        "zig" => ("zig", &["version"]),
        "lua" => ("lua", &["-v"]),
        _ => return None,
    })
}

/// The human name a version is prefixed with, so the fact reads
/// "Rust 1.89.0" rather than "1.89.0".
pub fn display_name(language: &str) -> &'static str {
    match language {
        "rust" => "Rust",
        "go" => "Go",
        "python" => "Python",
        "java" => "Java",
        "kotlin" => "Kotlin",
        "scala" => "Scala",
        "javascript" => "JavaScript",
        "typescript" => "TypeScript",
        "node" => "Node.js",
        "dart" => "Dart",
        "php" => "PHP",
        "ruby" => "Ruby",
        "csharp" => "C#",
        "cpp" => "C++",
        "c" => "C",
        "elixir" => "Elixir",
        "haskell" => "Haskell",
        "perl" => "Perl",
        "zig" => "Zig",
        "lua" => "Lua",
        "css" => "CSS",
        "html" => "HTML",
        "markdown" => "Markdown",
        "shell" => "Shell",
        "terraform" => "Terraform",
        _ => "",
    }
}

/// The first thing in `text` that looks like a version number.
///
/// Every toolchain says it differently - `rustc 1.89.0 (abc 2026-01-01)`,
/// `go version go1.24.2 windows/amd64`, `openjdk version "21.0.3"`,
/// `v22.11.0` - so rather than a parser per tool, take the first run of
/// digits-and-dots with at least one dot.
pub fn extract(text: &str) -> Option<String> {
    let bytes: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_digit() {
            let start = i;
            let mut dots = 0;
            while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == '.') {
                if bytes[i] == '.' {
                    dots += 1;
                }
                i += 1;
            }
            let found: String = bytes[start..i].iter().collect();
            let found = found.trim_end_matches('.').to_string();
            if dots >= 1 && !found.is_empty() {
                return Some(found);
            }
        } else {
            i += 1;
        }
    }
    None
}

type Cache = HashMap<String, Option<String>>;

fn cache() -> &'static Mutex<Cache> {
    static C: OnceLock<Mutex<Cache>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The cached answer for `language`, if one has been worked out.
///
/// Never blocks and never spawns: this is what `header()` calls.
pub fn cached(language: &str) -> Option<String> {
    cache().lock().ok()?.get(language)?.clone()
}

/// Work the version out on the blocking pool if it is not known yet.
///
/// Idempotent and cheap to call every frame: once an answer is in the
/// cache - including "this toolchain is not installed", stored as
/// `None` - nothing runs again.
pub fn ensure(runtime: &hs_core::runtime::AppRuntime, language: &str) {
    if language.is_empty() {
        return;
    }
    {
        let mut c = match cache().lock() {
            Ok(c) => c,
            Err(_) => return,
        };
        if c.contains_key(language) {
            return;
        }
        // Claimed, so a second frame does not spawn a second probe.
        c.insert(language.to_string(), None);
    }
    let Some((program, args)) = probe(language) else {
        return;
    };
    let lang = language.to_string();
    runtime.spawn_blocking(move || {
        // `background_command` keeps a console window from flashing on
        // Windows, which a bare `Command::new` would do (CLAUDE.md).
        let out = hs_core::paths::background_command(program)
            .args(args)
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
        if let Some(v) = extract(&text)
            && let Ok(mut c) = cache().lock()
        {
            c.insert(lang, Some(v));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every toolchain announces itself differently; one rule has to
    /// cope with all of them.
    #[test]
    fn a_version_is_found_in_whatever_the_toolchain_prints() {
        for (text, want) in [
            ("rustc 1.89.0 (29483883e 2026-01-04)", "1.89.0"),
            ("go version go1.24.2 windows/amd64", "1.24.2"),
            ("openjdk version \"21.0.3\" 2026-04-16", "21.0.3"),
            ("v22.11.0", "22.11.0"),
            ("Python 3.12.4", "3.12.4"),
            ("PHP 8.3.6 (cli) (built: ...)", "8.3.6"),
            ("Lua 5.4.6  Copyright (C) 1994-2023", "5.4.6"),
        ] {
            assert_eq!(extract(text).as_deref(), Some(want), "from {text:?}");
        }
        // Nothing version-shaped: no answer rather than a wrong one.
        assert_eq!(extract("command not found"), None);
        assert_eq!(extract(""), None);
    }

    #[test]
    fn a_language_without_a_version_still_has_a_name() {
        assert_eq!(display_name("csharp"), "C#");
        assert_eq!(display_name("cpp"), "C++");
        assert!(probe("cpp").is_none(), "no single C++ toolchain to ask");
        assert_eq!(display_name("nonsense"), "");
    }
}
