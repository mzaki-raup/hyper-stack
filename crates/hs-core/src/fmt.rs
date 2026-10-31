//! Pretty-printing for the data formats the tools show: JSON and XML
//! (HTML is close enough to be laid out the same way). Every function
//! returns `None` when the text is not that format or is already as good
//! as it gets, so a caller can keep what the user typed.

/// Re-indent JSON with two spaces. `None` when it does not parse.
///
/// The text is re-indented, not re-serialised: `serde_json::Value` is a
/// sorted map, so round-tripping a body through it would reorder the
/// user's keys (and rewrite their numbers). Only whitespace changes here.
pub fn pretty_json(text: &str) -> Option<String> {
    let t = text.trim();
    serde_json::from_str::<serde_json::Value>(t).ok()?;
    Some(respace_json(t, true))
}

/// JSON on one line (a compact body to send), key order kept.
pub fn minify_json(text: &str) -> Option<String> {
    let t = text.trim();
    serde_json::from_str::<serde_json::Value>(t).ok()?;
    Some(respace_json(t, false))
}

/// Re-lay-out valid JSON: `pretty` indents by two spaces per level, else
/// every space outside a string is dropped.
fn respace_json(text: &str, pretty: bool) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len() + text.len() / 4);
    let mut depth = 0usize;
    let mut in_str = false;
    let mut escaped = false;
    let mut i = 0;
    let indent = |out: &mut String, depth: usize| {
        out.push('\n');
        for _ in 0..depth {
            out.push_str("  ");
        }
    };
    while i < chars.len() {
        let c = chars[i];
        if in_str {
            out.push(c);
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_str = false;
            }
            i += 1;
            continue;
        }
        match c {
            '"' => {
                in_str = true;
                out.push(c);
            }
            '{' | '[' => {
                let close = if c == '{' { '}' } else { ']' };
                let mut j = i + 1;
                while j < chars.len() && chars[j].is_whitespace() {
                    j += 1;
                }
                out.push(c);
                // An empty object or array stays on one line.
                if chars.get(j) == Some(&close) {
                    out.push(close);
                    i = j + 1;
                    continue;
                }
                depth += 1;
                if pretty {
                    indent(&mut out, depth);
                }
            }
            '}' | ']' => {
                depth = depth.saturating_sub(1);
                if pretty {
                    indent(&mut out, depth);
                }
                out.push(c);
            }
            ',' => {
                out.push(c);
                if pretty {
                    indent(&mut out, depth);
                }
            }
            ':' => out.push_str(if pretty { ": " } else { ":" }),
            w if w.is_whitespace() => {}
            _ => out.push(c),
        }
        i += 1;
    }
    out
}

#[derive(Debug, PartialEq)]
enum Tok {
    /// `<name …>`
    Open(String),
    /// `</name>`
    Close(String),
    /// `<name …/>`, `<?xml …?>`, `<!-- … -->`, `<![CDATA[…]]>`, `<!DOCTYPE …>`
    Standalone(String),
    Text(String),
}

/// Split XML into tags and the text between them. `None` on anything
/// malformed (an unterminated tag, text before the first tag).
fn tokenize_xml(text: &str) -> Option<Vec<Tok>> {
    let b: Vec<char> = text.trim().chars().collect();
    if b.first() != Some(&'<') {
        return None;
    }
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] != '<' {
            let start = i;
            while i < b.len() && b[i] != '<' {
                i += 1;
            }
            let t: String = b[start..i].iter().collect();
            if !t.trim().is_empty() {
                out.push(Tok::Text(t.trim().to_string()));
            }
            continue;
        }
        // A tag; CDATA and comments may contain '>' and run to their own end.
        let rest: String = b[i..].iter().take(9).collect();
        let end = if rest.starts_with("<![CDATA[") {
            find_from(&b, i, "]]>")? + 3
        } else if rest.starts_with("<!--") {
            find_from(&b, i, "-->")? + 3
        } else {
            find_from(&b, i, ">")? + 1
        };
        let raw: String = b[i..end].iter().collect();
        i = end;
        let inner = raw.trim_start_matches('<').trim_end_matches('>');
        let name = inner
            .trim_start_matches('/')
            .split([' ', '\t', '\n', '\r', '/', '>'])
            .next()
            .unwrap_or("")
            .to_string();
        if raw.starts_with("</") {
            out.push(Tok::Close(name));
        } else if raw.starts_with("<?") || raw.starts_with("<!") || raw.ends_with("/>") {
            out.push(Tok::Standalone(raw));
        } else {
            out.push(Tok::Open(raw));
        }
    }
    (!out.is_empty()).then_some(out)
}

/// Index of `needle` in `b` at or after `from`, in chars.
fn find_from(b: &[char], from: usize, needle: &str) -> Option<usize> {
    let n: Vec<char> = needle.chars().collect();
    (from..b.len().saturating_sub(n.len() - 1)).find(|&j| b[j..j + n.len()] == n[..])
}

/// Re-indent XML (and HTML-shaped markup) with two spaces, keeping a
/// short text node on the line of its element. `None` when the text is
/// not markup or the tags do not nest.
pub fn pretty_xml(text: &str) -> Option<String> {
    pretty_xml_with(text, "  ")
}

/// Re-indent XML with `indent` for each level — two spaces, four, or a
/// tab, as the devtools XML page offers.
pub fn pretty_xml_with(text: &str, indent: &str) -> Option<String> {
    let toks = tokenize_xml(text)?;
    let mut out = String::new();
    let mut depth: usize = 0;
    let mut open_stack: Vec<String> = Vec::new();
    let mut i = 0;
    while i < toks.len() {
        match &toks[i] {
            Tok::Open(raw) => {
                let name = element_name(raw);
                // `<a>text</a>` stays on one line.
                if let (Some(Tok::Text(t)), Some(Tok::Close(c))) =
                    (toks.get(i + 1), toks.get(i + 2))
                    && *c == name
                    && t.len() <= 120
                    && !t.contains('\n')
                {
                    push_line(&mut out, depth, indent, &format!("{raw}{t}</{c}>"));
                    i += 3;
                    continue;
                }
                // `<a></a>` likewise.
                if let Some(Tok::Close(c)) = toks.get(i + 1)
                    && *c == name
                {
                    push_line(&mut out, depth, indent, &format!("{raw}</{c}>"));
                    i += 2;
                    continue;
                }
                push_line(&mut out, depth, indent, raw);
                open_stack.push(name);
                depth += 1;
            }
            Tok::Close(name) => {
                // Tags that do not nest (a stray close) mean this is not
                // something to re-indent.
                if open_stack.pop().as_deref() != Some(name.as_str()) {
                    return None;
                }
                depth = depth.saturating_sub(1);
                push_line(&mut out, depth, indent, &format!("</{name}>"));
            }
            Tok::Standalone(raw) => push_line(&mut out, depth, indent, raw),
            Tok::Text(t) => {
                for line in t.lines() {
                    let line = line.trim();
                    if !line.is_empty() {
                        push_line(&mut out, depth, indent, line);
                    }
                }
            }
        }
        i += 1;
    }
    if !open_stack.is_empty() {
        return None;
    }
    Some(out.trim_end().to_string())
}

fn element_name(raw: &str) -> String {
    raw.trim_start_matches('<')
        .trim_end_matches('>')
        .split([' ', '\t', '\n', '\r', '/'])
        .next()
        .unwrap_or("")
        .to_string()
}

fn push_line(out: &mut String, depth: usize, indent: &str, text: &str) {
    for _ in 0..depth {
        out.push_str(indent);
    }
    out.push_str(text);
    out.push('\n');
}

/// Pretty-print whatever the text looks like (JSON first, then markup).
/// `None` when nothing applies or the result equals the input.
pub fn pretty_auto(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    let out = match trimmed.chars().next()? {
        '{' | '[' => pretty_json(trimmed),
        '<' => pretty_xml(trimmed),
        _ => pretty_json(trimmed),
    }?;
    (out != trimmed).then_some(out)
}

/// Pretty-print for a known format name (`json`, `xml`, `html`, …).
pub fn pretty_as(text: &str, format: &str) -> Option<String> {
    let trimmed = text.trim();
    let out = match format.to_ascii_lowercase().as_str() {
        "json" | "jsonc" => pretty_json(trimmed),
        "xml" | "html" | "xhtml" | "svg" | "xsd" | "xsl" => pretty_xml(trimmed),
        _ => pretty_auto(trimmed),
    }?;
    (out != trimmed).then_some(out)
}

/// A message as a toast or a hint shows it: sentence case, so a driver's
/// "purged “orders”" reads "Purged “orders”". Only the first letter moves.
pub fn sentence(text: &str) -> String {
    let mut c = text.chars();
    match c.next() {
        Some(first) => first.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_message_starts_with_a_capital() {
        assert_eq!(sentence("purged “q”"), "Purged “q”");
        assert_eq!(sentence(""), "");
    }

    #[test]
    fn json_is_reindented_and_minified() {
        assert_eq!(
            pretty_json(r#"{"b":1,"a":[1,2]}"#).unwrap(),
            "{\n  \"b\": 1,\n  \"a\": [\n    1,\n    2\n  ]\n}",
            "keys stay in the author's order, not sorted"
        );
        assert_eq!(minify_json("{\n \"a\" : 1 \n}").unwrap(), r#"{"a":1}"#);
        assert!(pretty_json("not json").is_none());
    }

    #[test]
    fn json_layout_keeps_strings_and_empty_containers_intact() {
        let src = r#"{"msg":"a, b: {x} \" end","empty":{},"none":[],"n":1.50}"#;
        let out = pretty_json(src).unwrap();
        assert!(out.contains(r#""msg": "a, b: {x} \" end""#), "{out}");
        assert!(out.contains(r#""empty": {}"#), "{out}");
        assert!(out.contains(r#""none": []"#), "{out}");
        assert!(
            out.contains("\"n\": 1.50"),
            "numbers are not rewritten: {out}"
        );
        // Re-indenting an already pretty document changes nothing.
        assert_eq!(pretty_json(&out).unwrap(), out);
        assert_eq!(minify_json(&out).unwrap(), minify_json(src).unwrap());
    }

    #[test]
    fn xml_nests_and_keeps_short_text_inline() {
        let src = r#"<?xml version="1.0"?><order id="7"><item><name>Widget</name><qty>2</qty></item><note/></order>"#;
        assert_eq!(
            pretty_xml(src).unwrap(),
            "<?xml version=\"1.0\"?>\n<order id=\"7\">\n  <item>\n    <name>Widget</name>\n    <qty>2</qty>\n  </item>\n  <note/>\n</order>"
        );
    }

    #[test]
    fn xml_keeps_comments_and_cdata_whole() {
        let src = "<a><!-- a > b --><b><![CDATA[x > y]]></b></a>";
        assert_eq!(
            pretty_xml(src).unwrap(),
            "<a>\n  <!-- a > b -->\n  <b>\n    <![CDATA[x > y]]>\n  </b>\n</a>"
        );
    }

    #[test]
    fn malformed_markup_is_left_alone() {
        assert!(pretty_xml("<a><b></a></b>").is_none(), "crossed tags");
        assert!(pretty_xml("<a><b></b>").is_none(), "unclosed");
        assert!(pretty_xml("plain text").is_none());
        assert!(pretty_xml("<a").is_none(), "unterminated tag");
    }

    #[test]
    fn auto_picks_the_format_and_skips_no_op() {
        assert!(pretty_auto(r#"{"a":1}"#).is_some());
        assert!(pretty_auto("<a><b>1</b></a>").is_some());
        assert!(pretty_auto("").is_none());
        assert!(pretty_auto("hello").is_none());
        // Already pretty: nothing to do.
        let pretty = pretty_json(r#"{"a":1}"#).unwrap();
        assert!(pretty_auto(&pretty).is_none());
        assert_eq!(pretty_as("<a>1</a>", "xml"), None, "already one line");
        assert!(pretty_as(r#"{"a":[1]}"#, "json").is_some());
    }
}
