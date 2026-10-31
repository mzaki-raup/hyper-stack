//! The trackers' text formats and Markdown, converted at the edges
//! (ADR-029): Atlassian Document Format (Jira Cloud), Confluence storage
//! format (XHTML), Jira wiki markup (Data Center) and Textile (Redmine).
//!
//! Everything goes through one small tree, [`Block`] and [`Inline`], for
//! the subset every format shares with Markdown: paragraphs, headings,
//! lists, code, quotes, rules, tables, emphasis, code spans and links.
//! A reader that meets anything else keeps its text where it can and says
//! what it could not keep, so an edit is never sent over content it would
//! drop without the user being told.

use std::collections::BTreeSet;

use serde_json::{Value, json};

#[derive(Debug, Clone, PartialEq)]
pub enum Inline {
    Text(String),
    Code(String),
    Strong(Vec<Inline>),
    Em(Vec<Inline>),
    Strike(Vec<Inline>),
    Link(String, Vec<Inline>),
    Break,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Block {
    Para(Vec<Inline>),
    Heading(u8, Vec<Inline>),
    /// Language, text.
    Code(String, String),
    /// Ordered, items.
    List(bool, Vec<Vec<Block>>),
    Quote(Vec<Block>),
    Rule,
    /// Rows of cells; the first row is the header.
    Table(Vec<Vec<Vec<Inline>>>),
}

/// What a reader could not keep, by name, for the warning.
#[derive(Default)]
struct Lost(BTreeSet<String>);

impl Lost {
    fn add(&mut self, what: &str) {
        self.0.insert(what.to_string());
    }
    fn note(self) -> Option<String> {
        if self.0.is_empty() {
            None
        } else {
            Some(self.0.into_iter().collect::<Vec<_>>().join(", "))
        }
    }
}

// ---------------------------------------------------------------- Markdown

/// Markdown to the tree.
pub fn parse_md(md: &str) -> Vec<Block> {
    use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};

    enum Frame {
        Root(Vec<Block>),
        Quote(Vec<Block>),
        List(bool, Vec<Vec<Block>>),
        Item(Vec<Block>),
        Table(Vec<Vec<Vec<Inline>>>),
        Row(Vec<Vec<Inline>>),
    }
    enum Mark {
        /// `true` when no paragraph tag opened it (a tight list item).
        Para(bool),
        Heading(u8),
        Cell,
        Strong,
        Em,
        Strike,
        Link(String),
    }

    fn push_block(frames: &mut [Frame], b: Block) {
        if let Some(Frame::Root(v) | Frame::Quote(v) | Frame::Item(v)) = frames.last_mut() {
            v.push(b);
        }
    }
    fn push_inline(marks: &mut Vec<(Mark, Vec<Inline>)>, i: Inline) {
        if marks.is_empty() {
            marks.push((Mark::Para(true), Vec::new()));
        }
        if let Some((_, v)) = marks.last_mut() {
            v.push(i);
        }
    }
    /// Close the inline frame on top, into its parent or as a block.
    fn close(frames: &mut [Frame], marks: &mut Vec<(Mark, Vec<Inline>)>) {
        let Some((m, v)) = marks.pop() else { return };
        let done = match m {
            Mark::Para(_) => {
                push_block(frames, Block::Para(v));
                return;
            }
            Mark::Heading(l) => {
                push_block(frames, Block::Heading(l, v));
                return;
            }
            Mark::Cell => {
                if let Some(Frame::Row(r)) = frames.last_mut() {
                    r.push(v);
                }
                return;
            }
            Mark::Strong => Inline::Strong(v),
            Mark::Em => Inline::Em(v),
            Mark::Strike => Inline::Strike(v),
            Mark::Link(u) => Inline::Link(u, v),
        };
        push_inline(marks, done);
    }
    fn close_implicit(frames: &mut [Frame], marks: &mut Vec<(Mark, Vec<Inline>)>) {
        if matches!(marks.first(), Some((Mark::Para(true), _))) {
            while !marks.is_empty() {
                close(frames, marks);
            }
        }
    }

    let mut frames = vec![Frame::Root(Vec::new())];
    let mut marks: Vec<(Mark, Vec<Inline>)> = Vec::new();
    let mut code: Option<(String, String)> = None;
    let opts = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH;
    for ev in Parser::new_ext(md, opts) {
        if let Some((_, text)) = code.as_mut() {
            match ev {
                Event::Text(t) => {
                    text.push_str(&t);
                    continue;
                }
                Event::End(TagEnd::CodeBlock) => {
                    let (lang, mut text) = code.take().unwrap_or_default();
                    if text.ends_with('\n') {
                        text.pop();
                    }
                    push_block(&mut frames, Block::Code(lang, text));
                    continue;
                }
                _ => continue,
            }
        }
        match ev {
            Event::Start(tag) => match tag {
                Tag::Paragraph => {
                    close_implicit(&mut frames, &mut marks);
                    marks.push((Mark::Para(false), Vec::new()));
                }
                Tag::Heading { level, .. } => marks.push((Mark::Heading(level as u8), Vec::new())),
                Tag::BlockQuote(_) => {
                    close_implicit(&mut frames, &mut marks);
                    frames.push(Frame::Quote(Vec::new()));
                }
                Tag::CodeBlock(kind) => {
                    close_implicit(&mut frames, &mut marks);
                    let lang = match kind {
                        CodeBlockKind::Fenced(l) => {
                            l.split_whitespace().next().unwrap_or("").to_string()
                        }
                        CodeBlockKind::Indented => String::new(),
                    };
                    code = Some((lang, String::new()));
                }
                Tag::List(start) => {
                    close_implicit(&mut frames, &mut marks);
                    frames.push(Frame::List(start.is_some(), Vec::new()));
                }
                Tag::Item => frames.push(Frame::Item(Vec::new())),
                Tag::Table(_) => frames.push(Frame::Table(Vec::new())),
                Tag::TableHead | Tag::TableRow => frames.push(Frame::Row(Vec::new())),
                Tag::TableCell => marks.push((Mark::Cell, Vec::new())),
                Tag::Emphasis => marks.push((Mark::Em, Vec::new())),
                Tag::Strong => marks.push((Mark::Strong, Vec::new())),
                Tag::Strikethrough => marks.push((Mark::Strike, Vec::new())),
                Tag::Link { dest_url, .. } | Tag::Image { dest_url, .. } => {
                    marks.push((Mark::Link(dest_url.to_string()), Vec::new()))
                }
                _ => {}
            },
            Event::End(tag) => match tag {
                TagEnd::Paragraph
                | TagEnd::Heading(_)
                | TagEnd::TableCell
                | TagEnd::Emphasis
                | TagEnd::Strong
                | TagEnd::Strikethrough
                | TagEnd::Link
                | TagEnd::Image => close(&mut frames, &mut marks),
                TagEnd::BlockQuote(_) => {
                    close_implicit(&mut frames, &mut marks);
                    if let Some(Frame::Quote(v)) = frames.pop() {
                        push_block(&mut frames, Block::Quote(v));
                    }
                }
                TagEnd::Item => {
                    close_implicit(&mut frames, &mut marks);
                    if let Some(Frame::Item(v)) = frames.pop()
                        && let Some(Frame::List(_, items)) = frames.last_mut()
                    {
                        items.push(v);
                    }
                }
                TagEnd::List(_) => {
                    if let Some(Frame::List(o, items)) = frames.pop() {
                        push_block(&mut frames, Block::List(o, items));
                    }
                }
                TagEnd::TableHead | TagEnd::TableRow => {
                    if let Some(Frame::Row(r)) = frames.pop()
                        && let Some(Frame::Table(rows)) = frames.last_mut()
                    {
                        rows.push(r);
                    }
                }
                TagEnd::Table => {
                    if let Some(Frame::Table(rows)) = frames.pop() {
                        push_block(&mut frames, Block::Table(rows));
                    }
                }
                _ => {}
            },
            Event::Text(t) | Event::Html(t) | Event::InlineHtml(t) => {
                push_inline(&mut marks, Inline::Text(t.to_string()))
            }
            Event::Code(t) => push_inline(&mut marks, Inline::Code(t.to_string())),
            Event::SoftBreak => push_inline(&mut marks, Inline::Text(" ".into())),
            Event::HardBreak => push_inline(&mut marks, Inline::Break),
            Event::Rule => {
                close_implicit(&mut frames, &mut marks);
                push_block(&mut frames, Block::Rule);
            }
            _ => {}
        }
    }
    while !marks.is_empty() {
        close(&mut frames, &mut marks);
    }
    match frames.into_iter().next() {
        Some(Frame::Root(v)) => v,
        _ => Vec::new(),
    }
}

/// Neighbouring text runs as one, so readers that split text differently
/// compare equal.
fn merge(v: Vec<Inline>) -> Vec<Inline> {
    let mut out: Vec<Inline> = Vec::with_capacity(v.len());
    for i in v {
        let i = match i {
            Inline::Strong(c) => Inline::Strong(merge(c)),
            Inline::Em(c) => Inline::Em(merge(c)),
            Inline::Strike(c) => Inline::Strike(merge(c)),
            Inline::Link(u, c) => Inline::Link(u, merge(c)),
            other => other,
        };
        match (out.last_mut(), i) {
            (Some(Inline::Text(a)), Inline::Text(b)) => a.push_str(&b),
            (_, i) => out.push(i),
        }
    }
    out
}

fn md_escape(t: &str) -> String {
    let mut out = String::with_capacity(t.len());
    for c in t.chars() {
        if matches!(
            c,
            '\\' | '*' | '_' | '`' | '[' | ']' | '<' | '>' | '|' | '~'
        ) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

fn md_inline(v: &[Inline]) -> String {
    let mut out = String::new();
    for i in v {
        match i {
            Inline::Text(t) => out.push_str(&md_escape(t)),
            Inline::Code(c) => {
                if c.contains('`') {
                    out.push_str(&format!("`` {c} ``"));
                } else {
                    out.push_str(&format!("`{c}`"));
                }
            }
            Inline::Strong(c) => out.push_str(&format!("**{}**", md_inline(c))),
            Inline::Em(c) => out.push_str(&format!("*{}*", md_inline(c))),
            Inline::Strike(c) => out.push_str(&format!("~~{}~~", md_inline(c))),
            Inline::Link(u, c) => out.push_str(&format!("[{}]({u})", md_inline(c))),
            Inline::Break => out.push_str("\\\n"),
        }
    }
    out
}

/// A paragraph that would read as a block marker is escaped.
fn md_para(v: &[Inline]) -> String {
    let s = md_inline(v);
    let t = s.trim_start();
    let digits = t.chars().take_while(char::is_ascii_digit).count();
    let marker = t.starts_with('#')
        || t.starts_with("- ")
        || t.starts_with("+ ")
        || t.starts_with("---")
        || t.starts_with("===")
        || (digits > 0 && matches!(t[digits..].chars().next(), Some('.' | ')')));
    if marker { format!("\\{s}") } else { s }
}

fn indent(s: &str, first: &str) -> String {
    let pad = " ".repeat(first.chars().count());
    let mut out = String::new();
    for (n, line) in s.lines().enumerate() {
        if n > 0 {
            out.push('\n');
        }
        if n == 0 {
            out.push_str(first);
        } else if !line.is_empty() {
            out.push_str(&pad);
        }
        out.push_str(line);
    }
    out
}

/// The tree as Markdown, the one way every reader writes it.
pub fn to_md(blocks: &[Block]) -> String {
    let mut parts = Vec::with_capacity(blocks.len());
    for b in blocks {
        parts.push(match b {
            Block::Para(v) => md_para(v),
            Block::Heading(l, v) => {
                format!("{} {}", "#".repeat((*l).clamp(1, 6) as usize), md_inline(v))
            }
            Block::Code(lang, text) => {
                let fence = if text.contains("```") { "````" } else { "```" };
                format!("{fence}{lang}\n{text}\n{fence}")
            }
            Block::List(ordered, items) => items
                .iter()
                .enumerate()
                .map(|(n, item)| {
                    let marker = if *ordered {
                        format!("{}. ", n + 1)
                    } else {
                        "- ".to_string()
                    };
                    // Tight: an item's blocks are not separated by blank lines.
                    let body = item
                        .iter()
                        .map(|b| to_md(std::slice::from_ref(b)))
                        .collect::<Vec<_>>()
                        .join("\n");
                    indent(&body, &marker)
                })
                .collect::<Vec<_>>()
                .join("\n"),
            Block::Quote(v) => to_md(v)
                .lines()
                .map(|l| {
                    if l.is_empty() {
                        ">".to_string()
                    } else {
                        format!("> {l}")
                    }
                })
                .collect::<Vec<_>>()
                .join("\n"),
            Block::Rule => "---".to_string(),
            Block::Table(rows) => {
                let width = rows.iter().map(Vec::len).max().unwrap_or(0);
                let mut out = Vec::with_capacity(rows.len() + 1);
                for (n, r) in rows.iter().enumerate() {
                    let mut cells: Vec<String> =
                        r.iter().map(|c| md_inline(c).replace('\n', " ")).collect();
                    cells.resize(width, String::new());
                    out.push(format!("| {} |", cells.join(" | ")));
                    if n == 0 {
                        out.push(format!("|{}", " --- |".repeat(width)));
                    }
                }
                out.join("\n")
            }
        });
    }
    parts.join("\n\n")
}

/// Markdown as every reader writes it: what a round trip is compared to.
pub fn canonical_md(md: &str) -> String {
    to_md(&normalise(parse_md(md)))
}

fn normalise(blocks: Vec<Block>) -> Vec<Block> {
    blocks
        .into_iter()
        .map(|b| match b {
            Block::Para(v) => Block::Para(merge(v)),
            Block::Heading(l, v) => Block::Heading(l, merge(v)),
            Block::List(o, items) => Block::List(o, items.into_iter().map(normalise).collect()),
            Block::Quote(v) => Block::Quote(normalise(v)),
            Block::Table(rows) => Block::Table(
                rows.into_iter()
                    .map(|r| r.into_iter().map(merge).collect())
                    .collect(),
            ),
            other => other,
        })
        .collect()
}

// ------------------------------------------------- Atlassian Document Format

/// A Jira Cloud document as Markdown, and what it could not keep.
pub fn adf_to_md(doc: &Value) -> (String, Option<String>) {
    let mut lost = Lost::default();
    let blocks = adf_blocks(doc.get("content"), &mut lost);
    (to_md(&normalise(blocks)), lost.note())
}

fn adf_blocks(content: Option<&Value>, lost: &mut Lost) -> Vec<Block> {
    let mut out = Vec::new();
    for n in content.and_then(Value::as_array).into_iter().flatten() {
        let ty = n.get("type").and_then(Value::as_str).unwrap_or("");
        let kids = n.get("content");
        match ty {
            "paragraph" => out.push(Block::Para(adf_inlines(kids, lost))),
            "heading" => {
                let l = n
                    .pointer("/attrs/level")
                    .and_then(Value::as_u64)
                    .unwrap_or(1) as u8;
                out.push(Block::Heading(l, adf_inlines(kids, lost)));
            }
            "codeBlock" => {
                let lang = n
                    .pointer("/attrs/language")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let text = kids
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|t| t.get("text").and_then(Value::as_str))
                    .collect::<String>();
                out.push(Block::Code(lang, text));
            }
            "bulletList" | "orderedList" => {
                let items = kids
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .map(|li| adf_blocks(li.get("content"), lost))
                    .collect();
                out.push(Block::List(ty == "orderedList", items));
            }
            "blockquote" => out.push(Block::Quote(adf_blocks(kids, lost))),
            "rule" => out.push(Block::Rule),
            "table" => {
                let rows = kids
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .map(|row| {
                        row.get("content")
                            .and_then(Value::as_array)
                            .into_iter()
                            .flatten()
                            .map(|cell| cell_inlines(adf_blocks(cell.get("content"), lost), lost))
                            .collect()
                    })
                    .collect();
                out.push(Block::Table(rows));
            }
            _ => {
                // Panels, expands, layouts, media: keep the text inside,
                // and say what was there.
                lost.add(ty);
                out.extend(adf_blocks(kids, lost));
            }
        }
    }
    out
}

/// A table cell holds blocks in ADF and storage format, a line in Markdown.
fn cell_inlines(blocks: Vec<Block>, lost: &mut Lost) -> Vec<Inline> {
    let mut out = Vec::new();
    for (n, b) in blocks.into_iter().enumerate() {
        if n > 0 {
            out.push(Inline::Break);
        }
        match b {
            Block::Para(v) | Block::Heading(_, v) => out.extend(v),
            _ => lost.add("a block inside a table cell"),
        }
    }
    out
}

fn adf_inlines(content: Option<&Value>, lost: &mut Lost) -> Vec<Inline> {
    let mut out = Vec::new();
    for n in content.and_then(Value::as_array).into_iter().flatten() {
        let ty = n.get("type").and_then(Value::as_str).unwrap_or("");
        let attr = |k: &str| {
            n.pointer(&format!("/attrs/{k}"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string()
        };
        match ty {
            "text" => {
                let text = n
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let marks: Vec<&Value> = n
                    .get("marks")
                    .and_then(Value::as_array)
                    .map(|m| m.iter().collect())
                    .unwrap_or_default();
                let has = |t: &str| {
                    marks
                        .iter()
                        .any(|m| m.get("type").and_then(Value::as_str) == Some(t))
                };
                let mut i = if has("code") {
                    Inline::Code(text)
                } else {
                    Inline::Text(text)
                };
                for m in &marks {
                    match m.get("type").and_then(Value::as_str).unwrap_or("") {
                        "strong" => i = Inline::Strong(vec![i]),
                        "em" => i = Inline::Em(vec![i]),
                        "strike" => i = Inline::Strike(vec![i]),
                        "code" | "link" => {}
                        other => lost.add(other),
                    }
                }
                if let Some(href) = marks
                    .iter()
                    .find(|m| m.get("type").and_then(Value::as_str) == Some("link"))
                    .and_then(|m| m.pointer("/attrs/href"))
                    .and_then(Value::as_str)
                {
                    i = Inline::Link(href.to_string(), vec![i]);
                }
                out.push(i);
            }
            "hardBreak" => out.push(Inline::Break),
            "mention" => {
                lost.add("mention");
                out.push(Inline::Text(attr("text")));
            }
            "emoji" => {
                lost.add("emoji");
                let t = attr("text");
                out.push(Inline::Text(if t.is_empty() {
                    attr("shortName")
                } else {
                    t
                }));
            }
            "inlineCard" => {
                lost.add("inlineCard");
                let u = attr("url");
                out.push(Inline::Link(u.clone(), vec![Inline::Text(u)]));
            }
            _ => {
                lost.add(ty);
                let t = attr("text");
                if !t.is_empty() {
                    out.push(Inline::Text(t));
                }
            }
        }
    }
    out
}

/// Markdown as an Atlassian Document Format document.
pub fn md_to_adf(md: &str) -> Value {
    json!({ "type": "doc", "version": 1, "content": adf_of_blocks(&parse_md(md)) })
}

fn adf_of_blocks(blocks: &[Block]) -> Vec<Value> {
    blocks
        .iter()
        .map(|b| match b {
            Block::Para(v) => adf_node("paragraph", adf_of_inlines(v)),
            Block::Heading(l, v) => {
                let mut n = adf_node("heading", adf_of_inlines(v));
                n["attrs"] = json!({ "level": l });
                n
            }
            Block::Code(lang, text) => {
                let content = if text.is_empty() {
                    Vec::new()
                } else {
                    vec![json!({ "type": "text", "text": text })]
                };
                let mut n = adf_node("codeBlock", content);
                if !lang.is_empty() {
                    n["attrs"] = json!({ "language": lang });
                }
                n
            }
            Block::List(ordered, items) => adf_node(
                if *ordered {
                    "orderedList"
                } else {
                    "bulletList"
                },
                items
                    .iter()
                    .map(|i| adf_node("listItem", adf_of_blocks(i)))
                    .collect(),
            ),
            Block::Quote(v) => adf_node("blockquote", adf_of_blocks(v)),
            Block::Rule => json!({ "type": "rule" }),
            Block::Table(rows) => adf_node(
                "table",
                rows.iter()
                    .enumerate()
                    .map(|(n, r)| {
                        let cell = if n == 0 { "tableHeader" } else { "tableCell" };
                        adf_node(
                            "tableRow",
                            r.iter()
                                .map(|c| {
                                    adf_node(cell, vec![adf_node("paragraph", adf_of_inlines(c))])
                                })
                                .collect(),
                        )
                    })
                    .collect(),
            ),
        })
        .collect()
}

fn adf_node(ty: &str, content: Vec<Value>) -> Value {
    if content.is_empty() {
        json!({ "type": ty })
    } else {
        json!({ "type": ty, "content": content })
    }
}

fn adf_of_inlines(v: &[Inline]) -> Vec<Value> {
    fn walk(v: &[Inline], marks: &mut Vec<Value>, out: &mut Vec<Value>) {
        for i in v {
            let with = |m: Value, c: &[Inline], marks: &mut Vec<Value>, out: &mut Vec<Value>| {
                marks.push(m);
                walk(c, marks, out);
                marks.pop();
            };
            match i {
                Inline::Text(t) | Inline::Code(t) if t.is_empty() => {}
                Inline::Text(t) => out.push(text(t, marks)),
                Inline::Code(t) => {
                    // ADF lets code sit only with a link.
                    let mut m: Vec<Value> = marks
                        .iter()
                        .filter(|m| m["type"] == "link")
                        .cloned()
                        .collect();
                    m.push(json!({ "type": "code" }));
                    out.push(text(t, &m));
                }
                Inline::Strong(c) => with(json!({ "type": "strong" }), c, marks, out),
                Inline::Em(c) => with(json!({ "type": "em" }), c, marks, out),
                Inline::Strike(c) => with(json!({ "type": "strike" }), c, marks, out),
                Inline::Link(u, c) => with(
                    json!({ "type": "link", "attrs": { "href": u } }),
                    c,
                    marks,
                    out,
                ),
                Inline::Break => out.push(json!({ "type": "hardBreak" })),
            }
        }
    }
    fn text(t: &str, marks: &[Value]) -> Value {
        if marks.is_empty() {
            json!({ "type": "text", "text": t })
        } else {
            json!({ "type": "text", "text": t, "marks": marks })
        }
    }
    let mut out = Vec::new();
    walk(v, &mut Vec::new(), &mut out);
    out
}

// ------------------------------------------------- Confluence storage format

#[derive(Debug, Clone, PartialEq)]
enum Node {
    El(String, Vec<(String, String)>, Vec<Node>),
    Text(String),
}

fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let Some(end) = rest.find(';').filter(|e| *e < 12) else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let name = &rest[1..end];
        let c = match name {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some('\u{a0}'),
            n if n.starts_with("#x") => u32::from_str_radix(&n[2..], 16)
                .ok()
                .and_then(char::from_u32),
            n if n.starts_with('#') => n[1..].parse().ok().and_then(char::from_u32),
            _ => None,
        };
        match c {
            Some(c) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// A forgiving XHTML reader: storage format is well formed, but a
/// stray `<br>` must not swallow the rest of the page.
/// An open element: name, attributes, children so far.
type Open = (String, Vec<(String, String)>, Vec<Node>);

fn parse_xml(s: &str) -> Vec<Node> {
    const VOID: &[&str] = &["br", "hr", "img", "col"];
    let mut stack: Vec<Open> = vec![(String::new(), Vec::new(), Vec::new())];
    let mut rest = s;
    let push = |stack: &mut Vec<Open>, n: Node| {
        if let Some(top) = stack.last_mut() {
            top.2.push(n);
        }
    };
    while !rest.is_empty() {
        let Some(lt) = rest.find('<') else {
            push(&mut stack, Node::Text(decode_entities(rest)));
            break;
        };
        if lt > 0 {
            push(&mut stack, Node::Text(decode_entities(&rest[..lt])));
        }
        rest = &rest[lt..];
        if let Some(r) = rest.strip_prefix("<![CDATA[") {
            let end = r.find("]]>").unwrap_or(r.len());
            push(&mut stack, Node::Text(r[..end].to_string()));
            rest = r.get(end + 3..).unwrap_or("");
            continue;
        }
        if let Some(r) = rest.strip_prefix("<!--") {
            rest = r.find("-->").map(|e| &r[e + 3..]).unwrap_or("");
            continue;
        }
        let Some(gt) = rest.find('>') else {
            push(&mut stack, Node::Text(decode_entities(rest)));
            break;
        };
        let tag = &rest[1..gt];
        rest = &rest[gt + 1..];
        if let Some(name) = tag.strip_prefix('/') {
            let name = name.trim();
            if let Some(pos) = stack.iter().rposition(|f| f.0 == name)
                && pos > 0
            {
                while stack.len() > pos {
                    let (n, a, k) = stack.pop().expect("frame above pos");
                    push(&mut stack, Node::El(n, a, k));
                }
            }
            continue;
        }
        let self_closing = tag.ends_with('/');
        let tag = tag.trim_end_matches('/');
        let name_end = tag.find(char::is_whitespace).unwrap_or(tag.len());
        let name = tag[..name_end].to_string();
        let attrs = parse_attrs(&tag[name_end..]);
        if self_closing || VOID.contains(&name.as_str()) {
            push(&mut stack, Node::El(name, attrs, Vec::new()));
        } else {
            stack.push((name, attrs, Vec::new()));
        }
    }
    while stack.len() > 1 {
        let (n, a, k) = stack.pop().expect("frame");
        push(&mut stack, Node::El(n, a, k));
    }
    stack.pop().map(|f| f.2).unwrap_or_default()
}

fn parse_attrs(s: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut rest = s.trim();
    while let Some(eq) = rest.find('=') {
        let name = rest[..eq].trim().to_string();
        let after = rest[eq + 1..].trim_start();
        let Some(q) = after.chars().next().filter(|c| *c == '"' || *c == '\'') else {
            break;
        };
        let body = &after[1..];
        let end = body.find(q).unwrap_or(body.len());
        out.push((name, decode_entities(&body[..end])));
        rest = body.get(end + 1..).unwrap_or("").trim_start();
    }
    out
}

fn attr<'a>(a: &'a [(String, String)], k: &str) -> &'a str {
    a.iter()
        .find(|(n, _)| n == k)
        .map(|(_, v)| v.as_str())
        .unwrap_or("")
}

fn node_text(nodes: &[Node]) -> String {
    nodes
        .iter()
        .map(|n| match n {
            Node::Text(t) => t.clone(),
            Node::El(_, _, k) => node_text(k),
        })
        .collect()
}

fn is_block(name: &str) -> bool {
    matches!(
        name,
        "p" | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "pre"
            | "ul"
            | "ol"
            | "blockquote"
            | "hr"
            | "table"
            | "div"
            | "ac:structured-macro"
            | "ac:layout"
            | "ac:layout-section"
            | "ac:layout-cell"
            | "ac:task-list"
    )
}

/// A Confluence page's storage format as Markdown, and what it could not keep.
pub fn storage_to_md(xhtml: &str) -> (String, Option<String>) {
    let mut lost = Lost::default();
    let blocks = xml_blocks(&parse_xml(xhtml), &mut lost);
    (to_md(&normalise(blocks)), lost.note())
}

fn xml_blocks(nodes: &[Node], lost: &mut Lost) -> Vec<Block> {
    let mut out = Vec::new();
    let mut run: Vec<Inline> = Vec::new();
    let flush = |run: &mut Vec<Inline>, out: &mut Vec<Block>| {
        let v = trim_inlines(std::mem::take(run));
        if !v.is_empty() {
            out.push(Block::Para(v));
        }
    };
    for n in nodes {
        let Node::El(name, attrs, kids) = n else {
            run.extend(xml_inlines(std::slice::from_ref(n), lost));
            continue;
        };
        if !is_block(name) {
            run.extend(xml_inlines(std::slice::from_ref(n), lost));
            continue;
        }
        flush(&mut run, &mut out);
        match name.as_str() {
            "p" => {
                let v = trim_inlines(xml_inlines(kids, lost));
                if !v.is_empty() {
                    out.push(Block::Para(v));
                }
            }
            h if h.len() == 2 && h.starts_with('h') && h[1..].parse::<u8>().is_ok() => {
                let l = h[1..].parse().unwrap_or(1);
                out.push(Block::Heading(l, trim_inlines(xml_inlines(kids, lost))));
            }
            "pre" => out.push(Block::Code(String::new(), node_text(kids))),
            "ul" | "ol" => {
                let items = kids
                    .iter()
                    .filter_map(|k| match k {
                        Node::El(n, _, c) if n == "li" => Some(xml_blocks(c, lost)),
                        _ => None,
                    })
                    .collect();
                out.push(Block::List(name == "ol", items));
            }
            "blockquote" => out.push(Block::Quote(xml_blocks(kids, lost))),
            "hr" => out.push(Block::Rule),
            "table" => {
                let mut rows = Vec::new();
                collect_rows(kids, &mut rows, lost);
                out.push(Block::Table(rows));
            }
            "ac:structured-macro" if attr(attrs, "ac:name") == "code" => {
                let mut lang = String::new();
                let mut body = String::new();
                for k in kids {
                    if let Node::El(n, a, c) = k {
                        if n == "ac:parameter" && attr(a, "ac:name") == "language" {
                            lang = node_text(c);
                        } else if n == "ac:plain-text-body" {
                            body = node_text(c);
                        }
                    }
                }
                out.push(Block::Code(lang, body));
            }
            "ac:structured-macro" => {
                lost.add(&format!("the {} macro", attr(attrs, "ac:name")));
                out.extend(xml_blocks(kids, lost));
            }
            other => {
                lost.add(other);
                out.extend(xml_blocks(kids, lost));
            }
        }
    }
    flush(&mut run, &mut out);
    out
}

fn collect_rows(nodes: &[Node], rows: &mut Vec<Vec<Vec<Inline>>>, lost: &mut Lost) {
    for n in nodes {
        if let Node::El(name, _, kids) = n {
            match name.as_str() {
                "tbody" | "thead" | "tfoot" => collect_rows(kids, rows, lost),
                "tr" => {
                    let row = kids
                        .iter()
                        .filter_map(|c| match c {
                            Node::El(n, _, k) if n == "td" || n == "th" => {
                                let blocks = xml_blocks(k, lost);
                                Some(cell_inlines(blocks, lost))
                            }
                            _ => None,
                        })
                        .collect();
                    rows.push(row);
                }
                "colgroup" | "col" => {}
                other => lost.add(other),
            }
        }
    }
}

fn xml_inlines(nodes: &[Node], lost: &mut Lost) -> Vec<Inline> {
    let mut out = Vec::new();
    for n in nodes {
        match n {
            Node::Text(t) => {
                // Whitespace in XHTML collapses.
                let mut s = String::with_capacity(t.len());
                let mut space = false;
                for c in t.chars() {
                    if c.is_whitespace() && c != '\u{a0}' {
                        space = true;
                    } else {
                        if space {
                            s.push(' ');
                            space = false;
                        }
                        s.push(c);
                    }
                }
                if space {
                    s.push(' ');
                }
                if !s.is_empty() {
                    out.push(Inline::Text(s));
                }
            }
            Node::El(name, attrs, kids) => match name.as_str() {
                "strong" | "b" => out.push(Inline::Strong(xml_inlines(kids, lost))),
                "em" | "i" => out.push(Inline::Em(xml_inlines(kids, lost))),
                "s" | "del" | "strike" => out.push(Inline::Strike(xml_inlines(kids, lost))),
                "code" => out.push(Inline::Code(node_text(kids))),
                "a" => out.push(Inline::Link(
                    attr(attrs, "href").to_string(),
                    xml_inlines(kids, lost),
                )),
                "br" => out.push(Inline::Break),
                "span" => out.extend(xml_inlines(kids, lost)),
                other => {
                    lost.add(other);
                    out.extend(xml_inlines(kids, lost));
                }
            },
        }
    }
    out
}

fn trim_inlines(mut v: Vec<Inline>) -> Vec<Inline> {
    v = merge(v);
    if let Some(Inline::Text(t)) = v.first_mut() {
        *t = t.trim_start().to_string();
    }
    if let Some(Inline::Text(t)) = v.last_mut() {
        *t = t.trim_end().to_string();
    }
    v.retain(|i| !matches!(i, Inline::Text(t) if t.is_empty()));
    v
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Markdown as Confluence storage format.
pub fn md_to_storage(md: &str) -> String {
    storage_of_blocks(&parse_md(md))
}

fn storage_of_blocks(blocks: &[Block]) -> String {
    let mut out = String::new();
    for b in blocks {
        match b {
            Block::Para(v) => out.push_str(&format!("<p>{}</p>", storage_of_inlines(v))),
            Block::Heading(l, v) => {
                let l = (*l).clamp(1, 6);
                out.push_str(&format!("<h{l}>{}</h{l}>", storage_of_inlines(v)));
            }
            Block::Code(lang, text) => {
                out.push_str(r#"<ac:structured-macro ac:name="code">"#);
                if !lang.is_empty() {
                    out.push_str(&format!(
                        r#"<ac:parameter ac:name="language">{}</ac:parameter>"#,
                        xml_escape(lang)
                    ));
                }
                out.push_str(&format!(
                    "<ac:plain-text-body><![CDATA[{}]]></ac:plain-text-body></ac:structured-macro>",
                    text.replace("]]>", "]]]]><![CDATA[>")
                ));
            }
            Block::List(ordered, items) => {
                let tag = if *ordered { "ol" } else { "ul" };
                out.push_str(&format!("<{tag}>"));
                for item in items {
                    out.push_str("<li>");
                    match item.as_slice() {
                        [Block::Para(v)] => out.push_str(&storage_of_inlines(v)),
                        blocks => out.push_str(&storage_of_blocks(blocks)),
                    }
                    out.push_str("</li>");
                }
                out.push_str(&format!("</{tag}>"));
            }
            Block::Quote(v) => out.push_str(&format!(
                "<blockquote>{}</blockquote>",
                storage_of_blocks(v)
            )),
            Block::Rule => out.push_str("<hr />"),
            Block::Table(rows) => {
                out.push_str("<table><tbody>");
                for (n, r) in rows.iter().enumerate() {
                    let cell = if n == 0 { "th" } else { "td" };
                    out.push_str("<tr>");
                    for c in r {
                        out.push_str(&format!("<{cell}>{}</{cell}>", storage_of_inlines(c)));
                    }
                    out.push_str("</tr>");
                }
                out.push_str("</tbody></table>");
            }
        }
    }
    out
}

fn storage_of_inlines(v: &[Inline]) -> String {
    let mut out = String::new();
    for i in v {
        match i {
            Inline::Text(t) => out.push_str(&xml_escape(t)),
            Inline::Code(t) => out.push_str(&format!("<code>{}</code>", xml_escape(t))),
            Inline::Strong(c) => {
                out.push_str(&format!("<strong>{}</strong>", storage_of_inlines(c)))
            }
            Inline::Em(c) => out.push_str(&format!("<em>{}</em>", storage_of_inlines(c))),
            Inline::Strike(c) => out.push_str(&format!("<s>{}</s>", storage_of_inlines(c))),
            Inline::Link(u, c) => out.push_str(&format!(
                r#"<a href="{}">{}</a>"#,
                xml_escape(u),
                storage_of_inlines(c)
            )),
            Inline::Break => out.push_str("<br />"),
        }
    }
    out
}

// ------------------------------------------------ Jira wiki markup, Textile

/// Jira Data Center's wiki markup and Redmine's Textile: close cousins,
/// read line by line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lite {
    JiraWiki,
    Textile,
}

/// Jira wiki markup or Textile as Markdown, and what it could not keep.
pub fn lite_to_md(src: &str, d: Lite) -> (String, Option<String>) {
    let mut lost = Lost::default();
    let mut out: Vec<String> = Vec::new();
    let lines: Vec<&str> = src.lines().map(|l| l.trim_end_matches('\r')).collect();
    let mut i = 0;
    // The widths of the open list markers, for nesting.
    let mut table_open = false;
    while i < lines.len() {
        let line = lines[i];
        let t = line.trim();
        // Code blocks.
        if let Some((lang, end)) = code_start(t, d) {
            let mut body = Vec::new();
            i += 1;
            while i < lines.len() && !lines[i].trim().starts_with(end) {
                body.push(lines[i]);
                i += 1;
            }
            out.push(format!("```{lang}\n{}\n```", body.join("\n")));
            out.push(String::new());
            i += 1;
            table_open = false;
            continue;
        }
        if d == Lite::Textile && t.starts_with("bc. ") {
            out.push(format!("```\n{}\n```", &t[4..]));
            out.push(String::new());
            i += 1;
            continue;
        }
        // Tables.
        if t.starts_with('|') {
            let header = match d {
                Lite::JiraWiki => t.starts_with("||"),
                Lite::Textile => t.contains("|_."),
            };
            let cells = table_cells(t, d)
                .iter()
                .map(|c| lite_inline(c, d, &mut lost))
                .collect::<Vec<_>>();
            if !table_open && !header {
                // Markdown wants a header row.
                out.push(format!("|{}", " |".repeat(cells.len())));
                out.push(format!("|{}", " --- |".repeat(cells.len())));
            }
            out.push(format!("| {} |", cells.join(" | ")));
            if !table_open && header {
                out.push(format!("|{}", " --- |".repeat(cells.len())));
            }
            table_open = true;
            i += 1;
            continue;
        }
        if table_open {
            out.push(String::new());
            table_open = false;
        }
        // Headings and quotes.
        if t.len() > 3
            && t.starts_with('h')
            && t[1..2].parse::<u8>().is_ok_and(|l| (1..=6).contains(&l))
            && t[2..].starts_with(". ")
        {
            let l: usize = t[1..2].parse().unwrap_or(1);
            out.push(format!(
                "{} {}",
                "#".repeat(l),
                lite_inline(&t[4..], d, &mut lost)
            ));
            i += 1;
            continue;
        }
        if let Some(q) = t.strip_prefix("bq. ") {
            out.push(format!("> {}", lite_inline(q, d, &mut lost)));
            i += 1;
            continue;
        }
        if t == "----" {
            out.push("---".into());
            i += 1;
            continue;
        }
        if d == Lite::JiraWiki && (t == "{quote}" || t.starts_with("{quote}")) {
            // A {quote} block: its lines quoted.
            i += 1;
            while i < lines.len() && !lines[i].trim().starts_with("{quote}") {
                out.push(format!("> {}", lite_inline(lines[i].trim(), d, &mut lost)));
                i += 1;
            }
            i += 1;
            continue;
        }
        // Lists: `*`, `#` (and Jira's `-`) repeated for depth.
        let marks: String = t.chars().take_while(|c| matches!(c, '*' | '#')).collect();
        let jira_dash = d == Lite::JiraWiki && t.starts_with("- ");
        if (!marks.is_empty() && t[marks.len()..].starts_with(' ')) || jira_dash {
            let (marks, rest) = if jira_dash {
                ("*".to_string(), &t[2..])
            } else {
                (marks.clone(), &t[marks.len() + 1..])
            };
            let mut pad = String::new();
            for c in marks.chars().take(marks.chars().count() - 1) {
                pad.push_str(if c == '#' { "   " } else { "  " });
            }
            let marker = if marks.ends_with('#') { "1. " } else { "- " };
            out.push(format!("{pad}{marker}{}", lite_inline(rest, d, &mut lost)));
            i += 1;
            continue;
        }
        if t.is_empty() {
            out.push(String::new());
        } else {
            // Textile's paragraph signatures.
            let t = t.strip_prefix("p. ").unwrap_or(t);
            out.push(lite_inline(t, d, &mut lost));
        }
        i += 1;
    }
    // One blank line between blocks, however many the source had.
    let mut md = String::new();
    for l in out {
        if l.is_empty() && (md.is_empty() || md.ends_with("\n\n")) {
            continue;
        }
        md.push_str(&l);
        md.push('\n');
    }
    let md = md.trim().to_string();
    (md, lost.note())
}

fn code_start(t: &str, d: Lite) -> Option<(String, &'static str)> {
    match d {
        Lite::JiraWiki => {
            if let Some(r) = t.strip_prefix("{code") {
                let lang = r
                    .trim_start_matches(':')
                    .trim_end_matches('}')
                    .split('|')
                    .next()
                    .unwrap_or("")
                    .trim();
                let lang = if lang.contains('=') { "" } else { lang };
                Some((lang.to_string(), "{code}"))
            } else if t.starts_with("{noformat") {
                Some((String::new(), "{noformat}"))
            } else {
                None
            }
        }
        Lite::Textile => {
            if let Some(r) = t.strip_prefix("<pre><code class=\"") {
                let lang = r.split('"').next().unwrap_or("");
                Some((lang.to_string(), "</code></pre>"))
            } else if t.starts_with("<pre>") {
                Some((String::new(), "</pre>"))
            } else {
                None
            }
        }
    }
}

fn table_cells(t: &str, d: Lite) -> Vec<String> {
    let t = t.trim();
    let sep = if d == Lite::JiraWiki && t.starts_with("||") {
        "||"
    } else {
        "|"
    };
    let inner = t.trim_start_matches(sep).trim_end_matches(sep);
    inner
        .split(sep)
        .map(|c| c.trim().trim_start_matches("_.").trim().to_string())
        .collect()
}

/// One line's inline markup as Markdown.
fn lite_inline(s: &str, d: Lite, lost: &mut Lost) -> String {
    let (open, close) = match d {
        Lite::JiraWiki => ("{{", "}}"),
        Lite::Textile => ("@", "@"),
    };
    // Code spans first, so nothing inside them is converted.
    let mut out = String::new();
    let mut rest = s;
    while let Some(a) = rest.find(open) {
        let after = &rest[a + open.len()..];
        let Some(b) = after.find(close).filter(|b| *b > 0) else {
            break;
        };
        out.push_str(&lite_text(&rest[..a], d, lost));
        out.push('`');
        out.push_str(&after[..b]);
        out.push('`');
        rest = &after[b + close.len()..];
    }
    out.push_str(&lite_text(rest, d, lost));
    out
}

fn lite_text(s: &str, d: Lite, lost: &mut Lost) -> String {
    let mut s = s.to_string();
    match d {
        Lite::JiraWiki => {
            if s.contains("[~") {
                lost.add("mention");
            }
            if s.contains('!') && s.split('!').count() > 2 {
                lost.add("image");
            }
            if s.contains("{color") || s.contains("{panel") || s.contains("{status") {
                lost.add("macro");
            }
            s = links(&s, '[', ']', |inner| match inner.split_once('|') {
                Some((t, u)) => format!("[{t}]({u})"),
                None if inner.starts_with('~') => format!("@{}", &inner[1..]),
                None => format!("<{inner}>"),
            });
            s = s.replace("\\\\", "\\\n");
        }
        Lite::Textile => {
            if s.contains('!') && s.split('!').count() > 2 {
                lost.add("image");
            }
            if s.contains("%{") || s.contains("{{") {
                lost.add("style or macro");
            }
            s = textile_links(&s);
        }
    }
    s = pairs(&s, '*', "**", "**");
    s = pairs(&s, '_', "*", "*");
    s = pairs(&s, '-', "~~", "~~");
    s
}

/// `[…]` spans through `f`.
fn links(s: &str, open: char, close: char, f: impl Fn(&str) -> String) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(a) = rest.find(open) {
        let Some(b) = rest[a..].find(close) else {
            break;
        };
        out.push_str(&rest[..a]);
        out.push_str(&f(&rest[a + 1..a + b]));
        rest = &rest[a + b + 1..];
    }
    out.push_str(rest);
    out
}

/// `"text":url`, the URL ending before trailing punctuation.
fn textile_links(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(a) = rest.find('"') {
        let after = &rest[a + 1..];
        let Some(b) = after.find("\":") else { break };
        let text = &after[..b];
        let url_part = &after[b + 2..];
        let end = url_part.find(char::is_whitespace).unwrap_or(url_part.len());
        let mut url = &url_part[..end];
        let trail = url.len()
            - url
                .trim_end_matches(['.', ',', ';', ':', '!', '?', ')'])
                .len();
        url = &url[..url.len() - trail];
        if url.is_empty() || text.contains('"') {
            out.push_str(&rest[..a + 1]);
            rest = after;
            continue;
        }
        out.push_str(&rest[..a]);
        out.push_str(&format!("[{text}]({url})"));
        rest = &url_part[url.len()..];
    }
    out.push_str(rest);
    out
}

/// `*x*` → `**x**` where the markers sit at word edges, as the wiki
/// formats require.
fn pairs(s: &str, delim: char, open: &str, close: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let edge_before =
        |i: usize| i == 0 || chars[i - 1].is_whitespace() || "([{'\"".contains(chars[i - 1]);
    let edge_after = |i: usize| {
        i + 1 >= chars.len()
            || chars[i + 1].is_whitespace()
            || ".,;:!?)]}'\"".contains(chars[i + 1])
    };
    let mut out = String::with_capacity(s.len() + 8);
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == delim
            && edge_before(i)
            && chars
                .get(i + 1)
                .is_some_and(|c| !c.is_whitespace() && *c != delim)
            && let Some(j) = (i + 2..chars.len())
                .find(|&j| chars[j] == delim && !chars[j - 1].is_whitespace() && edge_after(j))
        {
            out.push_str(open);
            out.extend(&chars[i + 1..j]);
            out.push_str(close);
            i = j + 1;
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// Markdown as Jira wiki markup or Textile.
pub fn md_to_lite(md: &str, d: Lite) -> String {
    lite_of_blocks(&parse_md(md), d, "")
}

fn lite_of_blocks(blocks: &[Block], d: Lite, list: &str) -> String {
    let mut parts = Vec::with_capacity(blocks.len());
    for b in blocks {
        parts.push(match b {
            Block::Para(v) => lite_of_inlines(v, d),
            Block::Heading(l, v) => format!("h{}. {}", (*l).clamp(1, 6), lite_of_inlines(v, d)),
            Block::Code(lang, text) => match (d, lang.is_empty()) {
                (Lite::JiraWiki, true) => format!("{{code}}\n{text}\n{{code}}"),
                (Lite::JiraWiki, false) => format!("{{code:{lang}}}\n{text}\n{{code}}"),
                (Lite::Textile, true) => format!("<pre>\n{text}\n</pre>"),
                (Lite::Textile, false) => {
                    format!("<pre><code class=\"{lang}\">\n{text}\n</code></pre>")
                }
            },
            Block::List(ordered, items) => {
                let marks = format!("{list}{}", if *ordered { '#' } else { '*' });
                items
                    .iter()
                    .map(|item| {
                        let mut lines = Vec::new();
                        for b in item {
                            match b {
                                Block::List(..) => {
                                    lines.push(lite_of_blocks(std::slice::from_ref(b), d, &marks))
                                }
                                other => lines.push(format!(
                                    "{marks} {}",
                                    lite_of_blocks(std::slice::from_ref(other), d, "")
                                )),
                            }
                        }
                        lines.join("\n")
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            }
            Block::Quote(v) => v
                .iter()
                .map(|b| format!("bq. {}", lite_of_blocks(std::slice::from_ref(b), d, "")))
                .collect::<Vec<_>>()
                .join("\n\n"),
            Block::Rule => "----".into(),
            Block::Table(rows) => rows
                .iter()
                .enumerate()
                .map(|(n, r)| {
                    let cells: Vec<String> = r.iter().map(|c| lite_of_inlines(c, d)).collect();
                    match (d, n == 0) {
                        (Lite::JiraWiki, true) => format!("||{}||", cells.join("||")),
                        (Lite::Textile, true) => format!("|_. {}|", cells.join("|_. ")),
                        _ => format!("|{}|", cells.join("|")),
                    }
                })
                .collect::<Vec<_>>()
                .join("\n"),
        });
    }
    let sep = if list.is_empty() { "\n\n" } else { "\n" };
    parts.join(sep)
}

fn lite_of_inlines(v: &[Inline], d: Lite) -> String {
    let mut out = String::new();
    for i in v {
        match i {
            Inline::Text(t) => out.push_str(t),
            Inline::Code(t) => match d {
                Lite::JiraWiki => out.push_str(&format!("{{{{{t}}}}}")),
                Lite::Textile => out.push_str(&format!("@{t}@")),
            },
            Inline::Strong(c) => out.push_str(&format!("*{}*", lite_of_inlines(c, d))),
            Inline::Em(c) => out.push_str(&format!("_{}_", lite_of_inlines(c, d))),
            Inline::Strike(c) => out.push_str(&format!("-{}-", lite_of_inlines(c, d))),
            Inline::Link(u, c) => match d {
                Lite::JiraWiki => out.push_str(&format!("[{}|{u}]", lite_of_inlines(c, d))),
                Lite::Textile => out.push_str(&format!("\"{}\":{u}", lite_of_inlines(c, d))),
            },
            Inline::Break => match d {
                Lite::JiraWiki => out.push_str("\\\\ "),
                Lite::Textile => out.push('\n'),
            },
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "# Release notes\n\nThe **build** is *green* and ~~red~~ is gone; see [the run](https://ci.example.com/42) and `cargo test`.\n\n- one\n- two\n  1. nested\n  2. again\n- three\n\n```rust\nfn main() {}\n```\n\n> quoted\n\n---\n\n| Name | State |\n| --- | --- |\n| api | up |";

    #[test]
    fn markdown_round_trips_through_its_own_tree() {
        let once = canonical_md(SAMPLE);
        assert_eq!(canonical_md(&once), once);
        assert!(once.contains("[the run](https://ci.example.com/42)"));
        assert!(once.contains("  1. nested"));
    }

    #[test]
    fn markdown_round_trips_through_adf_with_nothing_lost() {
        let adf = md_to_adf(SAMPLE);
        assert_eq!(adf["type"], "doc");
        let (back, lost) = adf_to_md(&adf);
        assert_eq!(back, canonical_md(SAMPLE));
        assert_eq!(lost, None);
    }

    #[test]
    fn adf_with_a_panel_and_a_mention_keeps_the_text_and_names_what_it_lost() {
        let doc = json!({ "type": "doc", "version": 1, "content": [
            { "type": "panel", "attrs": { "panelType": "info" }, "content": [
                { "type": "paragraph", "content": [
                    { "type": "text", "text": "Ask " },
                    { "type": "mention", "attrs": { "id": "1", "text": "@Ada" } },
                    { "type": "text", "text": " first", "marks": [{ "type": "underline" }] }
                ]}
            ]}
        ]});
        let (md, lost) = adf_to_md(&doc);
        assert_eq!(md, "Ask @Ada first");
        assert_eq!(lost.as_deref(), Some("mention, panel, underline"));
    }

    #[test]
    fn code_in_adf_keeps_only_a_link_beside_it() {
        let adf = md_to_adf("**`x`**");
        let marks = &adf["content"][0]["content"][0]["marks"];
        assert_eq!(marks, &json!([{ "type": "code" }]));
    }

    #[test]
    fn markdown_round_trips_through_confluence_storage() {
        let xhtml = md_to_storage(SAMPLE);
        assert!(xhtml.contains(r#"<ac:structured-macro ac:name="code"><ac:parameter ac:name="language">rust</ac:parameter>"#));
        let (back, lost) = storage_to_md(&xhtml);
        assert_eq!(back, canonical_md(SAMPLE));
        assert_eq!(lost, None);
    }

    #[test]
    fn a_page_with_a_macro_says_so() {
        let page = r#"<p>Intro &amp; more&nbsp;text</p><ac:structured-macro ac:name="info"><ac:rich-text-body><p>Careful</p></ac:rich-text-body></ac:structured-macro><p>A <ac:link><ri:page ri:content-title="Other" /></ac:link></p>"#;
        let (md, lost) = storage_to_md(page);
        assert!(md.starts_with("Intro & more\u{a0}text"), "{md}");
        assert!(md.contains("Careful"));
        let lost = lost.expect("lossy");
        assert!(lost.contains("the info macro"), "{lost}");
        assert!(lost.contains("ac:link"), "{lost}");
    }

    #[test]
    fn a_stray_unclosed_tag_does_not_swallow_the_page() {
        let (md, _) = storage_to_md("<p>one<br>two</p><p>three</p>");
        assert_eq!(md, "one\\\ntwo\n\nthree");
    }

    #[test]
    fn jira_wiki_reads_as_markdown() {
        let wiki = "h2. Steps\n\n# open the *app*\n## pick _Settings_\n* see {{config.toml}}\n\n{code:java}\nint x = 1;\n{code}\n\nSee [the docs|https://example.com/docs] or [https://example.com].\n\n||Name||State||\n|api|up|";
        let (md, lost) = lite_to_md(wiki, Lite::JiraWiki);
        assert_eq!(lost, None);
        assert_eq!(
            md,
            "## Steps\n\n1. open the **app**\n   1. pick *Settings*\n- see `config.toml`\n\n```java\nint x = 1;\n```\n\nSee [the docs](https://example.com/docs) or <https://example.com>.\n\n| Name | State |\n| --- | --- |\n| api | up |"
        );
    }

    #[test]
    fn markdown_round_trips_through_jira_wiki_and_textile() {
        let md = "## Steps\n\nThe **build** is *green*, see [the run](https://ci.example.com/42) and `cargo test`.\n\n- one\n- two\n\n```rust\nfn main() {}\n```\n\n| Name | State |\n| --- | --- |\n| api | up |";
        for d in [Lite::JiraWiki, Lite::Textile] {
            let lite = md_to_lite(md, d);
            let (back, lost) = lite_to_md(&lite, d);
            assert_eq!(lost, None, "{d:?}");
            assert_eq!(canonical_md(&back), canonical_md(md), "{d:?}: {lite}");
        }
    }

    #[test]
    fn textile_reads_as_markdown() {
        let t = "h1. Title\n\np. Plain *bold* and _em_ with @code@ and \"a link\":https://example.com.\n\n<pre>\nraw\n</pre>\n\n|_. A|_. B|\n|1|2|";
        let (md, lost) = lite_to_md(t, Lite::Textile);
        assert_eq!(lost, None);
        assert_eq!(
            md,
            "# Title\n\nPlain **bold** and *em* with `code` and [a link](https://example.com).\n\n```\nraw\n```\n\n| A | B |\n| --- | --- |\n| 1 | 2 |"
        );
    }

    #[test]
    fn a_word_with_a_hyphen_or_an_underscore_is_left_alone() {
        let (md, _) = lite_to_md("well-known snake_case a - b", Lite::Textile);
        assert_eq!(md, "well-known snake_case a - b");
    }
}
