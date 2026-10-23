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
}

impl Format {
    pub const ALL: [Format; 7] = [
        Format::Auto,
        Format::Plain,
        Format::Json,
        Format::Logfmt,
        Format::Syslog,
        Format::Nginx,
        Format::Postgres,
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
        }
    }
    /// JSON and logfmt carry named fields, so the list can show columns.
    pub fn structured(self) -> bool {
        matches!(self, Format::Json | Format::Logfmt)
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
    if format == Format::Json {
        // A JSON line starts a record; anything else continues one.
        return !trimmed.starts_with('{');
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
        Format::Plain | Format::Auto => parse_plain(&mut rec, &text),
    }
    rec
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
