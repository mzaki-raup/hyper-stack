//! Log formats as pure functions (docs/tools/logs.md §6): detection from a
//! sample, one record per line with the timestamp, level, message and
//! fields pulled out, and the continuation rule that glues a stack trace
//! to the line that started it.

use std::sync::LazyLock;

use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use regex::Regex;
use serde::{Deserialize, Serialize};

/// Normalised severity, ordered so `level >= WARN` is a comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Level {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
    Fatal,
}

impl Level {
    pub const ALL: [Level; 6] = [
        Level::Trace,
        Level::Debug,
        Level::Info,
        Level::Warn,
        Level::Error,
        Level::Fatal,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Level::Trace => "TRACE",
            Level::Debug => "DEBUG",
            Level::Info => "INFO",
            Level::Warn => "WARN",
            Level::Error => "ERROR",
            Level::Fatal => "FATAL",
        }
    }

    /// Many spellings into one. This is the single list: `Level::parse`
    /// reads it, and `LEVEL_TOKEN` - the regex that finds a level in a
    /// plain or syslog line - is built from it, so the two can never
    /// disagree about what a level looks like. They had: the parser knew
    /// `WRN`, `DBG`, `INF` and `TRC`, the scanner did not, so a log that
    /// used the short forms showed no level at all (owner's report,
    /// 2026-10-23).
    ///
    /// Syslog's numeric severities are deliberately absent: a bare digit
    /// in prose is not a level, so they are handled by `parse` alone,
    /// where the caller already knows the field is one.
    pub const ALIASES: &'static [(&'static str, Level)] = &[
        ("TRACE", Level::Trace),
        ("TRC", Level::Trace),
        ("VERBOSE", Level::Trace),
        ("FINEST", Level::Trace),
        ("DEBUG", Level::Debug),
        ("DBG", Level::Debug),
        ("FINER", Level::Debug),
        ("FINE", Level::Debug),
        ("INFORMATION", Level::Info),
        ("INFO", Level::Info),
        ("INF", Level::Info),
        ("NOTICE", Level::Info),
        ("WARNING", Level::Warn),
        ("WARN", Level::Warn),
        ("WRN", Level::Warn),
        ("ERROR", Level::Error),
        ("ERR", Level::Error),
        ("SEVERE", Level::Error),
        ("CRITICAL", Level::Fatal),
        ("CRIT", Level::Fatal),
        ("EMERGENCY", Level::Fatal),
        ("EMERG", Level::Fatal),
        ("FATAL", Level::Fatal),
        ("PANIC", Level::Fatal),
        ("ALERT", Level::Fatal),
    ];

    /// Many spellings into one: `WARNING`, `ERR`, `CRIT`, Go's `warn`,
    /// Python's `CRITICAL`, syslog's numeric severities (0-7).
    pub fn parse(token: &str) -> Option<Level> {
        let t = token.trim().trim_matches(|c: char| !c.is_alphanumeric());
        let upper = t.to_ascii_uppercase();
        if let Some((_, l)) = Self::ALIASES.iter().find(|(name, _)| *name == upper) {
            return Some(*l);
        }
        Some(match upper.as_str() {
            "7" => Level::Trace,
            "6" => Level::Debug,
            "5" => Level::Info,
            "4" => Level::Warn,
            "3" => Level::Error,
            "2" | "1" | "0" => Level::Fatal,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Format {
    #[default]
    Auto,
    Plain,
    Json,
    Logfmt,
    Syslog,
    Nginx,
    Postgres,
    /// A JVM thread dump (`jstack`, `kill -3`, a container's crash log):
    /// one record per thread, its stack folded in, with the thread's name,
    /// id, state and locks pulled out as fields (owner's request,
    /// 2026-10-24).
    ThreadDump,
    /// `journalctl -o json`: one object per record with `MESSAGE`,
    /// `PRIORITY`, `__REALTIME_TIMESTAMP` and the unit's fields.
    Journal,
    /// Windows Event Log records as the Logs activity's own PowerShell
    /// follower prints them: `t`, `level`, `provider`, `id`, `record`,
    /// `msg`.
    EventLog,
}

impl Format {
    pub const ALL: [Format; 10] = [
        Format::Auto,
        Format::Plain,
        Format::Json,
        Format::Logfmt,
        Format::Syslog,
        Format::Nginx,
        Format::Postgres,
        Format::ThreadDump,
        Format::Journal,
        Format::EventLog,
    ];
    pub fn label_key(self) -> &'static str {
        match self {
            Format::Auto => "logs-format-auto",
            Format::Plain => "logs-format-plain",
            Format::Json => "logs-format-json",
            Format::Logfmt => "logs-format-logfmt",
            Format::Syslog => "logs-format-syslog",
            Format::Nginx => "logs-format-nginx",
            Format::Postgres => "logs-format-postgres",
            Format::ThreadDump => "logs-format-thread-dump",
            Format::Journal => "logs-format-journal",
            Format::EventLog => "logs-format-eventlog",
        }
    }
    /// JSON and logfmt carry named fields, so the list can show columns.
    pub fn structured(self) -> bool {
        matches!(
            self,
            Format::Json | Format::Logfmt | Format::Journal | Format::EventLog
        )
    }
}

/// How a source spells each level. `WARN`, `warning` and syslog's `4` all
/// mean the same thing, and HyperStack normalises them so a filter and a
/// saved preset mean the same across sources. The screen should still read
/// in the source's own words, so the first spelling each level arrives in
/// is remembered and used for labels (owner's request, 2026-10-23).
///
/// Learning stops once all six are known, so a long log pays for at most
/// six lookups.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LevelAliases([Option<String>; 6]);

impl LevelAliases {
    fn slot(level: Level) -> usize {
        match level {
            Level::Trace => 0,
            Level::Debug => 1,
            Level::Info => 2,
            Level::Warn => 3,
            Level::Error => 4,
            Level::Fatal => 5,
        }
    }

    /// Whether this level's spelling is still unknown, so the caller can
    /// skip looking for one.
    pub fn wants(&self, level: Level) -> bool {
        self.0[Self::slot(level)].is_none()
    }

    /// Remember how this source spells `level`. A token that is only a
    /// number (syslog's severities) is not a name anyone wants on a
    /// filter chip, so it is ignored and the canonical label stands.
    pub fn learn(&mut self, level: Level, token: &str) {
        let t = token.trim();
        if t.is_empty() || t.chars().all(|c| c.is_ascii_digit()) {
            return;
        }
        let slot = Self::slot(level);
        if self.0[slot].is_none() {
            self.0[slot] = Some(t.to_string());
        }
    }

    /// What to call this level on screen: the source's spelling, or the
    /// canonical one when the source has not shown it yet.
    pub fn name(&self, level: Level) -> &str {
        match &self.0[Self::slot(level)] {
            Some(t) => t.as_str(),
            None => level.label(),
        }
    }

    pub fn clear(&mut self) {
        self.0 = Default::default();
    }
}

/// Where the level word sits in `text`, so the list can paint that word
/// in the level's colour whatever the format (owner's request,
/// 2026-10-23). `LEVEL_TOKEN` alone is not enough: it requires the word to
/// follow a space or a bracket, and the commonest shape of all is
/// `level=info`, where it follows an equals sign or a quote.
pub fn level_span(text: &str) -> Option<(std::ops::Range<usize>, Level)> {
    // `level=info`, `severity: "warn"`, `lvl='dbg'` and the like first:
    // the word after a level key is the level even when the same word
    // elsewhere in the line would not be.
    if let Some(c) = LEVEL_ASSIGN.captures(text)
        && let Some(m) = c.get(1)
        && let Some(l) = Level::parse(m.as_str())
    {
        return Some((m.range(), l));
    }
    let c = LEVEL_TOKEN.captures(text)?;
    let m = c.get(1)?;
    Level::parse(m.as_str()).map(|l| (m.range(), l))
}

/// `level=info`, `level: warn`, `"severity":"error"`: a level key, then
/// the word that is its value.
static LEVEL_ASSIGN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?i)(?:level|lvl|severity|loglevel|log_level|levelname)"?\s*[:=]\s*"?'?([A-Za-z]+)"#,
    )
    .expect("level assignment")
});

/// The token in `rec` that named its level, if one can be found: the
/// `level`/`severity`/`lvl` field a structured parser pulled out, else the
/// word in the raw line that `LEVEL_TOKEN` matched.
pub fn level_token(rec: &Record) -> Option<&str> {
    for key in [
        "level",
        "severity",
        "lvl",
        "loglevel",
        "log_level",
        "levelname",
    ] {
        if let Some(v) = rec.field_ci(key)
            && Level::parse(v).is_some()
        {
            return Some(v);
        }
    }
    LEVEL_TOKEN
        .captures(&rec.raw)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str())
}

/// One parsed record: a line and the continuation lines glued to it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Record {
    /// 1-based number of the first line in the source.
    pub line: u64,
    pub raw: String,
    pub ts: Option<DateTime<Utc>>,
    pub level: Option<Level>,
    pub message: String,
    /// Named fields in source order (JSON, logfmt, syslog, nginx).
    pub fields: Vec<(String, String)>,
}

impl Record {
    pub fn field(&self, name: &str) -> Option<&str> {
        self.fields
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    /// A field by a name whose case the writer chose, not the reader:
    /// `level`, `Level` and `LEVEL` are the same key, and so are
    /// `@timestamp` and `@Timestamp`. Used for the well-known keys a
    /// parser looks up on its own (the level, the timestamp, the
    /// message), while [`Record::field`] stays exact for a field the user
    /// named, where the key they picked is the key they meant. Owner's
    /// request, 2026-10-23.
    pub fn field_ci(&self, name: &str) -> Option<&str> {
        self.fields
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

static ISO: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?x)^\s*\[?
        (\d{4}-\d{2}-\d{2}[T\s]\d{2}:\d{2}:\d{2}(?:[.,]\d{1,9})?(?:\s?(?:Z|[+-]\d{2}:?\d{2}))?)
        \]?",
    )
    .unwrap()
});
/// Built from [`Level::ALIASES`], longest spelling first so the
/// alternation prefers `WARNING` over `WARN` and `INFORMATION` over
/// `INFO` rather than stopping at the shorter prefix.
static LEVEL_TOKEN: LazyLock<Regex> = LazyLock::new(|| {
    let mut names: Vec<&str> = Level::ALIASES.iter().map(|(n, _)| *n).collect();
    names.sort_by_key(|n| std::cmp::Reverse(n.len()));
    Regex::new(&format!(
        r"(?i)(?:^|[\s\[\(\|:])({})(?:$|[\s\]\)\|:])",
        names.join("|")
    ))
    .expect("level alternation")
});
static SYSLOG: LazyLock<Regex> = LazyLock::new(|| {
    // RFC 3164: `Oct 22 14:32:01 host app[123]: msg`; RFC 5424:
    // `<13>1 2026-10-22T14:32:01Z host app 123 - - msg`.
    Regex::new(
        r"(?x)^(?:<(?P<pri>\d{1,3})>(?:\d\s)?)?
        (?P<ts>[A-Z][a-z]{2}\s+\d{1,2}\s\d{2}:\d{2}:\d{2}|\d{4}-\d{2}-\d{2}T[\d:.]+(?:Z|[+-]\d{2}:\d{2})?)\s
        (?P<host>\S+)\s
        (?P<app>[^\s\[:]+)(?:\[(?P<pid>\d+)\])?(?:\s\S+\s\S+\s\S+)?:?\s
        (?P<msg>.*)$",
    )
    .unwrap()
});
static NGINX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?x)^(?P<ip>\S+)\s\S+\s(?P<user>\S+)\s\[(?P<ts>[^\]]+)\]\s
        "(?P<method>[A-Z]+)\s(?P<path>\S+)[^"]*"\s(?P<status>\d{3})\s(?P<bytes>\d+|-)
        (?:\s"(?P<referer>[^"]*)"\s"(?P<ua>[^"]*)")?(?:\s(?P<rt>[\d.]+))?"#,
    )
    .unwrap()
});
static POSTGRES: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?x)^(?P<ts>\d{4}-\d{2}-\d{2}\s\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:\s[A-Z]{2,5})?)\s
        (?:\[(?P<pid>\d+)\]\s)?(?:(?P<user>[^@\s]+)@(?P<db>\S+)\s)?
        (?P<level>LOG|ERROR|FATAL|PANIC|WARNING|NOTICE|INFO|DEBUG\d?|STATEMENT|DETAIL|HINT):\s{1,2}
        (?P<msg>.*)$",
    )
    .unwrap()
});
static DURATION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"duration:\s*([\d.]+)\s*ms").unwrap());
static DOCKER_PREFIX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d+Z)\s(.*)$").unwrap());

/// Timestamps in the forms logs use. Naive ones are read as UTC.
pub fn parse_timestamp(text: &str) -> Option<DateTime<Utc>> {
    let t = text.trim().trim_matches(|c| c == '[' || c == ']');
    if let Ok(d) = DateTime::parse_from_rfc3339(t) {
        return Some(d.with_timezone(&Utc));
    }
    let t2 = t.replacen(',', ".", 1);
    for f in [
        "%Y-%m-%dT%H:%M:%S%.f%:z",
        "%Y-%m-%dT%H:%M:%S%.f%z",
        "%Y-%m-%d %H:%M:%S%.f%:z",
        "%Y-%m-%d %H:%M:%S%.f%z",
        "%d/%b/%Y:%H:%M:%S %z",
    ] {
        if let Ok(d) = DateTime::parse_from_str(&t2, f) {
            return Some(d.with_timezone(&Utc));
        }
    }
    for f in [
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y-%m-%d %H:%M:%S%.f UTC",
        "%Y/%m/%d %H:%M:%S%.f",
    ] {
        if let Ok(n) = NaiveDateTime::parse_from_str(&t2, f) {
            return Some(Utc.from_utc_datetime(&n));
        }
    }
    // `Oct 22 14:32:01` (syslog): the current year.
    if let Ok(n) = NaiveDateTime::parse_from_str(
        &format!(
            "{} {}",
            Utc::now().format("%Y"),
            t.split_whitespace().collect::<Vec<_>>().join(" ")
        ),
        "%Y %b %d %H:%M:%S",
    ) {
        return Some(Utc.from_utc_datetime(&n));
    }
    // Epoch seconds or milliseconds.
    if let Ok(n) = t.parse::<i64>() {
        let (secs, nanos) = if n > 100_000_000_000 {
            (n / 1000, ((n % 1000) * 1_000_000) as u32)
        } else {
            (n, 0)
        };
        return DateTime::from_timestamp(secs, nanos);
    }
    if let Ok(f) = t.parse::<f64>()
        && f > 1_000_000_000.0
    {
        return DateTime::from_timestamp(f as i64, ((f.fract()) * 1e9) as u32);
    }
    None
}

/// Guess the format from the first lines.
pub fn detect(sample: &[&str]) -> Format {
    let lines: Vec<&str> = sample
        .iter()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .take(64)
        .collect();
    if lines.is_empty() {
        return Format::Plain;
    }
    let n = lines.len() as f32;
    let count = |f: fn(&str) -> bool| lines.iter().filter(|l| f(l)).count() as f32 / n;
    // A thread dump is recognised by presence, not by proportion: most of
    // its lines are stack frames, so no ratio test would ever pass, but
    // the VM line, a `tid=`/`nid=` thread header and the state line are
    // each unmistakable.
    if lines.iter().any(|l| l.starts_with("Full thread dump"))
        || lines.iter().any(|l| l.contains("java.lang.Thread.State:"))
        || lines
            .iter()
            .any(|l| l.starts_with('"') && (l.contains("tid=") || l.contains("nid=")))
    {
        return Format::ThreadDump;
    }
    // journald's JSON and the Event Log follower's JSON are told from
    // plain JSON lines by the keys only they carry.
    if count(|l| l.starts_with('{') && l.contains("\"__REALTIME_TIMESTAMP\"")) >= 0.6 {
        return Format::Journal;
    }
    if count(|l| l.starts_with('{') && l.contains("\"provider\"") && l.contains("\"record\""))
        >= 0.6
    {
        return Format::EventLog;
    }
    if count(|l| l.starts_with('{') && serde_json::from_str::<serde_json::Value>(l).is_ok()) >= 0.6
    {
        return Format::Json;
    }
    if count(|l| NGINX.is_match(l)) >= 0.6 {
        return Format::Nginx;
    }
    if count(|l| POSTGRES.is_match(l)) >= 0.6 {
        return Format::Postgres;
    }
    if count(|l| SYSLOG.is_match(l)) >= 0.6 {
        return Format::Syslog;
    }
    if count(|l| logfmt_pairs(l).len() >= 2) >= 0.6 {
        return Format::Logfmt;
    }
    Format::Plain
}

/// `key=value key2="quoted value"` pairs; anything else leaves the list
/// short, which is how a plain line fails the logfmt test.
pub fn logfmt_pairs(line: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut rest = line.trim();
    while !rest.is_empty() {
        let Some(eq) = rest.find('=') else { break };
        let key = rest[..eq].trim();
        if key.is_empty() || key.contains(char::is_whitespace) || key.contains('"') {
            break;
        }
        let after = &rest[eq + 1..];
        let (value, next) = if let Some(q) = after.strip_prefix('"') {
            let mut v = String::new();
            let mut chars = q.char_indices();
            let mut end = q.len();
            while let Some((i, c)) = chars.next() {
                match c {
                    '\\' => {
                        if let Some((_, n)) = chars.next() {
                            v.push(match n {
                                'n' => '\n',
                                't' => '\t',
                                other => other,
                            });
                        }
                    }
                    '"' => {
                        end = i + 1;
                        break;
                    }
                    c => v.push(c),
                }
            }
            (v, q[end.min(q.len())..].trim_start())
        } else {
            let end = after.find(char::is_whitespace).unwrap_or(after.len());
            (after[..end].to_string(), after[end..].trim_start())
        };
        out.push((key.to_string(), value));
        rest = next;
    }
    out
}

/// A line that belongs to the record before it: a stack frame, an
/// indented continuation, a `Caused by:`, JSON pretty-printed over lines.
pub fn is_continuation(format: Format, line: &str) -> bool {
    if line.is_empty() {
        return false;
    }
    let trimmed = line.trim_start();
    if trimmed.is_empty() {
        return true;
    }
    if matches!(format, Format::Json | Format::Journal | Format::EventLog) {
        // A JSON line starts a record; anything else continues one.
        return !trimmed.starts_with('{');
    }
    if format == Format::ThreadDump {
        // Only a thread header, a snapshot header or a section title
        // starts a record; every frame, monitor and state line belongs to
        // the thread above it.
        return !(is_thread_header(trimmed)
            || trimmed.starts_with("Full thread dump")
            || ISO.is_match(trimmed)
            || trimmed.to_ascii_lowercase().contains("deadlock")
            || trimmed.starts_with("JNI global ref"));
    }
    if line.starts_with([' ', '\t']) {
        return true;
    }
    trimmed.starts_with("at ")
        || trimmed.starts_with("Caused by:")
        || trimmed.starts_with("... ")
        || trimmed.starts_with("Traceback (most recent call last)")
        || trimmed.starts_with("File \"")
        || trimmed.starts_with("---> ")
        || trimmed.starts_with("--- End of")
        || (format == Format::Postgres && !POSTGRES.is_match(trimmed) && !ISO.is_match(trimmed))
}

/// Parse one (first) line of a record.
pub fn parse(format: Format, line_no: u64, line: &str) -> Record {
    let mut rec = Record {
        line: line_no,
        raw: line.to_string(),
        ..Default::default()
    };
    // A container runtime's timestamp prefix is read and stripped.
    let text = match DOCKER_PREFIX.captures(line) {
        Some(c) if format != Format::Nginx => {
            rec.ts = parse_timestamp(c.get(1).unwrap().as_str());
            c.get(2).unwrap().as_str().to_string()
        }
        _ => line.to_string(),
    };
    match format {
        Format::Json => parse_json(&mut rec, &text),
        Format::Logfmt => parse_logfmt(&mut rec, &text),
        Format::Syslog => parse_syslog(&mut rec, &text),
        Format::Nginx => parse_nginx(&mut rec, &text),
        Format::Postgres => parse_postgres(&mut rec, &text),
        Format::ThreadDump => parse_thread_dump(&mut rec, &text),
        Format::Journal => parse_journal(&mut rec, &text),
        Format::EventLog => parse_eventlog(&mut rec, &text),
        Format::Plain | Format::Auto => parse_plain(&mut rec, &text),
    }
    rec
}

/// `journalctl -o json`: the timestamp is microseconds since the epoch,
/// the level is syslog's priority, the text is `MESSAGE` (an array of
/// bytes when it was not valid UTF-8). The journal's own `__CURSOR` and
/// `__MONOTONIC_TIMESTAMP` are left out of the fields: nobody filters on
/// them, and they would push the unit and the identifier off the end of
/// a pinned-column row.
fn parse_journal(rec: &mut Record, text: &str) {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(text.trim()) else {
        parse_plain(rec, text);
        return;
    };
    let Some(obj) = v.as_object() else {
        parse_plain(rec, text);
        return;
    };
    for (k, v) in obj {
        if k == "__CURSOR" || k == "__MONOTONIC_TIMESTAMP" {
            continue;
        }
        let s = match v {
            serde_json::Value::String(s) => s.clone(),
            serde_json::Value::Array(bytes) if k == "MESSAGE" => {
                let raw: Vec<u8> = bytes
                    .iter()
                    .filter_map(|b| b.as_u64().map(|n| n as u8))
                    .collect();
                String::from_utf8_lossy(&raw).into_owned()
            }
            other => other.to_string(),
        };
        rec.fields.push((k.clone(), s));
    }
    rec.ts = rec
        .field("__REALTIME_TIMESTAMP")
        .and_then(|t| t.parse::<i64>().ok())
        .and_then(DateTime::<Utc>::from_timestamp_micros);
    rec.level = rec
        .field("PRIORITY")
        .and_then(|p| p.parse::<u8>().ok())
        .map(|p| match p {
            0..=2 => Level::Fatal,
            3 => Level::Error,
            4 => Level::Warn,
            5 | 6 => Level::Info,
            _ => Level::Debug,
        });
    rec.message = rec.field("MESSAGE").unwrap_or("").to_string();
}

/// The Event Log follower's JSON: plain JSON with the level in the words
/// `Get-WinEvent` uses, which `Level::parse` does not all know.
fn parse_eventlog(rec: &mut Record, text: &str) {
    parse_json(rec, text);
    if rec.level.is_none() {
        rec.level = match rec.field_ci("level").map(|l| l.to_ascii_lowercase()) {
            Some(l) if l == "information" => Some(Level::Info),
            Some(l) if l == "verbose" => Some(Level::Debug),
            Some(l) if l == "critical" => Some(Level::Fatal),
            Some(l) if l == "warning" => Some(Level::Warn),
            Some(l) if l == "error" => Some(Level::Error),
            _ => None,
        };
    }
}

/// The PowerShell that follows one Windows Event Log as JSON lines: the
/// last `history` records, then every two seconds whatever is newer than
/// the last record id. `Get-WinEvent` is the only portable reader; the
/// output is the shape [`Format::EventLog`] parses. The script travels as
/// `-EncodedCommand` (UTF-16LE base64): run through `cmd /C`, a quoted
/// `-Command` string is handed to PowerShell with its quotes on, which
/// makes it a string to print rather than a script to run.
pub fn eventlog_command(log_name: &str, history: u32) -> String {
    use base64::Engine;
    let log = log_name.trim().replace('\'', "''");
    let script = format!(
        "$ProgressPreference='SilentlyContinue'; $log='{log}'; $last=0; function Emit($e) {{ $script:last=[Math]::Max($script:last,$e.RecordId);          [pscustomobject]@{{t=$e.TimeCreated.ToString('o');level=$e.LevelDisplayName;provider=$e.ProviderName;id=$e.Id;record=$e.RecordId;msg=$e.Message}} | ConvertTo-Json -Compress }};          Get-WinEvent -LogName $log -MaxEvents {history} -ErrorAction SilentlyContinue | Sort-Object RecordId | ForEach-Object {{ Emit $_ }};          while($true) {{ Start-Sleep -Seconds 2;          Get-WinEvent -LogName $log -MaxEvents 500 -ErrorAction SilentlyContinue | Where-Object {{ $_.RecordId -gt $last }} | Sort-Object RecordId | ForEach-Object {{ Emit $_ }} }}"
    );
    let utf16: Vec<u8> = script
        .encode_utf16()
        .flat_map(|u| u.to_le_bytes())
        .collect();
    format!(
        "powershell -NoProfile -NonInteractive -EncodedCommand {}",
        base64::engine::general_purpose::STANDARD.encode(utf16)
    )
}

/// The journalctl that follows one unit as JSON lines.
pub fn journal_command(unit: &str, history: u32) -> String {
    if unit.trim().is_empty() {
        format!("journalctl -o json -n {history} -f")
    } else {
        format!("journalctl -o json -n {history} -f -u {}", unit.trim())
    }
}

/// A thread dump's thread header:
/// `"http-nio-8080-exec-1" #42 daemon prio=5 os_prio=0 cpu=12.34ms
/// elapsed=100.00s tid=0x00007f nid=0x1234 waiting on condition [0x...]`
///
/// Everything after the name is optional and varies by JVM and version,
/// so each piece is read where it appears rather than by position.
static THREAD_HEADER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"^"(?P<name>(?:[^"\\]|\\.)*)"(?P<rest>.*)$"#).unwrap());

/// Whether a line really is a thread header rather than a quoted thread
/// *name* used as a reference. The deadlock section is full of the
/// latter - `"worker-8":`, `which is held by "worker-7"` - and reading
/// those as headers invented extra threads and split the section apart.
pub fn is_thread_header(line: &str) -> bool {
    let Some(c) = THREAD_HEADER.captures(line.trim_end()) else {
        return false;
    };
    let rest = c.name("rest").unwrap().as_str();
    // A header always carries at least one of the JVM's own attributes.
    rest.contains("tid=")
        || rest.contains("nid=")
        || rest.contains("prio=")
        || rest.contains(" daemon")
        || rest.split_whitespace().any(|w| {
            w.strip_prefix('#')
                .is_some_and(|n| n.parse::<u64>().is_ok())
        })
}
/// `java.lang.Thread.State: TIMED_WAITING (on object monitor)`
static THREAD_STATE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*java\.lang\.Thread\.State:\s*(?P<state>[A-Z_]+)(?:\s*\((?P<detail>[^)]*)\))?")
        .unwrap()
});
/// A `key=value` out of a thread header, for the numeric odds and ends.
static HEADER_PAIR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?P<key>[a-z_]+)=(?P<value>[^\s]+)").unwrap());
/// The monitor lines under a frame: what this thread holds and what it
/// wants. These are what a deadlock is read from.
static LOCK_LINE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?x)^\s*-\s*
        (?P<verb>locked|waiting\ to\ lock|waiting\ on|parking\ to\ wait\ for|eliminated)
        \s*<(?P<id>[^>]*)>
        (?:\s*\(a\ (?P<class>[^)]*)\))?",
    )
    .unwrap()
});

/// The states a dump reports, as filterable values. A thread's state is a
/// field rather than a [`Level`], because a waiting thread is not a
/// warning - but **BLOCKED** and a deadlock are what a dump is opened to
/// find, so those do get a level and therefore a colour.
fn parse_thread_dump(rec: &mut Record, text: &str) {
    let trimmed = text.trim_end();
    // The dump's own header lines: the timestamp, the VM line, and the
    // deadlock and JNI sections.
    if let Some(c) = ISO.captures(trimmed)
        && let Some(ts) = parse_timestamp(c.get(1).unwrap().as_str())
    {
        rec.ts = Some(ts);
        rec.message = trimmed.trim().to_string();
        rec.fields.push(("kind".into(), "snapshot".into()));
        return;
    }
    if trimmed.starts_with("Full thread dump") {
        rec.message = trimmed.trim().to_string();
        rec.fields.push(("kind".into(), "snapshot".into()));
        return;
    }
    let lower = trimmed.to_ascii_lowercase();
    if lower.contains("deadlock") {
        rec.level = Some(Level::Error);
        rec.message = trimmed.trim().to_string();
        rec.fields.push(("kind".into(), "deadlock".into()));
        return;
    }
    let Some(c) = THREAD_HEADER
        .captures(trimmed)
        .filter(|_| is_thread_header(trimmed))
    else {
        // A section title ("JNI global refs:"), or a quoted thread name
        // used as a reference rather than as a header.
        rec.message = trimmed.trim().to_string();
        return;
    };
    let name = c.name("name").unwrap().as_str().to_string();
    let rest = c.name("rest").unwrap().as_str();
    rec.message = trimmed.trim().to_string();
    rec.fields.push(("kind".into(), "thread".into()));
    rec.fields.push(("thread".into(), name));
    if rest.contains(" daemon ") || rest.trim_start().starts_with("daemon") {
        rec.fields.push(("daemon".into(), "true".into()));
    }
    if let Some(id) = rest
        .split_whitespace()
        .find_map(|w| w.strip_prefix('#'))
        .filter(|id| !id.is_empty())
    {
        rec.fields.push(("id".into(), id.to_string()));
    }
    for p in HEADER_PAIR.captures_iter(rest) {
        let key = p.name("key").unwrap().as_str();
        // `tid`/`nid` identify the thread; the rest is cost and age.
        if matches!(key, "tid" | "nid" | "prio" | "os_prio" | "cpu" | "elapsed") {
            rec.fields.push((
                key.to_string(),
                p.name("value").unwrap().as_str().to_string(),
            ));
        }
    }
    // The trailing words are the JVM's own description of what the thread
    // is doing ("waiting on condition", "runnable", "in Object.wait()"),
    // which is kept as `activity`; the authoritative `state` arrives on
    // the next line.
    let described = match rest.find('[') {
        Some(at) => &rest[..at],
        None => rest,
    };
    if let Some(word) = tail_words(described) {
        rec.fields.push(("activity".into(), word));
    }
}

/// The JVM's description at the end of a thread header: the words after
/// the last `key=value`, which is where "waiting on condition" and
/// "in Object.wait()" sit.
fn tail_words(rest: &str) -> Option<String> {
    let start = HEADER_PAIR
        .find_iter(rest)
        .last()
        .map(|m| m.end())
        .unwrap_or(0);
    let tail = rest[start..].trim();
    (!tail.is_empty()).then(|| tail.to_string())
}

/// A continuation line of a thread dump record, folded into the thread it
/// belongs to: the state line and the monitor lines carry the fields a
/// dump is actually searched by, and they appear *below* the header.
fn absorb_thread_dump_line(rec: &mut Record, line: &str) {
    if let Some(c) = THREAD_STATE.captures(line) {
        let state = c.name("state").unwrap().as_str().to_string();
        // BLOCKED is the one state worth colouring: it is a thread that
        // cannot proceed, which is why someone takes a dump.
        if state == "BLOCKED" {
            rec.level = Some(Level::Error);
        }
        set_field(rec, "state", &state);
        if let Some(d) = c.name("detail") {
            set_field(rec, "state_detail", d.as_str());
        }
        return;
    }
    if let Some(c) = LOCK_LINE.captures(line) {
        let verb = c.name("verb").unwrap().as_str();
        let id = c.name("id").unwrap().as_str();
        let class = c.name("class").map(|m| m.as_str()).unwrap_or("");
        let key = match verb {
            "locked" => "locked",
            "eliminated" => return,
            _ => "waiting_for",
        };
        let value = if class.is_empty() {
            id.to_string()
        } else {
            format!("{id} ({class})")
        };
        append_field(rec, key, &value);
        return;
    }
    let trimmed = line.trim();
    if let Some(frame) = trimmed.strip_prefix("at ") {
        // The top frame is what a thread is doing right now, and the one
        // thing worth having without opening the record.
        if rec.field("top").is_none() {
            set_field(rec, "top", frame);
        }
        let n: u32 = rec
            .field("frames")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        set_field(rec, "frames", &(n + 1).to_string());
    }
}

/// Replace a field's value, or add it.
fn set_field(rec: &mut Record, key: &str, value: &str) {
    match rec.fields.iter_mut().find(|(k, _)| k == key) {
        Some((_, v)) => *v = value.to_string(),
        None => rec.fields.push((key.to_string(), value.to_string())),
    }
}

/// Add to a field that may hold several values (a thread can hold more
/// than one monitor).
fn append_field(rec: &mut Record, key: &str, value: &str) {
    match rec.fields.iter_mut().find(|(k, _)| k == key) {
        Some((_, v)) => {
            v.push_str(", ");
            v.push_str(value);
        }
        None => rec.fields.push((key.to_string(), value.to_string())),
    }
}

/// One link of a deadlock cycle: `waiter` cannot proceed until `holder`
/// lets go of `monitor`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeadlockEdge {
    pub waiter: String,
    pub monitor: String,
    pub holder: String,
}

/// `"worker-8":` - the thread an entry of the deadlock section is about.
static DEADLOCK_WAITER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"^\s*"(?P<name>(?:[^"\\]|\\.)*)"\s*:"#).unwrap());
/// `waiting to lock monitor 0x00007f8a1c0d0000,`
static DEADLOCK_MONITOR: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"waiting\s+(?:to\s+lock|for\s+ownable\s+synchronizer)\s+(?:monitor\s+)?(?P<id>0x[0-9a-fA-F]+)")
        .unwrap()
});
/// `which is held by "worker-7"`
static DEADLOCK_HOLDER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"which\s+is\s+held\s+by\s+"(?P<name>(?:[^"\\]|\\.)*)""#).unwrap()
});

/// The cycle a dump's deadlock section describes, as the edges between the
/// threads it names (owner's request, 2026-10-24). The section is one
/// record, so this reads its folded text:
///
/// ```text
/// Found one Java-level deadlock:
/// =============================
/// "worker-8":
///   waiting to lock monitor 0x00007f8a1c0d0000,
///   which is held by "worker-7"
/// ```
///
/// An entry is only reported once all three parts are known, so a
/// section this does not recognise yields nothing rather than a
/// half-read edge.
pub fn deadlock_cycle(raw: &str) -> Vec<DeadlockEdge> {
    let mut out = Vec::new();
    let mut waiter: Option<String> = None;
    let mut monitor: Option<String> = None;
    for line in raw.lines() {
        if let Some(c) = DEADLOCK_WAITER.captures(line) {
            // A new entry begins; anything half-read is abandoned.
            waiter = Some(c.name("name").unwrap().as_str().to_string());
            monitor = None;
            continue;
        }
        if let Some(c) = DEADLOCK_MONITOR.captures(line) {
            monitor = Some(c.name("id").unwrap().as_str().to_string());
        }
        if let Some(c) = DEADLOCK_HOLDER.captures(line)
            && let Some(w) = &waiter
        {
            out.push(DeadlockEdge {
                waiter: w.clone(),
                monitor: monitor.clone().unwrap_or_default(),
                holder: c.name("name").unwrap().as_str().to_string(),
            });
            waiter = None;
            monitor = None;
        }
    }
    out
}

/// Fold a continuation line into the record above it, pulling out any
/// fields that line carries. Formats other than a thread dump keep the
/// old behaviour of simply gluing the text on.
pub fn absorb_continuation(format: Format, rec: &mut Record, line: &str) {
    if format == Format::ThreadDump {
        absorb_thread_dump_line(rec, line);
    }
}

fn parse_plain(rec: &mut Record, text: &str) {
    let mut rest = text;
    if let Some(c) = ISO.captures(text)
        && let Some(ts) = parse_timestamp(c.get(1).unwrap().as_str())
    {
        rec.ts = rec.ts.or(Some(ts));
        rest = &text[c.get(0).unwrap().end()..];
    }
    if let Some(c) = LEVEL_TOKEN.captures(rest) {
        rec.level = Level::parse(c.get(1).unwrap().as_str());
    }
    rec.message = rest.trim().to_string();
    absorb_embedded_logfmt(rec);
}

fn parse_json(rec: &mut Record, text: &str) {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(text.trim()) else {
        parse_plain(rec, text);
        return;
    };
    let Some(obj) = v.as_object() else {
        parse_plain(rec, text);
        return;
    };
    for (k, v) in obj {
        let s = match v {
            serde_json::Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        rec.fields.push((k.clone(), s));
    }
    for k in [
        "ts",
        "time",
        "@timestamp",
        "timestamp",
        "t",
        "date",
        "datetime",
    ] {
        if let Some(v) = rec.field_ci(k) {
            rec.ts = parse_timestamp(v);
            if rec.ts.is_some() {
                break;
            }
        }
    }
    for k in [
        "level",
        "severity",
        "lvl",
        "loglevel",
        "log.level",
        "levelname",
        "log_level",
        "@l",
    ] {
        if let Some(v) = rec.field_ci(k) {
            rec.level = Level::parse(v);
            if rec.level.is_some() {
                break;
            }
        }
    }
    rec.message = ["msg", "message", "text", "event", "log", "@m"]
        .iter()
        .find_map(|k| rec.field_ci(k).map(str::to_string))
        .unwrap_or_else(|| text.trim().to_string());
}

fn parse_logfmt(rec: &mut Record, text: &str) {
    let pairs = logfmt_pairs(text);
    if pairs.len() < 2 {
        parse_plain(rec, text);
        return;
    }
    rec.fields = pairs;
    for k in ["ts", "time", "t", "timestamp", "date", "datetime"] {
        if let Some(v) = rec.field_ci(k) {
            rec.ts = parse_timestamp(v);
            if rec.ts.is_some() {
                break;
            }
        }
    }
    for k in [
        "level",
        "lvl",
        "severity",
        "loglevel",
        "log_level",
        "levelname",
    ] {
        if let Some(v) = rec.field_ci(k) {
            rec.level = Level::parse(v);
            if rec.level.is_some() {
                break;
            }
        }
    }
    rec.message = ["msg", "message", "text", "event"]
        .iter()
        .find_map(|k| rec.field_ci(k).map(str::to_string))
        .unwrap_or_else(|| text.trim().to_string());
}

fn parse_syslog(rec: &mut Record, text: &str) {
    let Some(c) = SYSLOG.captures(text) else {
        parse_plain(rec, text);
        return;
    };
    let g = |n: &str| {
        c.name(n)
            .map(|m| m.as_str().to_string())
            .unwrap_or_default()
    };
    rec.ts = parse_timestamp(&g("ts"));
    if let Some(pri) = c.name("pri").and_then(|m| m.as_str().parse::<u32>().ok()) {
        rec.level = Level::parse(&(pri % 8).to_string());
        rec.fields.push(("facility".into(), (pri / 8).to_string()));
    }
    rec.fields.push(("host".into(), g("host")));
    rec.fields.push(("app".into(), g("app")));
    if c.name("pid").is_some() {
        rec.fields.push(("pid".into(), g("pid")));
    }
    rec.message = g("msg");
    absorb_embedded_logfmt(rec);
    if rec.level.is_none()
        && let Some(l) = LEVEL_TOKEN.captures(&rec.message)
    {
        rec.level = Level::parse(l.get(1).unwrap().as_str());
    }
}

/// A syslog or plain line whose payload is `key=value` pairs: what every
/// Go daemon under systemd writes, which is what `journalctl -f -u docker`
/// hands back.
///
/// ```text
/// Oct 23 09:00:01 host dockerd[123]: time="…" level=info msg="API listen"
/// ```
///
/// The pairs become fields, so `level=warn` colours the row and
/// `level >= WARN` filters it, and `msg` becomes the message so the list
/// shows the sentence rather than the envelope. Owner's report,
/// 2026-10-23.
fn absorb_embedded_logfmt(rec: &mut Record) {
    let pairs = logfmt_pairs(&rec.message);
    // Two pairs is the bar the detector uses; one `a=b` in prose is not
    // logfmt.
    if pairs.len() < 2 {
        return;
    }
    let mut message = None;
    for (k, v) in pairs {
        match k.to_ascii_lowercase().as_str() {
            "level" | "severity" | "lvl" | "loglevel" | "log_level" | "levelname"
                if rec.level.is_none() =>
            {
                rec.level = Level::parse(&v);
                rec.fields.push((k, v));
            }
            "msg" | "message" if message.is_none() => message = Some(v),
            "time" | "ts" | "timestamp" if rec.ts.is_none() => {
                rec.ts = parse_timestamp(&v);
                rec.fields.push((k, v));
            }
            _ => rec.fields.push((k, v)),
        }
    }
    if let Some(m) = message {
        rec.message = m;
    }
}

fn parse_nginx(rec: &mut Record, text: &str) {
    let Some(c) = NGINX.captures(text) else {
        parse_plain(rec, text);
        return;
    };
    let g = |n: &str| {
        c.name(n)
            .map(|m| m.as_str().to_string())
            .unwrap_or_default()
    };
    rec.ts = parse_timestamp(&g("ts"));
    for k in [
        "ip", "user", "method", "path", "status", "bytes", "referer", "ua", "rt",
    ] {
        if let Some(m) = c.name(k) {
            let key = match k {
                "rt" => "request_time",
                other => other,
            };
            rec.fields.push((key.into(), m.as_str().to_string()));
        }
    }
    let status: u16 = g("status").parse().unwrap_or(0);
    rec.level = Some(match status {
        500.. => Level::Error,
        400..=499 => Level::Warn,
        _ => Level::Info,
    });
    rec.message = format!("{} {} {}", g("method"), g("path"), g("status"));
}

fn parse_postgres(rec: &mut Record, text: &str) {
    let Some(c) = POSTGRES.captures(text) else {
        parse_plain(rec, text);
        return;
    };
    let g = |n: &str| {
        c.name(n)
            .map(|m| m.as_str().to_string())
            .unwrap_or_default()
    };
    rec.ts = parse_timestamp(&g("ts"));
    for k in ["pid", "user", "db"] {
        if c.name(k).is_some() {
            rec.fields.push((k.into(), g(k)));
        }
    }
    let level = g("level");
    rec.level = Some(match level.as_str() {
        "ERROR" => Level::Error,
        "FATAL" | "PANIC" => Level::Fatal,
        "WARNING" => Level::Warn,
        l if l.starts_with("DEBUG") => Level::Debug,
        _ => Level::Info,
    });
    rec.fields.push(("kind".into(), level));
    rec.message = g("msg");
    if let Some(d) = DURATION.captures(&rec.message) {
        rec.fields
            .push(("duration_ms".into(), d.get(1).unwrap().as_str().to_string()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_from_many_spellings() {
        assert_eq!(Level::parse("WARNING"), Some(Level::Warn));
        assert_eq!(Level::parse("err"), Some(Level::Error));
        assert_eq!(Level::parse("CRIT"), Some(Level::Fatal));
        assert_eq!(Level::parse("[info]"), Some(Level::Info));
        assert_eq!(Level::parse("3"), Some(Level::Error));
        assert_eq!(Level::parse("hello"), None);
        assert!(Level::Error > Level::Warn && Level::Warn > Level::Info);
    }

    #[test]
    fn timestamps_in_the_usual_shapes() {
        for t in [
            "2026-10-22T14:32:01.118Z",
            "2026-10-22 14:32:01,118",
            "2026-10-22T14:32:01+02:00",
            "[2026-10-22 14:32:01]",
            "22/Oct/2026:14:32:01 +0000",
            "1761143521",
            "1761143521118",
        ] {
            assert!(parse_timestamp(t).is_some(), "{t}");
        }
        assert!(parse_timestamp("Oct 22 14:32:01").is_some());
        assert!(parse_timestamp("not a time").is_none());
        assert_eq!(
            parse_timestamp("2026-10-22T14:32:01+02:00").unwrap(),
            parse_timestamp("2026-10-22T12:32:01Z").unwrap()
        );
    }

    #[test]
    fn plain_lines_yield_time_level_and_message() {
        let r = parse(
            Format::Plain,
            1,
            "2026-10-22 14:32:01.118  WARN  OrderSvc payment timeout",
        );
        assert!(r.ts.is_some());
        assert_eq!(r.level, Some(Level::Warn));
        assert_eq!(r.message, "WARN  OrderSvc payment timeout");
        let r = parse(Format::Plain, 2, "just text");
        assert!(r.ts.is_none() && r.level.is_none());
        assert_eq!(r.message, "just text");
    }

    #[test]
    fn json_lines_expose_fields_and_normalise_level() {
        let line = r#"{"ts":"2026-10-22T14:32:01.120Z","level":"error","service":"OrderSvc","msg":"boom","n":3}"#;
        let r = parse(Format::Json, 1, line);
        assert_eq!(r.level, Some(Level::Error));
        assert_eq!(r.message, "boom");
        assert_eq!(r.field("service"), Some("OrderSvc"));
        assert_eq!(r.field("n"), Some("3"));
        assert!(r.ts.is_some());
        assert_eq!(detect(&[line, line, "{\"a\":1}"]), Format::Json);
    }

    #[test]
    fn logfmt_pairs_and_quotes() {
        let line = r#"time=2026-10-22T14:32:01Z level=warn msg="redis timeout" attempt=3"#;
        assert_eq!(logfmt_pairs(line).len(), 4);
        let r = parse(Format::Logfmt, 1, line);
        assert_eq!(r.level, Some(Level::Warn));
        assert_eq!(r.message, "redis timeout");
        assert_eq!(r.field("attempt"), Some("3"));
        assert_eq!(detect(&[line, line, line]), Format::Logfmt);
        assert_eq!(detect(&["plain words here", "more words"]), Format::Plain);
    }

    #[test]
    fn syslog_nginx_and_postgres() {
        let s = "<13>Oct 22 14:32:01 web-01 sshd[1234]: Accepted publickey for dev";
        let r = parse(Format::Syslog, 1, s);
        assert_eq!(r.field("host"), Some("web-01"));
        assert_eq!(r.field("app"), Some("sshd"));
        assert_eq!(r.field("pid"), Some("1234"));
        assert_eq!(r.level, Some(Level::Info)); // 13 % 8 = 5 = notice
        assert_eq!(r.message, "Accepted publickey for dev");
        assert_eq!(detect(&[s, s]), Format::Syslog);

        let n = r#"10.0.0.7 - - [22/Oct/2026:14:32:01 +0000] "GET /api/orders?x=1 HTTP/1.1" 502 193 "-" "curl/8.0" 0.512"#;
        let r = parse(Format::Nginx, 1, n);
        assert_eq!(r.field("status"), Some("502"));
        assert_eq!(r.field("path"), Some("/api/orders?x=1"));
        assert_eq!(r.field("request_time"), Some("0.512"));
        assert_eq!(r.level, Some(Level::Error));
        assert_eq!(detect(&[n, n]), Format::Nginx);

        let p = "2026-10-22 14:32:01.118 UTC [4321] app@shop LOG:  duration: 812.4 ms  statement: SELECT 1";
        let r = parse(Format::Postgres, 1, p);
        assert_eq!(r.field("pid"), Some("4321"));
        assert_eq!(r.field("db"), Some("shop"));
        assert_eq!(r.field("duration_ms"), Some("812.4"));
        assert_eq!(r.level, Some(Level::Info));
        assert_eq!(detect(&[p, p]), Format::Postgres);
    }

    #[test]
    fn continuation_lines_belong_to_the_record_before() {
        assert!(is_continuation(
            Format::Plain,
            "    at com.shop.OrderSvc.charge(OrderSvc.java:212)"
        ));
        assert!(is_continuation(
            Format::Plain,
            "Caused by: java.io.IOException"
        ));
        assert!(is_continuation(Format::Json, "  \"nested\": 1"));
        assert!(!is_continuation(
            Format::Plain,
            "2026-10-22 14:32:03 INFO next"
        ));
        assert!(!is_continuation(Format::Json, "{\"a\":1}"));
    }

    #[test]
    fn docker_prefix_is_read_then_dropped() {
        let r = parse(
            Format::Plain,
            1,
            "2026-10-22T14:32:01.118000000Z INFO started",
        );
        assert!(r.ts.is_some());
        assert_eq!(r.message, "INFO started");
        assert_eq!(r.level, Some(Level::Info));
    }

    /// `journalctl -f -u docker`: a syslog envelope wrapping logfmt, which
    /// is what every Go daemon under systemd writes. The level has to come
    /// out of `level=` or the row has no colour and no level filter, and
    /// the message has to be the sentence rather than the envelope.
    /// Owner's report, 2026-10-23.
    #[test]
    fn journalctl_lines_give_up_their_level_and_message() {
        let line = r#"Oct 23 09:00:01 web-01 dockerd[1234]: time="2026-10-23T09:00:01.123456789Z" level=warning msg="Health check for container 9f2 failed" container=9f2"#;
        let f = detect(&[line]);
        let r = parse(f, 1, line);
        assert_eq!(r.level, Some(Level::Warn), "level= decides the level");
        assert_eq!(r.message, "Health check for container 9f2 failed");
        assert!(r.ts.is_some(), "the inner timestamp is preferred");
        assert_eq!(r.field("container"), Some("9f2"));
        assert_eq!(r.field("app"), Some("dockerd"), "the envelope is kept too");

        // An error line colours red by the same route.
        let err = r#"Oct 23 09:00:02 web-01 dockerd[1234]: time="2026-10-23T09:00:02Z" level=error msg="failed to start"#;
        assert_eq!(parse(detect(&[err]), 2, err).level, Some(Level::Error));

        // Prose with one equals sign in it is not logfmt and is left alone.
        let prose =
            "Oct 23 09:00:03 web-01 sshd[9]: Accepted publickey for dev from 10.0.0.2 port x=1";
        let p = parse(Format::Syslog, 3, prose);
        assert!(p.message.starts_with("Accepted publickey"));
    }

    /// Every spelling the parser knows has to be findable in a plain line
    /// too. The two were written out separately and drifted: `WRN`,
    /// `DBG`, `INF` and `TRC` were parsed but never found, so a log using
    /// the short forms showed no level at all. Owner's report,
    /// 2026-10-23.
    #[test]
    fn every_level_alias_is_both_parsed_and_found_in_a_plain_line() {
        for (name, want) in Level::ALIASES {
            assert_eq!(Level::parse(name), Some(*want), "{name} parses");
            assert_eq!(
                Level::parse(&name.to_lowercase()),
                Some(*want),
                "{name} parses in lower case"
            );
            // Found in the middle of an ordinary line, in either case.
            for spelled in [name.to_string(), name.to_lowercase()] {
                let line = format!("2026-10-23T09:00:01Z {spelled} OrderSvc something happened");
                assert_eq!(
                    parse(Format::Plain, 1, &line).level,
                    Some(*want),
                    "{spelled} is found in a plain line"
                );
            }
            // And in brackets, which is how most Java and .NET logs write it.
            let line = format!("09:00:01 [{name}] OrderSvc something happened");
            assert_eq!(
                parse(Format::Plain, 1, &line).level,
                Some(*want),
                "[{name}] is found"
            );
        }
    }

    /// The longer spelling wins, or `WARNING` reads as `WARN` plus stray
    /// letters and `INFORMATION` as `INFO`.
    #[test]
    fn the_longest_spelling_wins() {
        assert_eq!(
            parse(Format::Plain, 1, "09:00:01 WARNING disk is filling").level,
            Some(Level::Warn)
        );
        assert_eq!(
            parse(Format::Plain, 1, "09:00:01 INFORMATION started").level,
            Some(Level::Info)
        );
        // A word that merely starts with a level name is not one.
        assert_eq!(
            parse(Format::Plain, 1, "09:00:01 WARNINGS were raised").level,
            None
        );
        assert_eq!(
            parse(Format::Plain, 1, "09:00:01 INFORMATIONAL note").level,
            None
        );
    }

    /// Syslog's numeric severities are a field, not a word: a bare digit
    /// in prose must never be read as a level.
    #[test]
    fn numbers_are_levels_only_where_a_number_means_one() {
        assert_eq!(Level::parse("4"), Some(Level::Warn));
        assert_eq!(Level::parse("0"), Some(Level::Fatal));
        assert_eq!(
            parse(Format::Plain, 1, "09:00:01 retried 4 times").level,
            None,
            "a digit in prose is not a level"
        );
    }

    /// The writer chooses the case of a key, not the reader. Serilog
    /// writes `@l`, .NET writes `LogLevel`, Python's JSON formatters write
    /// `levelname`, and plenty write `Level`. All of them are the level.
    /// Owner's request, 2026-10-23.
    #[test]
    fn a_level_key_is_found_whatever_its_case_or_spelling() {
        let cases = [
            r#"{"ts":"2026-10-23T09:00:01Z","Level":"Warning","Message":"disk"}"#,
            r#"{"ts":"2026-10-23T09:00:01Z","LEVEL":"warn","msg":"disk"}"#,
            r#"{"ts":"2026-10-23T09:00:01Z","LogLevel":"WRN","msg":"disk"}"#,
            r#"{"ts":"2026-10-23T09:00:01Z","levelname":"WARNING","message":"disk"}"#,
            r#"{"@t":"2026-10-23T09:00:01Z","@l":"Warning","@m":"disk"}"#,
        ];
        for line in cases {
            let r = parse(Format::Json, 1, line);
            assert_eq!(r.level, Some(Level::Warn), "{line}");
            assert_eq!(r.message, "disk", "{line}");
        }

        // logfmt, the same.
        for line in [
            r#"Time=2026-10-23T09:00:01Z Level=warning Msg="disk""#,
            r#"TS=2026-10-23T09:00:01Z LOGLEVEL=wrn MESSAGE="disk""#,
        ] {
            let r = parse(Format::Logfmt, 1, line);
            assert_eq!(r.level, Some(Level::Warn), "{line}");
            assert_eq!(r.message, "disk", "{line}");
        }

        // And inside a syslog envelope, which is the journalctl shape.
        let line =
            r#"Oct 23 09:00:01 web-01 app[1]: Time=2026-10-23T09:00:01Z LEVEL=Wrn Msg="disk""#;
        assert_eq!(parse(Format::Syslog, 1, line).level, Some(Level::Warn));

        // A field the user named keeps its exact case, because the key
        // they picked is the key they meant.
        let r = parse(Format::Json, 1, r#"{"Service":"orders","level":"info"}"#);
        assert_eq!(r.field("Service"), Some("orders"));
        assert_eq!(r.field("service"), None, "exact lookup stays exact");
        assert_eq!(r.field_ci("service"), Some("orders"));
    }

    /// The level word is painted in the level's colour wherever it sits,
    /// and `level=info` is the commonest place of all. Owner's report,
    /// 2026-10-23.
    #[test]
    fn the_level_word_is_located_in_every_shape_a_line_writes_it() {
        let cases = [
            (r#"time="..." level=info msg="up""#, "info", Level::Info),
            (
                r#"time="..." level=warning msg="x""#,
                "warning",
                Level::Warn,
            ),
            (r#"{"level":"error","msg":"x"}"#, "error", Level::Error),
            ("ts=1 lvl=dbg msg=x", "dbg", Level::Debug),
            ("severity: CRITICAL something", "CRITICAL", Level::Fatal),
            (
                "2026-10-23 09:00:01 WARN OrderSvc slow",
                "WARN",
                Level::Warn,
            ),
            ("09:00:01 [Trace] entering", "Trace", Level::Trace),
        ];
        for (line, word, want) in cases {
            let (span, got) = level_span(line).unwrap_or_else(|| panic!("no level in {line}"));
            assert_eq!(got, want, "{line}");
            assert_eq!(&line[span], word, "{line}");
        }
        // A line with no level has no span to paint.
        assert!(level_span("just some text about levels of water").is_none());
        // The level key wins over a level word elsewhere in the line.
        let line = "msg=\"ERROR was logged earlier\" level=info";
        let (span, got) = level_span(line).unwrap();
        assert_eq!(got, Level::Info);
        assert_eq!(&line[span], "info");
    }
}

#[cfg(test)]
mod depth_tests {
    use super::*;

    #[test]
    fn journal_json_becomes_a_record() {
        let line = r#"{"__CURSOR":"s=1","__REALTIME_TIMESTAMP":"1700000000123456","__MONOTONIC_TIMESTAMP":"1","PRIORITY":"3","_SYSTEMD_UNIT":"nginx.service","SYSLOG_IDENTIFIER":"nginx","MESSAGE":"upstream timed out"}"#;
        let r = parse(Format::Journal, 1, line);
        assert_eq!(r.level, Some(Level::Error));
        assert_eq!(r.message, "upstream timed out");
        assert_eq!(r.ts.map(|t| t.timestamp()), Some(1_700_000_000));
        assert_eq!(r.field("_SYSTEMD_UNIT"), Some("nginx.service"));
        assert!(r.field("__CURSOR").is_none());
        assert_eq!(detect(&[line, line, line]), Format::Journal);
        let bytes =
            r#"{"__REALTIME_TIMESTAMP":"1700000000000000","PRIORITY":"6","MESSAGE":[104,105]}"#;
        assert_eq!(parse(Format::Journal, 1, bytes).message, "hi");
        assert!(is_continuation(Format::Journal, "  at line"));
    }

    #[test]
    fn eventlog_json_becomes_a_record() {
        let line = r#"{"t":"2026-10-30T10:00:00.0000000+08:00","level":"Information","provider":"Service Control Manager","id":7036,"record":12345,"msg":"The Print Spooler service entered the running state."}"#;
        let r = parse(Format::EventLog, 1, line);
        assert_eq!(r.level, Some(Level::Info));
        assert!(r.ts.is_some());
        assert_eq!(r.field("provider"), Some("Service Control Manager"));
        assert_eq!(detect(&[line, line, line]), Format::EventLog);
        let crit = r#"{"t":"2026-10-30T10:00:00Z","level":"Critical","provider":"x","id":1,"record":2,"msg":"m"}"#;
        assert_eq!(parse(Format::EventLog, 1, crit).level, Some(Level::Fatal));
    }

    #[test]
    fn follower_commands_are_built() {
        assert_eq!(
            journal_command("nginx", 500),
            "journalctl -o json -n 500 -f -u nginx"
        );
        assert_eq!(journal_command("  ", 10), "journalctl -o json -n 10 -f");
        let c = eventlog_command("Application", 300);
        assert!(c.starts_with("powershell -NoProfile -NonInteractive -EncodedCommand "));
        use base64::Engine;
        let b64 = c.rsplit(' ').next().unwrap();
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(b64)
            .unwrap();
        let units: Vec<u16> = bytes
            .chunks(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        let script = String::from_utf16(&units).unwrap();
        assert!(script.contains("$log='Application'"));
        assert!(script.contains("-MaxEvents 300"));
    }
}
