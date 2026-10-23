//! Redaction rules (docs/tools/logs.md §15.4). Logs carry bearer tokens,
//! passwords, keys and personal data, and **Save Filtered… writes a plain
//! file**. The project's rule is that secrets never touch disk in plaintext
//! (docs/06-security.md), so everything that leaves a Logs tab goes through
//! here first: Copy, Copy as JSON, Save Filtered…, the assistant's context
//! and a Problems entry.
//!
//! Masking happens on the way out. The bytes in the engine's window are
//! never rewritten, so the **Mask Secrets** view toggle costs nothing to
//! flip and no reload.

use std::path::{Path, PathBuf};

use regex::Regex;
use serde::{Deserialize, Serialize};

use hs_core::storage;

pub const SCHEMA: u32 = 1;

/// What replaces a match. The capture group named `keep`, when a rule has
/// one, survives: `Authorization: Bearer xyz` becomes
/// `Authorization: Bearer ●●●●` rather than losing the header's name.
pub const MASK: &str = "\u{25cf}\u{25cf}\u{25cf}\u{25cf}";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Rule {
    /// Shown in Settings and on the toggle.
    pub name: String,
    pub pattern: String,
    pub enabled: bool,
}

impl Rule {
    pub fn new(name: &str, pattern: &str) -> Self {
        Self {
            name: name.into(),
            pattern: pattern.into(),
            enabled: true,
        }
    }
}

/// The workspace's rule set (`<workspace>/logs/redaction.toml`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuleSet {
    pub schema: u32,
    pub rules: Vec<Rule>,
}

impl Default for RuleSet {
    fn default() -> Self {
        Self {
            schema: SCHEMA,
            rules: defaults(),
        }
    }
}

/// The rules every workspace starts with. Each keeps the part that says
/// *what* was found and masks only the secret itself, because a log line
/// reading `●●●●` tells the reader nothing about what was redacted.
pub fn defaults() -> Vec<Rule> {
    vec![
        Rule::new(
            "Bearer tokens",
            r"(?P<keep>[Aa]uthorization\s*[:=]\s*[Bb]earer\s+)[A-Za-z0-9._\-+/=]{8,}",
        ),
        Rule::new(
            "Basic authentication",
            r"(?P<keep>[Aa]uthorization\s*[:=]\s*[Bb]asic\s+)[A-Za-z0-9+/=]{8,}",
        ),
        Rule::new(
            "JSON Web Tokens",
            r"eyJ[A-Za-z0-9_\-]{8,}\.eyJ[A-Za-z0-9_\-]{8,}\.[A-Za-z0-9_\-]{8,}",
        ),
        Rule::new(
            "Passwords",
            r#"(?P<keep>(?i:password|passwd|pwd)"?\s*[:=]\s*"?)[^\s",;)]{1,}"#,
        ),
        Rule::new(
            "API keys",
            r#"(?P<keep>(?i:api[-_]?key|access[-_]?token|secret)"?\s*[:=]\s*"?)[^\s",;)]{6,}"#,
        ),
        Rule::new("AWS access key ids", r"\b(?:AKIA|ASIA)[0-9A-Z]{16}\b"),
        Rule::new(
            "AWS secret keys",
            r"(?P<keep>(?i:aws_secret_access_key)\s*[:=]\s*)[A-Za-z0-9/+=]{40}",
        ),
        Rule::new(
            "E-mail addresses",
            r"\b[A-Za-z0-9._%+\-]+@[A-Za-z0-9.\-]+\.[A-Za-z]{2,}\b",
        ),
    ]
}

impl RuleSet {
    pub fn file_for(dir: &Path) -> PathBuf {
        dir.join("redaction.toml")
    }

    /// The workspace's rules, or the defaults when the file is missing. A
    /// file that will not parse is reported and the defaults are used: a
    /// broken rule file must never mean *no* masking.
    pub fn load(dir: &Path) -> Self {
        let p = Self::file_for(dir);
        if !p.exists() {
            return Self::default();
        }
        match storage::read_toml::<Self>(&p) {
            Ok(s) if s.schema <= SCHEMA => s,
            Ok(_) => {
                tracing::warn!(
                    "redaction rules {}: newer schema, using defaults",
                    p.display()
                );
                Self::default()
            }
            Err(e) => {
                tracing::warn!("redaction rules {}: {e}, using defaults", p.display());
                Self::default()
            }
        }
    }

    pub fn save(&self, dir: &Path) -> storage::Result<()> {
        let _ = std::fs::create_dir_all(dir);
        storage::write_toml(&Self::file_for(dir), self)
    }
}

/// The compiled rules, ready to mask with. Compiling is done once per
/// change, never per record.
#[derive(Default)]
pub struct Redactor {
    rules: Vec<Regex>,
}

impl Redactor {
    pub fn compile(set: &RuleSet) -> Self {
        let mut rules = Vec::new();
        for r in set.rules.iter().filter(|r| r.enabled) {
            match Regex::new(&r.pattern) {
                Ok(re) => rules.push(re),
                // A rule that will not compile is dropped loudly: silently
                // skipping it would be a secret reaching a file.
                Err(e) => tracing::warn!("redaction rule {:?} ignored: {e}", r.name),
            }
        }
        Self { rules }
    }

    /// Whether anything would be masked at all.
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// `text` with every enabled rule applied. A rule with a `keep` group
    /// puts that group back in front of the mask.
    pub fn apply(&self, text: &str) -> String {
        let mut out = std::borrow::Cow::Borrowed(text);
        for re in &self.rules {
            let keeps = re.capture_names().any(|n| n == Some("keep"));
            let replacement = if keeps {
                format!("${{keep}}{MASK}")
            } else {
                MASK.to_string()
            };
            out = std::borrow::Cow::Owned(re.replace_all(&out, replacement.as_str()).into_owned());
        }
        out.into_owned()
    }

    /// Whether `text` holds anything this would mask. Used to mark a row
    /// rather than to rewrite it.
    pub fn would_mask(&self, text: &str) -> bool {
        self.rules.iter().any(|re| re.is_match(text))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r() -> Redactor {
        Redactor::compile(&RuleSet::default())
    }

    #[test]
    fn the_default_rules_mask_a_secret_and_keep_what_names_it() {
        let red = r();
        let out =
            red.apply("GET /v1/orders Authorization: Bearer fixture-token-NOT-A-REAL-SECRET-0002");
        assert!(
            out.contains("Authorization: Bearer "),
            "the header survives"
        );
        assert!(
            !out.contains("fixture-token-NOT-A-REAL-SECRET-0002"),
            "the token does not"
        );
        assert!(out.contains(MASK));

        let out = red.apply(r#"{"user":"ada","password":"hunter2","ok":true}"#);
        assert!(!out.contains("hunter2"));
        assert!(out.contains("\"user\":\"ada\""), "the rest is untouched");
        assert!(out.contains("\"ok\":true"));

        assert!(
            !red.apply("key AKIAIOSFODNN7EXAMPLE used")
                .contains("AKIAIOSFODNN7EXAMPLE")
        );
        assert!(
            !red.apply("to ada@example.com at 10:00")
                .contains("ada@example.com")
        );
        let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dBjftJeZ4CVPmB92K27uhbUJU1p1r";
        assert!(!red.apply(&format!("token={jwt}")).contains(jwt));
    }

    #[test]
    fn an_ordinary_line_is_returned_unchanged() {
        let red = r();
        let line = "2026-10-22T14:32:01Z WARN OrderSvc payment timeout after 3000 ms";
        assert_eq!(red.apply(line), line);
        assert!(!red.would_mask(line));
        assert!(red.would_mask("password=hunter2"));
    }

    #[test]
    fn a_rule_that_does_not_compile_is_dropped_not_fatal() {
        let set = RuleSet {
            schema: SCHEMA,
            rules: vec![
                Rule::new("broken", "([unclosed"),
                Rule::new("fine", "secretvalue"),
            ],
        };
        let red = Redactor::compile(&set);
        assert_eq!(red.apply("a secretvalue b"), format!("a {MASK} b"));
    }

    #[test]
    fn rules_round_trip_and_a_missing_file_means_the_defaults() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(RuleSet::load(dir.path()), RuleSet::default());

        let mut set = RuleSet::default();
        set.rules[0].enabled = false;
        set.rules.push(Rule::new("ticket ids", r"TICKET-\d+"));
        set.save(dir.path()).unwrap();
        let back = RuleSet::load(dir.path());
        assert_eq!(back, set);
        assert!(!back.rules[0].enabled);

        // A disabled rule stops masking; the rest keep going.
        let red = Redactor::compile(&back);
        let out =
            red.apply("Authorization: Bearer fixture-token-NOT-A-REAL-SECRET-0003 for TICKET-42");
        assert!(
            out.contains("fixture-token-NOT-A-REAL-SECRET-0003"),
            "the disabled rule is off"
        );
        assert!(!out.contains("TICKET-42"), "the added rule is on");
    }

    /// What Save Filtered… does, checked the way the vault's tests check
    /// the data folder: write the file, then grep it for every secret the
    /// fixture put in. A log export is the one place in this tool where a
    /// secret could land on disk in plaintext.
    #[test]
    fn a_saved_file_does_not_contain_the_secrets_that_went_into_it() {
        let secrets = [
            "fixture-token-NOT-A-REAL-SECRET-0001",
            "hunter2",
            "AKIAIOSFODNN7EXAMPLE",
            "ada.lovelace@example.com",
        ];
        let lines = [
            "14:32:01 INFO  GET /orders Authorization: Bearer fixture-token-NOT-A-REAL-SECRET-0001",
            r#"14:32:02 DEBUG login {"user":"ada","password":"hunter2"}"#,
            "14:32:03 WARN  s3 put failed, key AKIAIOSFODNN7EXAMPLE",
            "14:32:04 INFO  receipt sent to ada.lovelace@example.com",
            "14:32:05 ERROR payment timeout after 3000 ms",
        ];

        let red = r();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("filtered.log");
        let body: String = lines
            .iter()
            .map(|l| format!("{}\n", red.apply(l)))
            .collect();
        std::fs::write(&path, body).unwrap();

        let written = std::fs::read_to_string(&path).unwrap();
        for s in secrets {
            assert!(
                !written.contains(s),
                "{s} reached the exported file in plaintext"
            );
        }
        // The log is still readable: times, levels and the ordinary line
        // all survive, and only the secrets are gone.
        assert!(written.contains("14:32:01 INFO  GET /orders"));
        assert!(written.contains("payment timeout after 3000 ms"));
        assert_eq!(written.lines().count(), lines.len());
    }

    #[test]
    fn a_broken_file_falls_back_to_the_defaults_rather_than_to_nothing() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(RuleSet::file_for(dir.path()), "this is not toml {{{").unwrap();
        let set = RuleSet::load(dir.path());
        assert_eq!(set, RuleSet::default(), "never silently stop masking");
    }
}
