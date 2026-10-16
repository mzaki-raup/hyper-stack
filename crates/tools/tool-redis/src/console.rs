//! The console of docs/tools/redis.md §6: a command line split the way
//! `redis-cli` splits it, a guard class for every command, and a renderer
//! that turns a RESP reply into rows the UI can draw.

/// How dangerous a command is. The tool refuses what a read-only profile or a
/// protected environment must not run and asks before the rest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    /// Reads: run without asking.
    Read,
    /// Writes one key or a few: refused on a read-only profile.
    Write,
    /// Asks first, and on a protected environment asks for the name typed.
    Dangerous,
    /// Blocks the session or floods it; allowed with a warning.
    Blocking,
}

impl Class {
    pub fn writes(self) -> bool {
        !matches!(self, Class::Read)
    }
}

/// Commands that wipe or reconfigure the server, or walk the whole keyspace.
const DANGEROUS: &[&str] = &[
    "FLUSHALL",
    "FLUSHDB",
    "KEYS",
    "DEBUG",
    "SHUTDOWN",
    "SWAPDB",
    "REPLICAOF",
    "SLAVEOF",
    "FAILOVER",
    "MIGRATE",
    "RESET",
    "SCRIPT",
    "FUNCTION",
    "ACL",
    "CLUSTER",
    "BGREWRITEAOF",
    "BGSAVE",
    "SAVE",
    "LASTSAVE",
];

/// Commands that hold the connection open or stream for ever.
const BLOCKING: &[&str] = &[
    "MONITOR",
    "SUBSCRIBE",
    "PSUBSCRIBE",
    "SSUBSCRIBE",
    "BLPOP",
    "BRPOP",
    "BLMOVE",
    "BLMPOP",
    "BZPOPMIN",
    "BZPOPMAX",
    "BZMPOP",
    "WAIT",
    "XREAD",
    "XREADGROUP",
];

/// Read-only commands. Everything not listed here and not dangerous is
/// treated as a write, so a new command is guarded rather than waved through.
const READS: &[&str] = &[
    "GET",
    "GETRANGE",
    "STRLEN",
    "EXISTS",
    "TYPE",
    "TTL",
    "PTTL",
    "EXPIRETIME",
    "PEXPIRETIME",
    "SCAN",
    "HSCAN",
    "SSCAN",
    "ZSCAN",
    "RANDOMKEY",
    "DBSIZE",
    "HGET",
    "HMGET",
    "HGETALL",
    "HKEYS",
    "HVALS",
    "HLEN",
    "HEXISTS",
    "HSTRLEN",
    "HRANDFIELD",
    "LRANGE",
    "LLEN",
    "LINDEX",
    "LPOS",
    "SMEMBERS",
    "SCARD",
    "SISMEMBER",
    "SMISMEMBER",
    "SRANDMEMBER",
    "SINTER",
    "SUNION",
    "SDIFF",
    "ZRANGE",
    "ZRANGEBYSCORE",
    "ZRANGEBYLEX",
    "ZREVRANGE",
    "ZSCORE",
    "ZMSCORE",
    "ZCARD",
    "ZCOUNT",
    "ZRANK",
    "ZREVRANK",
    "ZRANDMEMBER",
    "ZLEXCOUNT",
    "XRANGE",
    "XREVRANGE",
    "XLEN",
    "XINFO",
    "XPENDING",
    "GETBIT",
    "BITCOUNT",
    "BITPOS",
    "PFCOUNT",
    "GEOPOS",
    "GEODIST",
    "GEOSEARCH",
    "OBJECT",
    "MEMORY",
    "INFO",
    "PING",
    "ECHO",
    "TIME",
    "COMMAND",
    "CONFIG",
    "CLIENT",
    "DUMP",
    "LOLWUT",
    "SELECT",
    "SLOWLOG",
    "LATENCY",
    "JSON.GET",
    "JSON.TYPE",
    "JSON.STRLEN",
    "JSON.OBJKEYS",
    "JSON.ARRLEN",
    "MGET",
    "SUBSTR",
    "SINTERCARD",
    "TOUCH",
    "WAITAOF",
    "DEBUG.JMAP",
];

/// Classify a command by its name (and its first argument where that decides:
/// `CONFIG GET` reads, `CONFIG SET` does not).
pub fn classify(args: &[String]) -> Class {
    let Some(name) = args.first() else {
        return Class::Read;
    };
    let upper = name.to_ascii_uppercase();
    let sub = args
        .get(1)
        .map(|s| s.to_ascii_uppercase())
        .unwrap_or_default();
    match upper.as_str() {
        // The container commands: the subcommand decides.
        "CONFIG" => match sub.as_str() {
            "GET" | "RESETSTAT" => Class::Read,
            _ => Class::Dangerous,
        },
        "CLIENT" => match sub.as_str() {
            "LIST" | "INFO" | "GETNAME" | "ID" | "NO-EVICT" => Class::Read,
            "KILL" | "UNPAUSE" | "PAUSE" => Class::Dangerous,
            _ => Class::Write,
        },
        "MEMORY" => match sub.as_str() {
            "PURGE" => Class::Dangerous,
            _ => Class::Read,
        },
        "SLOWLOG" => match sub.as_str() {
            "RESET" => Class::Write,
            _ => Class::Read,
        },
        "XINFO" | "OBJECT" | "COMMAND" | "LATENCY" => Class::Read,
        _ if DANGEROUS.contains(&upper.as_str()) => Class::Dangerous,
        _ if BLOCKING.contains(&upper.as_str()) => Class::Blocking,
        _ if READS.contains(&upper.as_str()) => Class::Read,
        _ => Class::Write,
    }
}

/// Split a command line the way `redis-cli` does: whitespace separates
/// arguments, double quotes group them and understand `\n`, `\t`, `\xhh` and
/// `\"`, single quotes group them literally apart from `\'`.
pub fn split(line: &str) -> Result<Vec<String>, String> {
    let mut out: Vec<String> = Vec::new();
    let b: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i < b.len() {
        while i < b.len() && b[i].is_whitespace() {
            i += 1;
        }
        if i >= b.len() {
            break;
        }
        let mut arg = String::new();
        match b[i] {
            '"' => {
                i += 1;
                loop {
                    if i >= b.len() {
                        return Err("unbalanced quotes".into());
                    }
                    match b[i] {
                        '"' => {
                            i += 1;
                            break;
                        }
                        '\\' if i + 1 < b.len() => {
                            i += 1;
                            match b[i] {
                                'n' => arg.push('\n'),
                                'r' => arg.push('\r'),
                                't' => arg.push('\t'),
                                'b' => arg.push('\u{8}'),
                                'a' => arg.push('\u{7}'),
                                'x' if i + 2 < b.len() => {
                                    let hex: String = b[i + 1..i + 3].iter().collect();
                                    match u8::from_str_radix(&hex, 16) {
                                        Ok(v) => {
                                            arg.push(v as char);
                                            i += 2;
                                        }
                                        Err(_) => arg.push('x'),
                                    }
                                }
                                other => arg.push(other),
                            }
                            i += 1;
                        }
                        c => {
                            arg.push(c);
                            i += 1;
                        }
                    }
                }
            }
            '\'' => {
                i += 1;
                loop {
                    if i >= b.len() {
                        return Err("unbalanced quotes".into());
                    }
                    match b[i] {
                        '\'' => {
                            i += 1;
                            break;
                        }
                        '\\' if i + 1 < b.len() && b[i + 1] == '\'' => {
                            arg.push('\'');
                            i += 2;
                        }
                        c => {
                            arg.push(c);
                            i += 1;
                        }
                    }
                }
            }
            _ => {
                while i < b.len() && !b[i].is_whitespace() {
                    arg.push(b[i]);
                    i += 1;
                }
            }
        }
        out.push(arg);
    }
    Ok(out)
}

/// One line of rendered output. `depth` indents nested arrays the way
/// `redis-cli` does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub depth: usize,
    /// "1)" or "field" for a map key; empty for a bare value.
    pub prefix: String,
    pub text: String,
    pub error: bool,
}

/// Render a reply as the console shows it.
pub fn render(v: &redis::Value) -> Vec<Line> {
    let mut out = Vec::new();
    push(v, 0, String::new(), &mut out);
    out
}

fn push(v: &redis::Value, depth: usize, prefix: String, out: &mut Vec<Line>) {
    let mut line = |text: String, error: bool| {
        out.push(Line {
            depth,
            prefix: prefix.clone(),
            text,
            error,
        })
    };
    match v {
        redis::Value::Nil => line("(nil)".into(), false),
        redis::Value::Int(i) => line(format!("(integer) {i}"), false),
        redis::Value::Double(d) => line(format!("(double) {d}"), false),
        redis::Value::Boolean(b) => line(format!("({b})"), false),
        redis::Value::SimpleString(s) => line(s.clone(), false),
        redis::Value::Okay => line("OK".into(), false),
        redis::Value::BulkString(b) => line(quote(b), false),
        redis::Value::VerbatimString { text, .. } => line(text.clone(), false),
        redis::Value::BigNumber(n) => line(String::from_utf8_lossy(n).into_owned(), false),
        redis::Value::ServerError(e) => match e.details() {
            Some(d) => line(format!("(error) {} {d}", e.code()), true),
            None => line(format!("(error) {}", e.code()), true),
        },
        redis::Value::Array(items) | redis::Value::Set(items) => {
            if items.is_empty() {
                line("(empty array)".into(), false);
                return;
            }
            for (n, item) in items.iter().enumerate() {
                push(item, depth, format!("{})", n + 1), out);
            }
        }
        redis::Value::Map(pairs) => {
            if pairs.is_empty() {
                line("(empty map)".into(), false);
                return;
            }
            for (k, val) in pairs {
                let key = match k {
                    redis::Value::BulkString(b) => String::from_utf8_lossy(b).into_owned(),
                    redis::Value::SimpleString(s) => s.clone(),
                    other => format!("{other:?}"),
                };
                push(val, depth, key, out);
            }
        }
        redis::Value::Attribute { data, .. } => push(data, depth, prefix, out),
        redis::Value::Push { data, .. } => {
            for item in data {
                push(item, depth + 1, String::new(), out);
            }
        }
        // The crate marks `Value` non-exhaustive: a type added by a later
        // release prints its debug form rather than being dropped.
        other => line(format!("{other:?}"), false),
    }
}

/// Bulk strings print quoted, with the non-printing bytes escaped.
fn quote(b: &[u8]) -> String {
    let mut s = String::with_capacity(b.len() + 2);
    s.push('"');
    for &c in b {
        match c {
            b'"' => s.push_str("\\\""),
            b'\\' => s.push_str("\\\\"),
            b'\n' => s.push_str("\\n"),
            b'\r' => s.push_str("\\r"),
            b'\t' => s.push_str("\\t"),
            0x20..=0x7e => s.push(c as char),
            other => s.push_str(&format!("\\x{other:02x}")),
        }
    }
    s.push('"');
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn quotes_group_arguments_and_escapes_decode() {
        assert_eq!(split("GET foo").unwrap(), args(&["GET", "foo"]));
        assert_eq!(
            split(r#"SET key "a b""#).unwrap(),
            args(&["SET", "key", "a b"])
        );
        assert_eq!(split(r#"SET k "a\nb""#).unwrap()[2], "a\nb");
        assert_eq!(split(r#"SET k "\x41""#).unwrap()[2], "A");
        // Single quotes are literal apart from an escaped quote.
        assert_eq!(split(r"SET k 'a\nb'").unwrap()[2], r"a\nb");
        assert_eq!(split(r"SET k 'it\'s'").unwrap()[2], "it's");
        // Extra whitespace collapses; an empty line is no command.
        assert_eq!(split("  GET   foo  ").unwrap(), args(&["GET", "foo"]));
        assert!(split("   ").unwrap().is_empty());
        assert!(split(r#"GET "unbalanced"#).is_err());
        // An empty quoted argument survives.
        assert_eq!(split(r#"SET k """#).unwrap(), args(&["SET", "k", ""]));
    }

    #[test]
    fn commands_are_classified_by_what_they_can_destroy() {
        assert_eq!(classify(&args(&["GET", "k"])), Class::Read);
        assert_eq!(classify(&args(&["get", "k"])), Class::Read);
        assert_eq!(classify(&args(&["SET", "k", "v"])), Class::Write);
        assert_eq!(classify(&args(&["FLUSHALL"])), Class::Dangerous);
        assert_eq!(classify(&args(&["KEYS", "*"])), Class::Dangerous);
        assert_eq!(classify(&args(&["MONITOR"])), Class::Blocking);
        assert_eq!(classify(&args(&["BLPOP", "q", "0"])), Class::Blocking);
        // The subcommand decides for the container commands.
        assert_eq!(
            classify(&args(&["CONFIG", "GET", "maxmemory"])),
            Class::Read
        );
        assert_eq!(
            classify(&args(&["CONFIG", "SET", "maxmemory", "0"])),
            Class::Dangerous
        );
        assert_eq!(classify(&args(&["CLIENT", "LIST"])), Class::Read);
        assert_eq!(
            classify(&args(&["CLIENT", "KILL", "id", "4"])),
            Class::Dangerous
        );
        // An unknown command is assumed to write rather than waved through.
        assert_eq!(classify(&args(&["SOMETHING.NEW", "k"])), Class::Write);
        assert_eq!(classify(&[]), Class::Read);
        assert!(Class::Write.writes() && !Class::Read.writes());
    }

    #[test]
    fn replies_render_the_way_redis_cli_shows_them() {
        let lines = render(&redis::Value::Int(7));
        assert_eq!(lines[0].text, "(integer) 7");
        assert_eq!(render(&redis::Value::Nil)[0].text, "(nil)");
        assert_eq!(render(&redis::Value::Okay)[0].text, "OK");
        let lines = render(&redis::Value::Array(vec![
            redis::Value::BulkString(b"a".to_vec()),
            redis::Value::BulkString(b"b".to_vec()),
        ]));
        assert_eq!(lines.len(), 2);
        assert_eq!(
            (lines[0].prefix.as_str(), lines[0].text.as_str()),
            ("1)", "\"a\"")
        );
        assert_eq!(lines[1].prefix, "2)");
        assert_eq!(
            render(&redis::Value::Array(Vec::new()))[0].text,
            "(empty array)"
        );
        // A map prints its keys as the prefix.
        let lines = render(&redis::Value::Map(vec![(
            redis::Value::BulkString(b"name".to_vec()),
            redis::Value::BulkString(b"Ada".to_vec()),
        )]));
        assert_eq!(lines[0].prefix, "name");
        assert_eq!(lines[0].text, "\"Ada\"");
    }

    #[test]
    fn binary_values_print_escaped_rather_than_raw() {
        assert_eq!(quote(b"ok"), "\"ok\"");
        assert_eq!(quote(b"a\nb"), "\"a\\nb\"");
        assert_eq!(quote(&[0x00, 0xff]), "\"\\x00\\xff\"");
        assert_eq!(quote(b"say \"hi\""), r#""say \"hi\"""#);
    }
}
