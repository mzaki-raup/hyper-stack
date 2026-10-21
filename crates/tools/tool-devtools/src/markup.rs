//! Checking and re-indenting TOML and XML.
//!
//! Both answer the same two questions: is this well formed, and if so what
//! does it look like laid out. The checking is separate from the
//! formatting so a file that fails says *where*, which is the whole point
//! of a checker.

/// How much to indent each level by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Indent {
    Two,
    #[default]
    Four,
    Tab,
}

impl Indent {
    pub fn text(self) -> &'static str {
        match self {
            Indent::Two => "  ",
            Indent::Four => "    ",
            Indent::Tab => "\t",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Indent::Two => "2 spaces",
            Indent::Four => "4 spaces",
            Indent::Tab => "Tab",
        }
    }
}

// ---- TOML ----------------------------------------------------------------

/// Whether the text parses as TOML, and where it stops if not.
pub fn check_toml(text: &str) -> Result<(), String> {
    if text.trim().is_empty() {
        return Err("nothing to check".into());
    }
    match text.parse::<toml::Table>() {
        Ok(_) => Ok(()),
        Err(e) => Err(toml_error(&e)),
    }
}

/// `toml`'s message already names the problem; this adds the line and
/// column when it knows them, because "expected a value" on its own sends
/// you hunting.
fn toml_error(e: &toml::de::Error) -> String {
    let msg = e.message().trim().to_string();
    match e.span() {
        Some(span) => format!("{msg} (byte {})", span.start),
        None => msg,
    }
}

/// TOML laid out again from its parsed form: keys grouped under their
/// tables, one pair to a line.
///
/// Unlike XML there is no indent to choose — TOML is not an
/// indentation-based format, and a table's keys sit at the left margin
/// whatever you do. The indent setting is ignored here on purpose rather
/// than silently doing nothing somewhere the user cannot see.
pub fn pretty_toml(text: &str) -> Result<String, String> {
    let table: toml::Table = text.parse().map_err(|e| toml_error(&e))?;
    toml::to_string_pretty(&table).map_err(|e| e.to_string())
}

// ---- XML -----------------------------------------------------------------

/// Whether the markup is well formed: every tag closed, in the right
/// order, and nothing left open. Reports the line and column of the first
/// problem.
///
/// This is well-formedness, not validity: no DTD or schema is consulted,
/// and the checker says so rather than implying more than it did.
pub fn check_xml(text: &str) -> Result<(), String> {
    if text.trim().is_empty() {
        return Err("nothing to check".into());
    }
    let b: Vec<char> = text.chars().collect();
    let mut stack: Vec<(String, usize, usize)> = Vec::new();
    let mut i = 0usize;
    let (mut line, mut col) = (1usize, 1usize);
    // Where each character sits, so a problem can be pointed at.
    let mut pos: Vec<(usize, usize)> = Vec::with_capacity(b.len());
    for c in &b {
        pos.push((line, col));
        if *c == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    let at = |i: usize| -> (usize, usize) { pos.get(i).copied().unwrap_or((line, col)) };

    while i < b.len() {
        if b[i] != '<' {
            i += 1;
            continue;
        }
        let start = i;
        // Comments, CDATA and declarations are skipped whole.
        if starts_with(&b, i, "<!--") {
            match find(&b, i + 4, "-->") {
                Some(j) => {
                    i = j + 3;
                    continue;
                }
                None => return Err(pointed(at(start), "a comment is never closed with -->")),
            }
        }
        if starts_with(&b, i, "<![CDATA[") {
            match find(&b, i + 9, "]]>") {
                Some(j) => {
                    i = j + 3;
                    continue;
                }
                None => return Err(pointed(at(start), "a CDATA section is never closed")),
            }
        }
        if starts_with(&b, i, "<?") || starts_with(&b, i, "<!") {
            match find(&b, i + 2, ">") {
                Some(j) => {
                    i = j + 1;
                    continue;
                }
                None => return Err(pointed(at(start), "a declaration is never closed with >")),
            }
        }
        let Some(end) = find(&b, i + 1, ">") else {
            return Err(pointed(at(start), "a tag is never closed with >"));
        };
        let raw: String = b[i + 1..end].iter().collect();
        i = end + 1;
        if let Some(rest) = raw.strip_prefix('/') {
            let name = tag_name(rest);
            match stack.pop() {
                Some((open, ..)) if open == name => {}
                Some((open, l, c)) => {
                    return Err(pointed(
                        at(start),
                        &format!(
                            "</{name}> closes nothing; <{open}> at line {l}, column {c} is still open"
                        ),
                    ));
                }
                None => {
                    return Err(pointed(at(start), &format!("</{name}> was never opened")));
                }
            }
        } else if !raw.ends_with('/') {
            let name = tag_name(&raw);
            if name.is_empty() {
                return Err(pointed(at(start), "a tag with no name"));
            }
            let (l, c) = at(start);
            stack.push((name, l, c));
        }
    }
    if let Some((name, l, c)) = stack.pop() {
        return Err(format!("<{name}> at line {l}, column {c} is never closed"));
    }
    Ok(())
}

/// Markup re-indented, after checking it: a formatter that silently
/// reshapes broken markup hides the break.
pub fn pretty_xml(text: &str, indent: Indent) -> Result<String, String> {
    check_xml(text)?;
    hs_core::fmt::pretty_xml_with(text, indent.text())
        .ok_or_else(|| "this does not look like markup".to_string())
}

fn pointed((line, col): (usize, usize), what: &str) -> String {
    format!("line {line}, column {col}: {what}")
}

fn starts_with(b: &[char], i: usize, s: &str) -> bool {
    let n: Vec<char> = s.chars().collect();
    b.len() >= i + n.len() && b[i..i + n.len()] == n[..]
}

fn find(b: &[char], from: usize, needle: &str) -> Option<usize> {
    let n: Vec<char> = needle.chars().collect();
    if n.is_empty() || b.len() < n.len() {
        return None;
    }
    (from..=b.len() - n.len()).find(|&j| b[j..j + n.len()] == n[..])
}

/// The element name out of a raw tag body (`a href="x"` -> `a`).
fn tag_name(raw: &str) -> String {
    raw.trim()
        .split([' ', '\t', '\n', '\r', '/'])
        .next()
        .unwrap_or("")
        .to_string()
}

/// Which of the two this page is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Toml,
    Xml,
}

#[derive(Default, serde::Serialize, serde::Deserialize, Clone)]
#[serde(default)]
pub struct State {
    pub input: String,
    pub indent: IndentSetting,
    /// The check and the pretty-printed result for the last (expanded)
    /// input and indent, so neither runs again on a frame where nothing
    /// changed: both walk the whole text and allocate a `Vec<char>` of it.
    #[serde(skip)]
    cache: Option<(u64, std::sync::Arc<Computed>)>,
}

struct Computed {
    check: Result<(), String>,
    out: Result<String, String>,
}

fn computed(st: &mut State, kind: Kind, text: &str) -> std::sync::Arc<Computed> {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut h);
    (kind as u8).hash(&mut h);
    (st.indent as u8).hash(&mut h);
    let key = h.finish();
    if let Some((k, c)) = &st.cache
        && *k == key
    {
        return c.clone();
    }
    let check = match kind {
        Kind::Toml => check_toml(text),
        Kind::Xml => check_xml(text),
    };
    let out = match (&check, kind) {
        (Err(e), _) => Err(e.clone()),
        (Ok(()), Kind::Toml) => pretty_toml(text),
        (Ok(()), Kind::Xml) => pretty_xml(text, st.indent.into()),
    };
    let c = std::sync::Arc::new(Computed { check, out });
    st.cache = Some((key, c.clone()));
    c
}

/// The stored form of [`Indent`]: an enum that serialises by name, so a
/// saved tab reopens with the indent it had.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum IndentSetting {
    Two,
    #[default]
    Four,
    Tab,
}

impl From<IndentSetting> for Indent {
    fn from(v: IndentSetting) -> Self {
        match v {
            IndentSetting::Two => Indent::Two,
            IndentSetting::Four => Indent::Four,
            IndentSetting::Tab => Indent::Tab,
        }
    }
}

pub fn ui(
    st: &mut State,
    kind: Kind,
    ui: &mut egui::Ui,
    s: &hs_core::Services,
    theme: &hs_ui::theme::Theme,
) {
    use hs_ui::form;
    let (title, sub, hint) = match kind {
        Kind::Toml => ("dt-toml", "dt-toml-sub", "dt-toml-hint"),
        Kind::Xml => ("dt-xml", "dt-xml-sub", "dt-xml-hint"),
    };
    form::page_title(ui, theme, &s.t(title), Some(&s.t(sub)));
    if kind == Kind::Xml {
        ui.horizontal(|ui| {
            ui.label(s.t("dt-indent"));
            for (setting, label) in [
                (IndentSetting::Two, Indent::Two.label()),
                (IndentSetting::Four, Indent::Four.label()),
                (IndentSetting::Tab, Indent::Tab.label()),
            ] {
                ui.selectable_value(&mut st.indent, setting, label);
            }
        });
        ui.add_space(4.0);
    }
    form::hint(ui, theme, &s.t(hint));
    ui.add_space(8.0);
    form::section(ui, theme, &s.t("dt-input"), |ui| {
        crate::util::input(ui, s, theme, &mut st.input, &s.t("dt-paste-hint"), 10);
    });
    let text = s.expand(&st.input).unwrap_or_else(|_| st.input.clone());
    let computed = computed(st, kind, &text);
    ui.add_space(8.0);
    form::section(ui, theme, &s.t("dt-result"), |ui| {
        match &computed.check {
            Ok(()) => crate::util::ok(ui, theme, &s.t("dt-markup-ok")),
            Err(e) => {
                ui.colored_label(theme.error, e);
                return;
            }
        }
        match &computed.out {
            Ok(formatted) => crate::util::output(ui, s, theme, &s.t("dt-formatted"), formatted, 14),
            Err(e) => {
                ui.colored_label(theme.error, e);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn good_toml_passes_and_comes_back_laid_out() {
        let src = "b=2\na=1\n[t]\nx='y'\n";
        assert!(check_toml(src).is_ok());
        let out = pretty_toml(src).unwrap();
        assert!(out.contains("a = 1"), "{out}");
        assert!(out.contains("[t]"), "{out}");
        // Laid out again, it still parses.
        assert!(check_toml(&out).is_ok());
    }

    #[test]
    fn broken_toml_says_what_is_wrong() {
        let e = check_toml("a = ").unwrap_err();
        assert!(!e.is_empty());
        assert!(check_toml("   ").is_err(), "empty input is not valid TOML");
        assert!(pretty_toml("a = [1, 2").is_err());
    }

    #[test]
    fn good_xml_passes_and_indents_the_way_it_was_asked() {
        let src = "<a><b>x</b></a>";
        assert!(check_xml(src).is_ok());
        let two = pretty_xml(src, Indent::Two).unwrap();
        let four = pretty_xml(src, Indent::Four).unwrap();
        let tab = pretty_xml(src, Indent::Tab).unwrap();
        assert!(two.contains("\n  <b>"), "{two:?}");
        assert!(four.contains("\n    <b>"), "{four:?}");
        assert!(tab.contains("\n\t<b>"), "{tab:?}");
    }

    #[test]
    fn declarations_comments_and_cdata_are_not_elements() {
        let src = "<?xml version=\"1.0\"?><!-- note --><a><![CDATA[<b>]]></a>";
        assert!(check_xml(src).is_ok(), "{:?}", check_xml(src));
    }

    #[test]
    fn self_closing_and_empty_elements_are_fine() {
        assert!(check_xml("<a><b/><c></c></a>").is_ok());
    }

    #[test]
    fn a_mismatched_tag_is_pointed_at() {
        let e = check_xml("<a>\n  <b>x</c>\n</a>").unwrap_err();
        assert!(e.contains("line 2"), "{e}");
        assert!(e.contains("</c>"), "{e}");
        assert!(e.contains("<b>"), "{e}");
    }

    #[test]
    fn an_unclosed_tag_is_named_with_where_it_started() {
        let e = check_xml("<a>\n  <b>x\n").unwrap_err();
        assert!(e.contains("<b>"), "{e}");
        assert!(e.contains("line 2"), "{e}");
        let e = check_xml("</a>").unwrap_err();
        assert!(e.contains("never opened"), "{e}");
    }

    #[test]
    fn an_unfinished_comment_or_tag_says_so() {
        assert!(
            check_xml("<a><!-- oops </a>")
                .unwrap_err()
                .contains("comment")
        );
        assert!(check_xml("<a").unwrap_err().contains("never closed"));
        assert!(
            check_xml("<a><![CDATA[x</a>")
                .unwrap_err()
                .contains("CDATA")
        );
    }

    #[test]
    fn broken_markup_is_never_quietly_reshaped() {
        // The formatter checks first: it must not hand back tidy-looking
        // output for markup that does not nest.
        assert!(pretty_xml("<a><b></a>", Indent::Two).is_err());
    }
}
