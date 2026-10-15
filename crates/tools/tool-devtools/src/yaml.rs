//! YAML: check a document for the mistakes that break a deployment, and
//! convert between YAML and Java `.properties` both ways (the
//! `application.yml` ↔ `application.properties` round trip).
//!
//! No YAML crate: the workspace carries none, and a configuration file
//! needs only a well-defined subset. What is understood: comments,
//! `key: value`, nested blocks by indentation, sequences (`- item`,
//! including `- key: value` maps), quoted and plain scalars, block
//! scalars (`|` and `>`), simple flow sequences (`[a, b]`) and the `---`
//! document marker. Anything outside that (anchors, aliases, tags, flow
//! maps, several documents) is reported rather than guessed at, so the
//! converter never invents a value.

use hs_core::Services;
use hs_ui::form;
use hs_ui::theme::Theme;
use serde::{Deserialize, Serialize};

use crate::util;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Mode {
    /// Report what is wrong with the YAML, or that it is sound.
    #[default]
    Check,
    YamlToProperties,
    PropertiesToYaml,
}

#[derive(Default, Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct State {
    pub mode: Mode,
    pub input: String,
}

/// A parsed document. Maps keep the order they were written in.
#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Scalar(String),
    Map(Vec<(String, Node)>),
    Seq(Vec<Node>),
}

/// Something wrong (or merely unsupported) at a 1-based line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    pub line: usize,
    pub message: String,
}

impl Problem {
    fn new(line: usize, message: impl Into<String>) -> Self {
        Self {
            line,
            message: message.into(),
        }
    }
}

struct Src {
    /// (1-based line number, indent, content without the indent)
    lines: Vec<(usize, usize, String)>,
    problems: Vec<Problem>,
}

/// Strip comments and blank lines, measure indentation, flag tabs.
fn scan(text: &str) -> Src {
    let mut lines = Vec::new();
    let mut problems = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        let n = i + 1;
        let indent_str: String = raw
            .chars()
            .take_while(|c| *c == ' ' || *c == '\t')
            .collect();
        if indent_str.contains('\t') {
            problems.push(Problem::new(
                n,
                "tab used to indent: YAML forbids tabs, use spaces",
            ));
        }
        let body = raw[indent_str.len()..].trim_end();
        if body.is_empty() || body.starts_with('#') {
            continue;
        }
        if body == "---" || body == "..." {
            continue;
        }
        lines.push((n, indent_str.chars().count(), body.to_string()));
    }
    Src { lines, problems }
}

/// Split `key: value` outside quotes. `None` when there is no colon that
/// ends a key.
fn split_key(body: &str) -> Option<(String, String)> {
    let chars: Vec<char> = body.chars().collect();
    let mut quote: Option<char> = None;
    for (i, &c) in chars.iter().enumerate() {
        match quote {
            Some(q) => {
                if c == q {
                    quote = None;
                }
            }
            None => {
                if c == '\'' || c == '"' {
                    quote = Some(c);
                } else if c == '#' && i > 0 && chars[i - 1] == ' ' {
                    // A trailing comment ends the line.
                    break;
                } else if c == ':' {
                    let after = chars.get(i + 1);
                    if after.is_none() || after == Some(&' ') {
                        let key: String = chars[..i].iter().collect();
                        let value: String = chars[i + 1..].iter().collect();
                        return Some((key.trim().to_string(), value.trim().to_string()));
                    }
                }
            }
        }
    }
    None
}

/// Strip a trailing ` # comment` from a plain scalar.
fn strip_comment(value: &str) -> &str {
    let bytes: Vec<char> = value.chars().collect();
    let mut quote: Option<char> = None;
    for (i, &c) in bytes.iter().enumerate() {
        match quote {
            Some(q) => {
                if c == q {
                    quote = None;
                }
            }
            None => {
                if c == '\'' || c == '"' {
                    quote = Some(c);
                } else if c == '#' && (i == 0 || bytes[i - 1] == ' ') {
                    return value[..value
                        .char_indices()
                        .nth(i)
                        .map(|(b, _)| b)
                        .unwrap_or(value.len())]
                        .trim_end();
                }
            }
        }
    }
    value
}

/// Unquote a scalar and report an unterminated quote.
fn scalar(value: &str, line: usize, problems: &mut Vec<Problem>) -> String {
    let v = strip_comment(value).trim();
    if v.len() >= 2 {
        let first = v.chars().next().unwrap();
        let last = v.chars().last().unwrap();
        if (first == '"' || first == '\'') && last == first {
            let inner: String = v[first.len_utf8()..v.len() - last.len_utf8()].to_string();
            return if first == '"' {
                inner
                    .replace("\\n", "\n")
                    .replace("\\t", "\t")
                    .replace("\\\"", "\"")
                    .replace("\\\\", "\\")
            } else {
                inner.replace("''", "'")
            };
        }
    }
    if (v.starts_with('"') || v.starts_with('\'')) && v.chars().count() > 1 {
        problems.push(Problem::new(line, "quote is never closed"));
    }
    if v.starts_with('&') || v.starts_with('*') || v.starts_with("!!") {
        problems.push(Problem::new(
            line,
            "anchors, aliases and tags are not supported; the value is kept as text",
        ));
    }
    if v.starts_with('{') {
        problems.push(Problem::new(
            line,
            "a flow map ({…}) is not supported; write it as an indented block",
        ));
    }
    v.to_string()
}

/// `[a, b, c]` to its items; `None` when it is not a flow sequence.
fn flow_seq(value: &str, line: usize, problems: &mut Vec<Problem>) -> Option<Vec<Node>> {
    let v = strip_comment(value).trim();
    let inner = v.strip_prefix('[')?.strip_suffix(']')?;
    if inner.trim().is_empty() {
        return Some(Vec::new());
    }
    Some(
        inner
            .split(',')
            .map(|item| Node::Scalar(scalar(item, line, problems)))
            .collect(),
    )
}

/// Parse the block at `indent` starting at `i`, leaving `i` on the first
/// line that belongs to an outer block.
fn parse_block(src: &mut Src, i: &mut usize, indent: usize) -> Node {
    let is_seq = src
        .lines
        .get(*i)
        .is_some_and(|(_, ind, body)| *ind == indent && (body == "-" || body.starts_with("- ")));
    if is_seq {
        let mut items = Vec::new();
        while let Some((n, ind, body)) = src.lines.get(*i).cloned() {
            if ind < indent {
                break;
            }
            if ind > indent || !(body == "-" || body.starts_with("- ")) {
                src.problems
                    .push(Problem::new(n, "this line is indented past its list item"));
                *i += 1;
                continue;
            }
            let rest = body[1..].trim_start().to_string();
            let rest_col = indent + (body.chars().count() - rest.chars().count());
            *i += 1;
            if rest.is_empty() {
                let child_indent = src.lines.get(*i).map(|(_, ind, _)| *ind).unwrap_or(0);
                if child_indent > indent {
                    items.push(parse_block(src, i, child_indent));
                } else {
                    items.push(Node::Scalar(String::new()));
                }
            } else if let Some((key, value)) = split_key(&rest) {
                // `- key: value` starts a map whose keys line up with `key`.
                let mut entries = Vec::new();
                finish_entry(src, i, rest_col, n, key, value, &mut entries);
                while let Some((_, ind, body)) = src.lines.get(*i).cloned() {
                    if ind != rest_col || body.starts_with("- ") {
                        break;
                    }
                    let (n2, _, body2) = src.lines[*i].clone();
                    let Some((k2, v2)) = split_key(&body2) else {
                        break;
                    };
                    *i += 1;
                    finish_entry(src, i, rest_col, n2, k2, v2, &mut entries);
                }
                items.push(Node::Map(entries));
            } else if let Some(seq) = flow_seq(&rest, n, &mut src.problems) {
                items.push(Node::Seq(seq));
            } else {
                items.push(Node::Scalar(scalar(&rest, n, &mut src.problems)));
            }
        }
        return Node::Seq(items);
    }
    let mut entries: Vec<(String, Node)> = Vec::new();
    while let Some((n, ind, body)) = src.lines.get(*i).cloned() {
        if ind < indent {
            break;
        }
        if ind > indent {
            src.problems.push(Problem::new(
                n,
                "this line is indented more than the one above it opens for",
            ));
            *i += 1;
            continue;
        }
        if body.starts_with("- ") || body == "-" {
            break;
        }
        let Some((key, value)) = split_key(&body) else {
            src.problems.push(Problem::new(
                n,
                format!("expected \"key: value\", found \"{body}\""),
            ));
            *i += 1;
            continue;
        };
        if key.is_empty() {
            src.problems.push(Problem::new(n, "the key is empty"));
        }
        if entries.iter().any(|(k, _)| *k == key) {
            src.problems.push(Problem::new(
                n,
                format!("\"{key}\" is set twice in the same block; the last one wins"),
            ));
        }
        *i += 1;
        finish_entry(src, i, indent, n, key, value, &mut entries);
    }
    Node::Map(entries)
}

/// The value of one `key: value`, which may open a nested block or a
/// block scalar.
fn finish_entry(
    src: &mut Src,
    i: &mut usize,
    indent: usize,
    line: usize,
    key: String,
    value: String,
    entries: &mut Vec<(String, Node)>,
) {
    let trimmed = strip_comment(&value).trim().to_string();
    if trimmed == "|" || trimmed == ">" || trimmed.starts_with("|-") || trimmed.starts_with(">-") {
        let fold = trimmed.starts_with('>');
        let mut text = String::new();
        while let Some((_, ind, body)) = src.lines.get(*i).cloned() {
            if ind <= indent {
                break;
            }
            if !text.is_empty() {
                text.push(if fold { ' ' } else { '\n' });
            }
            text.push_str(&body);
            *i += 1;
        }
        entries.push((key, Node::Scalar(text)));
        return;
    }
    if trimmed.is_empty() {
        let child_indent = src.lines.get(*i).map(|(_, ind, _)| *ind).unwrap_or(0);
        if child_indent > indent {
            let node = parse_block(src, i, child_indent);
            entries.push((key, node));
        } else {
            // `key:` with nothing under it is an empty value.
            entries.push((key, Node::Scalar(String::new())));
        }
        return;
    }
    if let Some(seq) = flow_seq(&trimmed, line, &mut src.problems) {
        entries.push((key, Node::Seq(seq)));
        return;
    }
    entries.push((key, Node::Scalar(scalar(&trimmed, line, &mut src.problems))));
}

/// Parse YAML (the supported subset) and report what is wrong with it.
pub fn parse_yaml(text: &str) -> (Node, Vec<Problem>) {
    let mut src = scan(text);
    if src.lines.is_empty() {
        return (Node::Map(Vec::new()), src.problems);
    }
    let base = src.lines[0].1;
    if base != 0 {
        src.problems.push(Problem::new(
            src.lines[0].0,
            "the document starts indented; the top level must begin at column 1",
        ));
    }
    let mut i = 0;
    let node = parse_block(&mut src, &mut i, base);
    if i < src.lines.len() {
        let (n, _, _) = src.lines[i];
        src.problems
            .push(Problem::new(n, "this line does not belong to any block"));
    }
    (node, src.problems)
}

// ---- properties ----------------------------------------------------------------

fn escape_property_value(v: &str) -> String {
    v.replace('\\', "\\\\")
        .replace('\n', "\\n")
        .replace('\t', "\\t")
}

fn escape_property_key(k: &str) -> String {
    k.replace('\\', "\\\\")
        .replace(' ', "\\ ")
        .replace('=', "\\=")
        .replace(':', "\\:")
}

fn flatten(node: &Node, prefix: &str, out: &mut Vec<(String, String)>) {
    match node {
        Node::Scalar(v) => out.push((prefix.to_string(), v.clone())),
        Node::Map(entries) => {
            for (k, v) in entries {
                let key = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{prefix}.{k}")
                };
                flatten(v, &key, out);
            }
        }
        Node::Seq(items) => {
            if items.is_empty() {
                out.push((prefix.to_string(), String::new()));
            }
            for (n, item) in items.iter().enumerate() {
                flatten(item, &format!("{prefix}[{n}]"), out);
            }
        }
    }
}

/// YAML to `.properties` text.
pub fn to_properties(node: &Node) -> String {
    let mut rows = Vec::new();
    flatten(node, "", &mut rows);
    rows.into_iter()
        .map(|(k, v)| {
            format!(
                "{}={}\n",
                escape_property_key(&k),
                escape_property_value(&v)
            )
        })
        .collect()
}

/// Split a properties line into key and value, honouring `=`, `:` and
/// escaped separators.
fn split_property(line: &str) -> Option<(String, String)> {
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    let mut key = String::new();
    while i < chars.len() {
        let c = chars[i];
        if c == '\\' && i + 1 < chars.len() {
            key.push(chars[i + 1]);
            i += 2;
            continue;
        }
        if c == '=' || c == ':' {
            let value: String = chars[i + 1..].iter().collect();
            return Some((key.trim().to_string(), value.trim().to_string()));
        }
        if c == ' ' && key.trim_end().len() == key.len() && !key.is_empty() {
            // `key value` is also a pair.
            let rest: String = chars[i + 1..].iter().collect();
            let rest = rest.trim_start();
            if !rest.starts_with('=') && !rest.starts_with(':') {
                return Some((key.trim().to_string(), rest.to_string()));
            }
        }
        key.push(c);
        i += 1;
    }
    None
}

fn unescape_property_value(v: &str) -> String {
    let chars: Vec<char> = v.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '\\' && i + 1 < chars.len() {
            match chars[i + 1] {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                'r' => out.push('\r'),
                'u' if i + 5 < chars.len() => {
                    let hex: String = chars[i + 2..i + 6].iter().collect();
                    if let Ok(v) = u32::from_str_radix(&hex, 16)
                        && let Some(c) = char::from_u32(v)
                    {
                        out.push(c);
                        i += 6;
                        continue;
                    }
                    out.push('u');
                }
                other => out.push(other),
            }
            i += 2;
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// One key's path: `a.b[0].c` to `["a", "b", "[0]", "c"]`.
fn key_path(key: &str) -> Vec<String> {
    let mut parts = Vec::new();
    for segment in key.split('.') {
        let mut rest = segment;
        while let Some(open) = rest.find('[') {
            if open > 0 {
                parts.push(rest[..open].to_string());
            }
            match rest[open..].find(']') {
                Some(close) => {
                    parts.push(rest[open..open + close + 1].to_string());
                    rest = &rest[open + close + 1..];
                }
                None => break,
            }
        }
        if !rest.is_empty() {
            parts.push(rest.to_string());
        }
    }
    parts
}

fn insert(node: &mut Node, path: &[String], value: String) {
    let Some((head, tail)) = path.split_first() else {
        *node = Node::Scalar(value);
        return;
    };
    if let Some(index) = head
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .and_then(|s| s.parse::<usize>().ok())
    {
        if !matches!(node, Node::Seq(_)) {
            *node = Node::Seq(Vec::new());
        }
        let Node::Seq(items) = node else { return };
        while items.len() <= index {
            items.push(Node::Scalar(String::new()));
        }
        insert(&mut items[index], tail, value);
        return;
    }
    if !matches!(node, Node::Map(_)) {
        *node = Node::Map(Vec::new());
    }
    let Node::Map(entries) = node else { return };
    if let Some(slot) = entries.iter_mut().find(|(k, _)| k == head) {
        insert(&mut slot.1, tail, value);
    } else {
        let mut child = Node::Scalar(String::new());
        insert(&mut child, tail, value);
        entries.push((head.clone(), child));
    }
}

/// `.properties` text to a tree, with its own problems (a line that is
/// not a pair).
pub fn parse_properties(text: &str) -> (Node, Vec<Problem>) {
    let mut root = Node::Map(Vec::new());
    let mut problems = Vec::new();
    let mut joined: Vec<(usize, String)> = Vec::new();
    let mut pending: Option<(usize, String)> = None;
    for (i, raw) in text.lines().enumerate() {
        let n = i + 1;
        let line = raw.trim_start();
        if let Some((start, mut acc)) = pending.take() {
            acc.push_str(line.trim_end_matches('\\'));
            if line.ends_with('\\') {
                pending = Some((start, acc));
            } else {
                joined.push((start, acc));
            }
            continue;
        }
        if line.is_empty() || line.starts_with('#') || line.starts_with('!') {
            continue;
        }
        if line.ends_with('\\') {
            pending = Some((n, line.trim_end_matches('\\').to_string()));
            continue;
        }
        joined.push((n, line.to_string()));
    }
    if let Some((n, acc)) = pending {
        joined.push((n, acc));
    }
    for (n, line) in joined {
        // Java's Properties.load accepts `key=v`, `key:v`, `key v` and a
        // bare `key` (empty value) alike; only a missing key is wrong.
        let (key, value) = split_property(&line).unwrap_or((line.clone(), String::new()));
        if key.is_empty() {
            problems.push(Problem::new(
                n,
                format!("no key before the separator in \"{line}\""),
            ));
            continue;
        }
        insert(&mut root, &key_path(&key), unescape_property_value(&value));
    }
    (root, problems)
}

// ---- emitting YAML ---------------------------------------------------------------

fn needs_quotes(v: &str) -> bool {
    if v.is_empty() {
        return false;
    }
    let first = v.chars().next().unwrap();
    v.trim() != v
        || v.contains(": ")
        || v.ends_with(':')
        || v.contains('#')
        || v.contains('\n')
        || matches!(
            first,
            '-' | '?'
                | ':'
                | ','
                | '['
                | ']'
                | '{'
                | '}'
                | '&'
                | '*'
                | '!'
                | '|'
                | '>'
                | '%'
                | '@'
                | '`'
                | '"'
                | '\''
        )
}

fn emit(node: &Node, indent: usize, out: &mut String) {
    let pad = "  ".repeat(indent);
    match node {
        Node::Scalar(v) => {
            out.push_str(&format!("{pad}{}\n", quote_if_needed(v)));
        }
        Node::Map(entries) => {
            for (k, v) in entries {
                match v {
                    Node::Scalar(s) => {
                        if s.contains('\n') {
                            out.push_str(&format!("{pad}{k}: |\n"));
                            for line in s.split('\n') {
                                out.push_str(&format!("{pad}  {line}\n"));
                            }
                        } else {
                            out.push_str(&format!("{pad}{k}: {}\n", quote_if_needed(s)));
                        }
                    }
                    Node::Map(inner) if inner.is_empty() => {
                        out.push_str(&format!("{pad}{k}: {{}}\n"));
                    }
                    Node::Seq(items) if items.is_empty() => {
                        out.push_str(&format!("{pad}{k}: []\n"));
                    }
                    _ => {
                        out.push_str(&format!("{pad}{k}:\n"));
                        emit(v, indent + 1, out);
                    }
                }
            }
        }
        Node::Seq(items) => {
            for item in items {
                match item {
                    Node::Scalar(s) => out.push_str(&format!("{pad}- {}\n", quote_if_needed(s))),
                    Node::Map(entries) => {
                        let mut first = true;
                        for (k, v) in entries {
                            let lead = if first { "- " } else { "  " };
                            first = false;
                            match v {
                                Node::Scalar(s) => out
                                    .push_str(&format!("{pad}{lead}{k}: {}\n", quote_if_needed(s))),
                                _ => {
                                    out.push_str(&format!("{pad}{lead}{k}:\n"));
                                    emit(v, indent + 2, out);
                                }
                            }
                        }
                    }
                    Node::Seq(_) => {
                        out.push_str(&format!("{pad}-\n"));
                        emit(item, indent + 1, out);
                    }
                }
            }
        }
    }
}

fn quote_if_needed(v: &str) -> String {
    if needs_quotes(v) {
        format!("\"{}\"", v.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        v.to_string()
    }
}

/// A tree back to YAML text.
pub fn to_yaml(node: &Node) -> String {
    let mut out = String::new();
    emit(node, 0, &mut out);
    out
}

// ---- UI --------------------------------------------------------------------------

pub fn ui(st: &mut State, ui: &mut egui::Ui, s: &Services, theme: &Theme) {
    form::page_title(ui, theme, &s.t("dt-yaml"), Some(&s.t("dt-yaml-sub")));
    ui.horizontal(|ui| {
        ui.selectable_value(&mut st.mode, Mode::Check, s.t("dt-yaml-check"));
        ui.selectable_value(
            &mut st.mode,
            Mode::YamlToProperties,
            s.t("dt-yaml-to-properties"),
        );
        ui.selectable_value(&mut st.mode, Mode::PropertiesToYaml, s.t("dt-yaml-to-yaml"));
    });
    ui.add_space(4.0);
    form::hint(ui, theme, &s.t("dt-yaml-subset-hint"));
    ui.add_space(8.0);
    form::section(ui, theme, &s.t("dt-input"), |ui| {
        util::input(
            ui,
            s,
            theme,
            &mut st.input,
            &s.t(if st.mode == Mode::PropertiesToYaml {
                "dt-yaml-properties-hint"
            } else {
                "dt-yaml-input-hint"
            }),
            10,
        );
    });
    let input = s.expand(&st.input).unwrap_or_else(|_| st.input.clone());
    let (problems, output, label) = match st.mode {
        Mode::Check => {
            let (_, problems) = parse_yaml(&input);
            (problems, String::new(), "")
        }
        Mode::YamlToProperties => {
            let (node, problems) = parse_yaml(&input);
            (problems, to_properties(&node), "properties")
        }
        Mode::PropertiesToYaml => {
            let (node, problems) = parse_properties(&input);
            (problems, to_yaml(&node), "YAML")
        }
    };
    ui.add_space(8.0);
    form::section(ui, theme, &s.t("dt-yaml-problems"), |ui| {
        if input.trim().is_empty() {
            form::hint(ui, theme, &s.t("dt-yaml-empty"));
        } else if problems.is_empty() {
            util::ok(ui, theme, &s.t("dt-yaml-ok"));
        } else {
            for p in &problems {
                util::error(
                    ui,
                    theme,
                    &s.i18n.t_args(
                        "dt-yaml-problem",
                        &[("line", &p.line.to_string()), ("message", &p.message)],
                    ),
                );
            }
        }
    });
    if st.mode != Mode::Check {
        ui.add_space(8.0);
        form::section(ui, theme, &s.t("dt-output"), |ui| {
            util::output(ui, s, theme, label, &output, 10);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(pairs: &[(&str, Node)]) -> Node {
        Node::Map(
            pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.clone()))
                .collect(),
        )
    }
    fn sc(v: &str) -> Node {
        Node::Scalar(v.to_string())
    }

    #[test]
    fn a_spring_style_document_parses_into_an_ordered_tree() {
        let src = "\
# the app
server:
  port: 8080
  servlet:
    context-path: /api
spring:
  datasource:
    url: jdbc:postgresql://db/shop
    password: \"p@ss: word\"
  profiles:
    active:
      - dev
      - local
features: [a, b]
empty:
";
        let (node, problems) = parse_yaml(src);
        assert!(problems.is_empty(), "{problems:?}");
        let Node::Map(top) = &node else {
            panic!("{node:?}")
        };
        assert_eq!(
            top.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(),
            ["server", "spring", "features", "empty"],
            "keys keep their order"
        );
        let props = to_properties(&node);
        assert!(props.contains("server.port=8080\n"), "{props}");
        assert!(
            props.contains("server.servlet.context-path=/api\n"),
            "{props}"
        );
        assert!(
            props.contains("spring.datasource.password=p@ss: word\n"),
            "a quoted value keeps its colon: {props}"
        );
        assert!(props.contains("spring.profiles.active[0]=dev\n"), "{props}");
        assert!(
            props.contains("spring.profiles.active[1]=local\n"),
            "{props}"
        );
        assert!(
            props.contains("features[0]=a\n") && props.contains("features[1]=b\n"),
            "{props}"
        );
        assert!(props.contains("empty=\n"), "{props}");
    }

    #[test]
    fn properties_become_yaml_and_back_again() {
        let props = "\
# a comment
server.port=8080
server.servlet.context-path=/api
spring.profiles.active[0]=dev
spring.profiles.active[1]=local
message=hello\\nworld
";
        let (node, problems) = parse_properties(props);
        assert!(problems.is_empty(), "{problems:?}");
        let yaml = to_yaml(&node);
        assert!(yaml.contains("server:\n  port: 8080\n"), "{yaml}");
        assert!(yaml.contains("    - dev\n"), "{yaml}");
        assert!(yaml.contains("message: |\n  hello\n  world\n"), "{yaml}");
        // Round trip: YAML → properties gives the same pairs back.
        let (back, problems) = parse_yaml(&yaml);
        assert!(problems.is_empty(), "{problems:?}");
        let again = to_properties(&back);
        for line in [
            "server.port=8080",
            "server.servlet.context-path=/api",
            "spring.profiles.active[0]=dev",
            "spring.profiles.active[1]=local",
            "message=hello\\nworld",
        ] {
            assert!(again.contains(line), "{line} missing from {again}");
        }
    }

    #[test]
    fn the_checker_names_the_line_and_the_mistake() {
        let (_, problems) = parse_yaml("a: 1\n\tb: 2\n");
        assert_eq!(problems[0].line, 2);
        assert!(problems[0].message.contains("tab"), "{problems:?}");

        let (_, problems) = parse_yaml("a: 1\nb\nc: 3\n");
        assert_eq!(problems[0].line, 2);
        assert!(problems[0].message.contains("key: value"), "{problems:?}");

        let (_, problems) = parse_yaml("a: 1\na: 2\n");
        assert!(problems[0].message.contains("twice"), "{problems:?}");

        let (_, problems) = parse_yaml("a: 1\n   b: 2\n");
        assert_eq!(problems[0].line, 2);
        assert!(problems[0].message.contains("indented"), "{problems:?}");

        let (_, problems) = parse_yaml("a: \"unclosed\n");
        assert!(problems[0].message.contains("quote"), "{problems:?}");

        let (_, problems) = parse_yaml("a: 1\nb:\n  c: 2\n");
        assert!(problems.is_empty(), "sound document: {problems:?}");
    }

    #[test]
    fn block_scalars_and_list_maps_survive_the_conversion() {
        let src = "\
script: |
  line one
  line two
servers:
  - name: a
    port: 1
  - name: b
    port: 2
";
        let (node, problems) = parse_yaml(src);
        assert!(problems.is_empty(), "{problems:?}");
        let props = to_properties(&node);
        assert!(props.contains("script=line one\\nline two\n"), "{props}");
        assert!(props.contains("servers[0].name=a\n"), "{props}");
        assert!(props.contains("servers[1].port=2\n"), "{props}");
        let (back, _) = parse_properties(&props);
        let yaml = to_yaml(&back);
        assert!(yaml.contains("- name: a\n"), "{yaml}");
        assert!(yaml.contains("  port: 2\n"), "{yaml}");
    }

    #[test]
    fn unsupported_yaml_is_reported_not_guessed() {
        let (_, problems) = parse_yaml("a: &anchor value\n");
        assert!(problems[0].message.contains("anchors"), "{problems:?}");
        let (_, problems) = parse_yaml("a: {b: 1}\n");
        assert!(problems[0].message.contains("flow map"), "{problems:?}");
    }

    #[test]
    fn property_lines_follow_javas_rules() {
        // `=`, `:`, a space and a bare key are all legal separators in
        // Java's Properties.load, so none of these is a mistake.
        let (node, problems) = parse_properties("a=1\nb:2\nc 3\nd\n");
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(
            node,
            map(&[
                ("a", sc("1")),
                ("b", sc("2")),
                ("c", sc("3")),
                ("d", sc(""))
            ])
        );
        // A line with no key before the separator is.
        let (_, problems) = parse_properties("=orphan\n");
        assert_eq!(problems.len(), 1);
        assert_eq!(problems[0].line, 1);
        // A continuation line joins the one before it.
        let (node, problems) = parse_properties("a=one \\\n  two\n");
        assert!(problems.is_empty());
        assert_eq!(node, map(&[("a", sc("one two"))]));
    }
}
