//! SQL text handling that does not need a server: a tokenizer that knows
//! each dialect's strings, quoted identifiers and comments, the statement
//! splitter (`;`, MySQL `DELIMITER`, `GO` lines, `$$` bodies, trigger and
//! routine `BEGIN … END` blocks) and the destructive-statement classifier of
//! docs/06-security.md §4.
//!
//! The classifier works on keywords at parenthesis depth, not on a full
//! grammar: it has to be right about the first verb and about a missing
//! `WHERE`, and conservative (unknown = write) about everything else.

use crate::driver::Engine;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tok {
    /// A bare word, upper-cased in `upper`, original text in `raw`.
    Word {
        upper: String,
        raw: String,
    },
    /// `"x"`, `` `x` `` or `[x]`, unquoted.
    Quoted(String),
    /// A string literal (contents ignored).
    Str,
    Number,
    LParen,
    RParen,
    Semi,
    Dot,
    Other(char),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub tok: Tok,
    pub start: usize,
    pub end: usize,
}

fn is_word_start(c: char) -> bool {
    c.is_alphabetic() || c == '_'
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '$'
}

/// Tokenize `text`; comments and whitespace are dropped.
pub fn tokenize(text: &str, engine: Engine) -> Vec<Token> {
    let b: Vec<(usize, char)> = text.char_indices().collect();
    let n = b.len();
    let at = |i: usize| b.get(i).map(|x| x.1);
    let off = |i: usize| b.get(i).map(|x| x.0).unwrap_or(text.len());
    let mysql = engine == Engine::Mysql;
    // `#` starts a comment in MySQL and ClickHouse.
    let hash_comment = matches!(engine, Engine::Mysql | Engine::ClickHouse);
    // E'…' strings, $tag$ quoting and nested comments: Postgres and DuckDB.
    let pg_like = matches!(engine, Engine::Postgres | Engine::DuckDb);
    let mut out = Vec::new();
    let mut i = 0;
    while i < n {
        let c = b[i].1;
        let start = off(i);
        // Whitespace
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        // Comments
        if c == '-' && at(i + 1) == Some('-') || (hash_comment && c == '#') {
            while i < n && b[i].1 != '\n' {
                i += 1;
            }
            continue;
        }
        if c == '/' && at(i + 1) == Some('*') {
            let mut depth = 1;
            i += 2;
            while i < n && depth > 0 {
                if b[i].1 == '*' && at(i + 1) == Some('/') {
                    depth -= 1;
                    i += 2;
                } else if pg_like && b[i].1 == '/' && at(i + 1) == Some('*') {
                    depth += 1;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            continue;
        }
        // Strings: '…' with '' doubling; backslash escapes in MySQL and in
        // Postgres E'…'.
        let e_string = pg_like
            && (c == 'E' || c == 'e')
            && at(i + 1) == Some('\'')
            && !out.last().is_some_and(|t: &Token| t.end == start);
        if c == '\'' || e_string || (mysql && c == '"') {
            let quote = if e_string { '\'' } else { c };
            let backslash = engine.backslash_strings() || e_string;
            i += if e_string { 2 } else { 1 };
            while i < n {
                let d = b[i].1;
                if backslash && d == '\\' {
                    i += 2;
                    continue;
                }
                if d == quote {
                    if at(i + 1) == Some(quote) {
                        i += 2;
                        continue;
                    }
                    i += 1;
                    break;
                }
                i += 1;
            }
            out.push(Token {
                tok: Tok::Str,
                start,
                end: off(i),
            });
            continue;
        }
        // Dollar quoting: $tag$ … $tag$ (Postgres, DuckDB).
        if pg_like && c == '$' {
            let mut j = i + 1;
            while j < n && (b[j].1.is_alphanumeric() || b[j].1 == '_') {
                j += 1;
            }
            if j < n && b[j].1 == '$' && !(j > i + 1 && b[i + 1].1.is_ascii_digit()) {
                let tag: String = b[i..=j].iter().map(|x| x.1).collect();
                let body_start = off(j + 1);
                let end = text[body_start..]
                    .find(&tag)
                    .map(|p| body_start + p + tag.len())
                    .unwrap_or(text.len());
                out.push(Token {
                    tok: Tok::Str,
                    start,
                    end,
                });
                while i < n && off(i) < end {
                    i += 1;
                }
                continue;
            }
        }
        // Quoted identifiers.
        let close = match c {
            '"' => Some('"'),
            '`' => Some('`'),
            '[' if matches!(engine, Engine::Sqlite | Engine::Mssql) => Some(']'),
            _ => None,
        };
        if let Some(q) = close {
            let mut s = String::new();
            i += 1;
            while i < n {
                let d = b[i].1;
                if d == q {
                    if at(i + 1) == Some(q) && q != ']' {
                        s.push(q);
                        i += 2;
                        continue;
                    }
                    i += 1;
                    break;
                }
                s.push(d);
                i += 1;
            }
            out.push(Token {
                tok: Tok::Quoted(s),
                start,
                end: off(i),
            });
            continue;
        }
        if is_word_start(c) {
            let mut j = i;
            while j < n && is_word_char(b[j].1) {
                j += 1;
            }
            let raw = &text[start..off(j)];
            out.push(Token {
                tok: Tok::Word {
                    upper: raw.to_ascii_uppercase(),
                    raw: raw.to_string(),
                },
                start,
                end: off(j),
            });
            i = j;
            continue;
        }
        if c.is_ascii_digit() {
            let mut j = i;
            while j < n && (b[j].1.is_ascii_alphanumeric() || b[j].1 == '.') {
                j += 1;
            }
            out.push(Token {
                tok: Tok::Number,
                start,
                end: off(j),
            });
            i = j;
            continue;
        }
        let tok = match c {
            '(' => Tok::LParen,
            ')' => Tok::RParen,
            ';' => Tok::Semi,
            '.' => Tok::Dot,
            other => Tok::Other(other),
        };
        out.push(Token {
            tok,
            start,
            end: off(i + 1),
        });
        i += 1;
    }
    out
}

/// One statement of a script: its text (without the delimiter) and where it
/// sits in the script, in bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Statement {
    pub text: String,
    pub start: usize,
    pub end: usize,
}

fn word(t: &Token) -> Option<&str> {
    match &t.tok {
        Tok::Word { upper, .. } => Some(upper),
        _ => None,
    }
}

/// Split a script into statements. `DELIMITER x` lines (MySQL clients) and
/// `GO` lines (SQL Server tools) are honoured and not returned.
pub fn split(text: &str, engine: Engine) -> Vec<Statement> {
    let mut out = Vec::new();
    // Work line by line for the client directives, then tokenize each chunk
    // between directives with the current delimiter.
    let mut delimiter = ";".to_string();
    let mut chunk_start = 0;
    let mut pos = 0;
    let push_chunk = |out: &mut Vec<Statement>, from: usize, to: usize, delim: &str| {
        if from < to {
            split_chunk(text, from, to, delim, engine, out);
        }
    };
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim();
        let upper = trimmed.to_ascii_uppercase();
        let directive = if upper == "GO" {
            Some(None)
        } else if upper.starts_with("DELIMITER ") || upper == "DELIMITER" {
            Some(trimmed.split_whitespace().nth(1).map(str::to_string))
        } else {
            None
        };
        if let Some(new_delim) = directive
            && !inside_open_token(&text[chunk_start..pos], engine)
        {
            push_chunk(&mut out, chunk_start, pos, &delimiter);
            if let Some(d) = new_delim {
                delimiter = d;
            }
            chunk_start = pos + line.len();
        }
        pos += line.len();
    }
    push_chunk(&mut out, chunk_start, text.len(), &delimiter);
    out
}

/// True when `text` ends inside a string, quoted identifier or block
/// comment (so a `GO` line there is content, not a directive).
fn inside_open_token(text: &str, engine: Engine) -> bool {
    tokenize(text, engine)
        .last()
        .is_some_and(|t| t.end > text.trim_end().len())
}

fn split_chunk(
    text: &str,
    from: usize,
    to: usize,
    delimiter: &str,
    engine: Engine,
    out: &mut Vec<Statement>,
) {
    let chunk = &text[from..to];
    let toks = tokenize(chunk, engine);
    let mut stmt_start = 0;
    let mut first: Option<usize> = None;
    let mut block_depth = 0i32;
    let mut routine = false;
    let mut k = 0;
    let finish = |out: &mut Vec<Statement>, a: usize, b: usize| {
        let s = chunk[a..b].trim();
        if !s.is_empty() && !tokenize(s, engine).is_empty() {
            let lead = chunk[a..b].len() - chunk[a..b].trim_start().len();
            out.push(Statement {
                text: s.to_string(),
                start: from + a + lead,
                end: from + a + lead + s.len(),
            });
        }
    };
    if delimiter != ";" {
        // A custom delimiter is matched on the raw text outside tokens.
        let mut cursor = 0;
        let mut search = 0;
        while let Some(p) = chunk[search..].find(delimiter) {
            let at = search + p;
            let inside = toks
                .iter()
                .any(|t| t.start < at && at < t.end && matches!(t.tok, Tok::Str | Tok::Quoted(_)));
            if !inside {
                finish(out, cursor, at);
                cursor = at + delimiter.len();
            }
            search = at + delimiter.len();
        }
        finish(out, cursor, chunk.len());
        return;
    }
    while k < toks.len() {
        let t = &toks[k];
        if first.is_none() && !matches!(t.tok, Tok::Semi) {
            first = Some(k);
            // CREATE [OR REPLACE] [TEMP] TRIGGER/FUNCTION/PROCEDURE bodies may
            // hold semicolons inside BEGIN … END.
            if word(t) == Some("CREATE") {
                routine = toks[k + 1..]
                    .iter()
                    .take(6)
                    .filter_map(word)
                    .any(|w| matches!(w, "TRIGGER" | "FUNCTION" | "PROCEDURE"));
            }
        }
        if routine {
            match word(t) {
                Some("BEGIN") | Some("CASE") => block_depth += 1,
                Some("END") => block_depth = (block_depth - 1).max(0),
                _ => {}
            }
        }
        if matches!(t.tok, Tok::Semi) && block_depth == 0 {
            finish(out, stmt_start, t.start);
            stmt_start = t.end;
            first = None;
            routine = false;
        }
        k += 1;
    }
    finish(out, stmt_start, chunk.len());
}

/// The statement under the cursor (byte offset), or the nearest one before
/// it when the cursor sits between statements.
pub fn statement_at(stmts: &[Statement], cursor: usize) -> Option<&Statement> {
    stmts
        .iter()
        .find(|s| cursor >= s.start && cursor <= s.end)
        .or_else(|| stmts.iter().rev().find(|s| s.end <= cursor))
        .or_else(|| stmts.first())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Class {
    /// SELECT, SHOW, EXPLAIN, …
    Read,
    /// BEGIN, SET, USE, … (allowed on read-only connections).
    Session,
    Write,
    /// UPDATE or DELETE without WHERE.
    WriteUnbounded,
    /// CREATE, ALTER, COMMENT, …
    Ddl,
    /// DROP, TRUNCATE, ALTER … DROP.
    DdlDestructive,
    /// GRANT, REVOKE, KILL, ALTER SYSTEM, CREATE USER, SET GLOBAL, …
    Admin,
}

impl Class {
    /// Whether a read-only connection may run it.
    pub fn is_read_only(self) -> bool {
        matches!(self, Class::Read | Class::Session)
    }
    /// Whether it needs the confirmation dialog (docs/tools/database.md §9).
    pub fn is_dangerous(self) -> bool {
        matches!(
            self,
            Class::WriteUnbounded | Class::DdlDestructive | Class::Admin
        )
    }
}

/// The classification of one statement plus the object it targets, for the
/// "type the name to confirm" guard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Classified {
    pub class: Class,
    pub target: Option<String>,
}

/// Index just past a qualified name starting at `k` (`a`, `a.b`, `"a"."b"`),
/// with the name as written (quotes removed).
fn qualified_name(toks: &[Token], mut k: usize) -> Option<(String, usize)> {
    let mut parts = Vec::new();
    loop {
        match toks.get(k).map(|t| &t.tok) {
            Some(Tok::Word { raw, .. }) => parts.push(raw.clone()),
            Some(Tok::Quoted(s)) => parts.push(s.clone()),
            _ => break,
        }
        k += 1;
        if matches!(toks.get(k).map(|t| &t.tok), Some(Tok::Dot)) {
            k += 1;
        } else {
            break;
        }
    }
    (!parts.is_empty()).then(|| (parts.join("."), k))
}

/// The name after `k`, skipping any of `skip` words first.
fn name_after(toks: &[Token], mut k: usize, skip: &[&str]) -> Option<String> {
    while let Some(w) = toks.get(k).and_then(word) {
        if skip.contains(&w) {
            k += 1;
        } else {
            break;
        }
    }
    qualified_name(toks, k).map(|(n, _)| n)
}

const OBJECT_WORDS: &[&str] = &[
    "TABLE",
    "VIEW",
    "MATERIALIZED",
    "INDEX",
    "SCHEMA",
    "DATABASE",
    "SEQUENCE",
    "FUNCTION",
    "PROCEDURE",
    "TRIGGER",
    "TYPE",
    "EXTENSION",
    "USER",
    "ROLE",
    "TEMPORARY",
    "TEMP",
    "IF",
    "EXISTS",
    "ONLY",
    "CONCURRENTLY",
    "UNIQUE",
    "OR",
    "REPLACE",
    "DOMAIN",
    "OWNED",
    "BY",
];

/// Is the UPDATE/DELETE at `k` (inside parenthesis depth `depth` of its own)
/// missing a WHERE (or LIMIT) at its level?
fn unbounded(toks: &[Token], k: usize) -> bool {
    let mut depth = 0i32;
    for t in &toks[k + 1..] {
        match &t.tok {
            Tok::LParen => depth += 1,
            Tok::RParen => {
                depth -= 1;
                if depth < 0 {
                    return true;
                }
            }
            Tok::Word { upper, .. }
                if depth == 0 && matches!(upper.as_str(), "WHERE" | "LIMIT") =>
            {
                return false;
            }
            _ => {}
        }
    }
    true
}

/// Whether a statement changes what the schema browser lists: it creates,
/// drops, alters or renames a database, schema or object (FEAT-0068). A
/// `TRUNCATE` empties a table but lists nothing new.
pub fn changes_catalog(stmt: &str, engine: Engine) -> bool {
    let toks = tokenize(stmt, engine);
    matches!(
        toks.first().and_then(word),
        Some("CREATE" | "DROP" | "ALTER" | "RENAME" | "ATTACH" | "DETACH" | "IMPORT")
    )
}

/// Classify one statement (as produced by [`split`]).
pub fn classify(stmt: &str, engine: Engine) -> Classified {
    let toks = tokenize(stmt, engine);
    let words: Vec<&str> = toks.iter().filter_map(word).collect();
    let w = |i: usize| words.get(i).copied().unwrap_or("");
    let first = w(0);
    let of = |class: Class| Classified {
        class,
        target: None,
    };

    // Data-modifying verbs anywhere (CTEs, EXPLAIN ANALYZE, sub-statements):
    // a verb counts when it starts the statement or follows "(".
    let mut worst = Class::Read;
    let mut worst_target = None;
    for (k, t) in toks.iter().enumerate() {
        let Some(v) = word(t) else { continue };
        let starts = k == 0 || matches!(toks[k - 1].tok, Tok::LParen) || {
            // EXPLAIN [ANALYZE] [VERBOSE] verb, WITH … AS (…) verb
            matches!(toks[k - 1].tok, Tok::RParen)
                || matches!(word(&toks[k - 1]), Some("ANALYZE" | "VERBOSE" | "EXPLAIN"))
        };
        if !starts {
            continue;
        }
        let (class, target) = match v {
            "INSERT" | "REPLACE" | "MERGE" | "UPSERT" => (Class::Write, None),
            "UPDATE" => {
                let t = name_after(&toks, k + 1, &["ONLY", "LOW_PRIORITY", "IGNORE"]);
                if unbounded(&toks, k) {
                    (Class::WriteUnbounded, t)
                } else {
                    (Class::Write, None)
                }
            }
            "DELETE" => {
                let t = name_after(
                    &toks,
                    k + 1,
                    &["FROM", "ONLY", "LOW_PRIORITY", "QUICK", "IGNORE"],
                );
                if unbounded(&toks, k) {
                    (Class::WriteUnbounded, t)
                } else {
                    (Class::Write, None)
                }
            }
            _ => continue,
        };
        if class > worst {
            worst = class;
            worst_target = target;
        }
    }

    let explain_analyze = first == "EXPLAIN" && words.iter().take(4).any(|x| *x == "ANALYZE");
    let base = match first {
        "SELECT" => {
            // SELECT … INTO new_table (Postgres, SQL Server) creates a table.
            let into = toks
                .iter()
                .scan(0i32, |d, t| {
                    match t.tok {
                        Tok::LParen => *d += 1,
                        Tok::RParen => *d -= 1,
                        _ => {}
                    }
                    Some((*d, word(t)))
                })
                .any(|(d, w)| d == 0 && w == Some("INTO"));
            let admin_fn = words.iter().any(|x| {
                matches!(
                    *x,
                    "PG_TERMINATE_BACKEND" | "PG_CANCEL_BACKEND" | "PG_RELOAD_CONF"
                )
            });
            // MySQL's `SELECT … INTO @var` is a read, but `INTO OUTFILE` /
            // `INTO DUMPFILE` write server files.
            let mysql_file = engine == Engine::Mysql
                && words.iter().any(|w| matches!(*w, "OUTFILE" | "DUMPFILE"));
            if admin_fn {
                of(Class::Admin)
            } else if (into && engine != Engine::Mysql) || mysql_file {
                of(Class::Write)
            } else {
                of(Class::Read)
            }
        }
        "WITH" | "VALUES" | "TABLE" | "SHOW" | "DESCRIBE" | "DESC" | "HELP" => of(Class::Read),
        "EXPLAIN" if !explain_analyze => of(Class::Read),
        "EXPLAIN" => of(Class::Read), // the analysed verb was classified above
        "PRAGMA" => {
            if toks.iter().any(|t| matches!(t.tok, Tok::Other('='))) {
                of(Class::Write)
            } else {
                of(Class::Read)
            }
        }
        "COPY" => {
            // `COPY … TO PROGRAM` runs a shell command on the server and
            // `COPY … TO '/path'` writes a server file; only `TO STDOUT` is
            // a read. `FROM` loads rows.
            if words.contains(&"PROGRAM") {
                of(Class::Admin)
            } else if words.contains(&"FROM") || !words.contains(&"STDOUT") {
                of(Class::Write)
            } else {
                of(Class::Read)
            }
        }
        "INSERT" | "REPLACE" | "MERGE" | "UPSERT" | "UPDATE" | "DELETE" | "CALL" | "EXEC"
        | "EXECUTE" | "DO" | "LOAD" | "IMPORT" | "LOCK" | "ATTACH" | "HANDLER" => of(Class::Write),
        "CREATE" => {
            if words[1..]
                .iter()
                .take(3)
                .any(|x| matches!(*x, "USER" | "ROLE" | "LOGIN"))
            {
                of(Class::Admin)
            } else {
                of(Class::Ddl)
            }
        }
        "ALTER" => {
            if matches!(
                w(1),
                "SYSTEM" | "USER" | "ROLE" | "LOGIN" | "SERVER" | "INSTANCE"
            ) {
                of(Class::Admin)
            } else if words.contains(&"DROP") {
                Classified {
                    class: Class::DdlDestructive,
                    target: name_after(&toks, 1, OBJECT_WORDS),
                }
            } else {
                of(Class::Ddl)
            }
        }
        "DROP" => Classified {
            class: Class::DdlDestructive,
            target: name_after(&toks, 1, OBJECT_WORDS),
        },
        "TRUNCATE" => Classified {
            class: Class::DdlDestructive,
            target: name_after(&toks, 1, &["TABLE", "ONLY"]),
        },
        "COMMENT" | "RENAME" | "REINDEX" | "ANALYZE" | "CLUSTER" | "REFRESH" | "OPTIMIZE"
        | "VACUUM" | "SECURITY" => of(Class::Ddl),
        "GRANT" | "REVOKE" | "KILL" | "SHUTDOWN" | "FLUSH" | "RESET" | "PURGE" | "INSTALL"
        | "UNINSTALL" | "CHANGE" | "CHECKPOINT" | "REASSIGN" => of(Class::Admin),
        "SET" => {
            if matches!(w(1), "GLOBAL" | "PERSIST" | "PERSIST_ONLY" | "PASSWORD") {
                of(Class::Admin)
            } else {
                of(Class::Session)
            }
        }
        "BEGIN" | "START" | "COMMIT" | "END" | "ROLLBACK" | "SAVEPOINT" | "RELEASE" | "USE"
        | "DETACH" | "LISTEN" | "UNLISTEN" | "DISCARD" | "DEALLOCATE" | "PREPARE" | "FETCH"
        | "CLOSE" | "DECLARE" | "MOVE" => of(Class::Session),
        _ => of(Class::Write),
    };
    if worst > base.class {
        Classified {
            class: worst,
            target: worst_target,
        }
    } else {
        base
    }
}

/// Wrap an identifier in the dialect's quotes when it needs them: when it
/// is not a plain word, or when the server would fold its case into a
/// different name (Postgres and DuckDB fold to lower case, Oracle and Db2
/// to upper case).
pub fn quote_ident(name: &str, engine: Engine) -> String {
    use crate::driver::Fold;
    let folds_away = match engine.folds() {
        Fold::Lower => name.chars().any(|c| c.is_ascii_uppercase()),
        Fold::Upper => name.chars().any(|c| c.is_ascii_lowercase()),
        Fold::Keep => false,
    };
    let plain = name.chars().next().is_some_and(is_word_start)
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && !folds_away;
    if plain {
        return name.to_string();
    }
    match engine {
        Engine::Mysql | Engine::ClickHouse => format!("`{}`", name.replace('`', "``")),
        Engine::Mssql => format!("[{}]", name.replace(']', "]]")),
        _ => format!("\"{}\"", name.replace('"', "\"\"")),
    }
}

/// A string literal in the dialect.
pub fn quote_literal(s: &str, engine: Engine) -> String {
    let body = s.replace('\'', "''");
    if engine.backslash_strings() {
        format!("'{}'", body.replace('\\', "\\\\"))
    } else {
        format!("'{body}'")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(sql: &str, e: Engine) -> Vec<String> {
        split(sql, e).into_iter().map(|s| s.text).collect()
    }

    fn class(sql: &str) -> Class {
        classify(sql, Engine::Postgres).class
    }

    #[test]
    fn copy_and_outfile_are_not_reads() {
        // Only `TO STDOUT` streams rows to the client; everything else
        // writes server files or runs a server-side command.
        assert_eq!(class("COPY (SELECT 1) TO STDOUT"), Class::Read);
        assert_eq!(class("COPY orders TO '/tmp/orders.csv'"), Class::Write);
        assert_eq!(class("COPY orders FROM '/tmp/orders.csv'"), Class::Write);
        assert_eq!(class("COPY (SELECT 1) TO PROGRAM 'rm -rf /'"), Class::Admin);
        assert_eq!(
            classify("SELECT * FROM t INTO OUTFILE '/tmp/t.csv'", Engine::Mysql).class,
            Class::Write
        );
        assert_eq!(
            classify("SELECT 1 INTO @x", Engine::Mysql).class,
            Class::Read
        );
    }

    #[test]
    fn splits_on_semicolons_outside_strings_and_comments() {
        let sql = "select 'a;b' as x; -- c;d\nselect \"e;f\" /* g; */ from t;\n\n  ;select 3";
        assert_eq!(
            texts(sql, Engine::Postgres),
            vec![
                "select 'a;b' as x",
                "-- c;d\nselect \"e;f\" /* g; */ from t",
                "select 3"
            ]
        );
    }

    #[test]
    fn statement_offsets_point_into_the_script() {
        let sql = "select 1;\n  select 2 ;";
        let s = split(sql, Engine::Sqlite);
        assert_eq!(&sql[s[1].start..s[1].end], "select 2");
        assert_eq!(statement_at(&s, 3).unwrap().text, "select 1");
        assert_eq!(statement_at(&s, 15).unwrap().text, "select 2");
        assert_eq!(statement_at(&s, sql.len()).unwrap().text, "select 2");
    }

    #[test]
    fn dollar_quotes_hold_semicolons() {
        let sql = "create function f() returns int as $body$ begin; return 1; end $body$ language plpgsql;\nselect $1, $$x;y$$;";
        let s = texts(sql, Engine::Postgres);
        assert_eq!(s.len(), 2, "{s:?}");
        assert!(s[1].ends_with("$$x;y$$"));
    }

    #[test]
    fn mysql_delimiter_and_backslash_strings() {
        let sql = "DELIMITER //\nCREATE PROCEDURE p() BEGIN SELECT 1; SELECT 2; END //\nDELIMITER ;\nselect 'it\\'s;' ; select 4";
        let s = texts(sql, Engine::Mysql);
        assert_eq!(s.len(), 3, "{s:?}");
        assert!(s[0].starts_with("CREATE PROCEDURE") && s[0].ends_with("END"));
        assert_eq!(s[1], "select 'it\\'s;'");
        assert_eq!(texts("select 1 # c;\n; select 2", Engine::Mysql).len(), 2);
    }

    #[test]
    fn sqlite_trigger_bodies_and_go_lines() {
        let sql = "CREATE TRIGGER t AFTER INSERT ON a BEGIN UPDATE b SET n = n + 1; INSERT INTO c VALUES (CASE WHEN 1 THEN 2 END); END;\nselect 1\nGO\nselect 2";
        let s = texts(sql, Engine::Sqlite);
        assert_eq!(s.len(), 3, "{s:?}");
        assert!(s[0].ends_with("END"));
        assert_eq!(s[1], "select 1");
        // A GO inside a string is content.
        assert_eq!(texts("select 'x\nGO\ny'", Engine::Sqlite).len(), 1);
    }

    #[test]
    fn classifies_reads_and_session_statements() {
        for s in [
            "SELECT * FROM t",
            "  select 1 -- delete from t",
            "with x as (select 1) select * from x",
            "SHOW TABLES",
            "explain select * from t",
            "VALUES (1)",
            "select 'drop table t'",
            "select * from t for update",
        ] {
            assert_eq!(class(s), Class::Read, "{s}");
        }
        for s in ["BEGIN", "commit", "SET search_path = x", "use shop"] {
            assert_eq!(class(s), Class::Session, "{s}");
        }
        assert_eq!(
            classify("PRAGMA table_info(t)", Engine::Sqlite).class,
            Class::Read
        );
        assert_eq!(
            classify("PRAGMA journal_mode = wal", Engine::Sqlite).class,
            Class::Write
        );
    }

    #[test]
    fn finds_unbounded_writes() {
        let c = classify("DELETE FROM public.orders", Engine::Postgres);
        assert_eq!(c.class, Class::WriteUnbounded);
        assert_eq!(c.target.as_deref(), Some("public.orders"));
        let c = classify("update \"Order Items\" set x = 1", Engine::Postgres);
        assert_eq!(c.class, Class::WriteUnbounded);
        assert_eq!(c.target.as_deref(), Some("Order Items"));
        assert_eq!(class("delete from t where id = 1"), Class::Write);
        assert_eq!(
            class("update t set a = (select max(b) from u where u.x = 1)"),
            Class::WriteUnbounded,
            "a WHERE inside a subquery does not bound the update"
        );
        assert_eq!(
            classify("DELETE FROM t LIMIT 10", Engine::Mysql).class,
            Class::Write
        );
        assert_eq!(
            class("with d as (delete from t returning *) select * from d"),
            Class::WriteUnbounded
        );
        assert_eq!(
            class("with d as (delete from t where id = 1 returning *) select * from d"),
            Class::Write
        );
        assert_eq!(
            class("explain analyze delete from t"),
            Class::WriteUnbounded
        );
        assert_eq!(class("insert into t values (1)"), Class::Write);
        assert_eq!(
            class("create table x (id int references y on delete cascade)"),
            Class::Ddl
        );
    }

    #[test]
    fn catalogue_changes_are_recognised() {
        let e = Engine::Postgres;
        assert!(changes_catalog("create table t (a int)", e));
        assert!(changes_catalog("  DROP DATABASE shop", Engine::Mysql));
        assert!(changes_catalog("alter table t add b int", e));
        assert!(changes_catalog("RENAME TABLE a TO b", Engine::Mysql));
        assert!(!changes_catalog("truncate table t", e));
        assert!(!changes_catalog("insert into t values (1)", e));
        assert!(!changes_catalog("select 'create'", e));
        // A comment before the verb is not the verb.
        assert!(changes_catalog("-- make it\ncreate view v as select 1", e));
    }

    #[test]
    fn classifies_ddl_and_admin() {
        let c = classify("DROP TABLE IF EXISTS shop.orders CASCADE", Engine::Mysql);
        assert_eq!(c.class, Class::DdlDestructive);
        assert_eq!(c.target.as_deref(), Some("shop.orders"));
        let c = classify("truncate table only orders", Engine::Postgres);
        assert_eq!(c.class, Class::DdlDestructive);
        assert_eq!(c.target.as_deref(), Some("orders"));
        let c = classify("ALTER TABLE orders DROP COLUMN note", Engine::Postgres);
        assert_eq!(c.class, Class::DdlDestructive);
        assert_eq!(c.target.as_deref(), Some("orders"));
        assert_eq!(class("alter table orders add column note text"), Class::Ddl);
        assert_eq!(class("CREATE INDEX i ON t (a)"), Class::Ddl);
        assert_eq!(class("GRANT SELECT ON t TO bob"), Class::Admin);
        assert_eq!(class("ALTER SYSTEM SET work_mem = '1GB'"), Class::Admin);
        assert_eq!(class("create user bob"), Class::Admin);
        assert_eq!(class("select pg_terminate_backend(42)"), Class::Admin);
        assert_eq!(
            classify("SET GLOBAL max_connections = 10", Engine::Mysql).class,
            Class::Admin
        );
        assert_eq!(class("frobnicate everything"), Class::Write);
        assert_eq!(class("select * into backup from t"), Class::Write);
    }

    #[test]
    fn quoting() {
        assert_eq!(quote_ident("orders", Engine::Postgres), "orders");
        assert_eq!(quote_ident("Orders", Engine::Postgres), "\"Orders\"");
        assert_eq!(quote_ident("Orders", Engine::Mysql), "Orders");
        assert_eq!(quote_ident("a b", Engine::Mysql), "`a b`");
        assert_eq!(quote_ident("a\"b", Engine::Sqlite), "\"a\"\"b\"");
        assert_eq!(quote_literal("it's", Engine::Postgres), "'it''s'");
        assert_eq!(quote_literal("a\\b", Engine::Mysql), "'a\\\\b'");
        // ADR-030 engines.
        assert_eq!(quote_ident("Orders", Engine::Mssql), "Orders");
        assert_eq!(quote_ident("a]b", Engine::Mssql), "[a]]b]");
        assert_eq!(quote_ident("orders", Engine::Oracle), "\"orders\"");
        assert_eq!(quote_ident("ORDERS", Engine::Db2), "ORDERS");
        assert_eq!(quote_ident("Orders", Engine::DuckDb), "\"Orders\"");
        assert_eq!(quote_ident("a b", Engine::ClickHouse), "`a b`");
        assert_eq!(quote_literal("a\\b", Engine::ClickHouse), "'a\\\\b'");
        assert_eq!(quote_literal("a\\b", Engine::Oracle), "'a\\b'");
    }
}
