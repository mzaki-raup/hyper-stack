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
/// "Rust 1.89.0" rather than "1.89.0". The list itself lives in
/// `hs_core::projects`, because Settings offers the same names when a
/// project's language is chosen by hand.
pub fn display_name(language: &str) -> &'static str {
    hs_core::projects::language_name(language)
}

/// The cached answer for `language`, if one has been worked out.
pub fn cached(language: &str) -> Option<String> {
    hs_core::versions::cached(&key(language))
}

/// Ask the toolchain once, on the blocking pool.
pub fn ensure(runtime: &hs_core::runtime::AppRuntime, language: &str) {
    let Some((program, args)) = probe(language) else {
        return;
    };
    hs_core::versions::ensure(runtime, &key(language), program, args);
}

fn key(language: &str) -> String {
    format!("lang:{language}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_language_without_a_version_still_has_a_name() {
        assert_eq!(display_name("csharp"), "C#");
        assert_eq!(display_name("cpp"), "C++");
        assert!(probe("cpp").is_none(), "no single C++ toolchain to ask");
        assert_eq!(display_name("nonsense"), "");
    }
}
