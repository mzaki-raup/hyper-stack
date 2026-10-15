//! URL encoding: turn the unsafe, reserved and non-ASCII characters of a
//! URL into percent escapes so it survives a request, and back again.
//!
//! Three modes, because "encode a URL" means different things:
//! - **Component** escapes everything a value may not contain, for one
//!   query value, a path segment or a form field.
//! - **Full URL** keeps the characters that make a URL a URL (`:/?#&=`)
//!   and escapes the rest, for a link that was typed with spaces or
//!   accented letters.
//! - **Form** is Component with spaces as `+` (`application/x-www-form-
//!   urlencoded`, what a browser sends).
//!
//! The parts table splits a URL into scheme, host, path and query so each
//! query value can be read decoded.

use hs_core::Services;
use hs_ui::form;
use hs_ui::theme::Theme;
use serde::{Deserialize, Serialize};

use crate::util;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Mode {
    #[default]
    Encode,
    Decode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Scope {
    /// One value: escape everything outside the unreserved set.
    #[default]
    Component,
    /// A whole URL: keep the delimiters that give it structure.
    Full,
    /// `application/x-www-form-urlencoded`: Component, space as `+`.
    Form,
}

#[derive(Default, Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct State {
    pub mode: Mode,
    pub scope: Scope,
    pub input: String,
    /// Also escape the unreserved `-._~` (some servers want it).
    pub strict: bool,
}

/// RFC 3986 unreserved: never escaped.
fn unreserved(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~')
}

/// RFC 3986 reserved: the delimiters that give a URL its structure.
fn reserved(b: u8) -> bool {
    matches!(
        b,
        b':' | b'/'
            | b'?'
            | b'#'
            | b'['
            | b']'
            | b'@'
            | b'!'
            | b'$'
            | b'&'
            | b'\''
            | b'('
            | b')'
            | b'*'
            | b'+'
            | b','
            | b';'
            | b'='
            | b'%'
    )
}

/// Percent-encode `text` for `scope`. `strict` escapes `-._~` as well.
pub fn encode(text: &str, scope: Scope, strict: bool) -> String {
    let mut out = String::with_capacity(text.len() + 8);
    for &b in text.as_bytes() {
        let keep = match scope {
            Scope::Component | Scope::Form => {
                unreserved(b) && !(strict && !b.is_ascii_alphanumeric())
            }
            // A whole URL keeps its delimiters, and an existing escape
            // (`%20`) is left alone rather than turned into `%2520`.
            Scope::Full => unreserved(b) || reserved(b),
        };
        if keep {
            out.push(b as char);
        } else if scope == Scope::Form && b == b' ' {
            out.push('+');
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Decode percent escapes. In `Form` scope `+` becomes a space. Invalid
/// escapes are left as they are, so nothing is silently lost.
pub fn decode(text: &str, scope: Scope) -> String {
    let b = text.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' if i + 2 < b.len() => match u8::from_str_radix(&text[i + 1..i + 3], 16) {
                Ok(v) => {
                    out.push(v);
                    i += 3;
                }
                Err(_) => {
                    out.push(b[i]);
                    i += 1;
                }
            },
            b'+' if scope == Scope::Form => {
                out.push(b' ');
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A URL split for the parts table: `(label, value)` rows, query values
/// decoded. Empty when the text does not look like a URL.
pub fn parts(url: &str) -> Vec<(String, String)> {
    let url = url.trim();
    let mut out = Vec::new();
    let Some((scheme, rest)) = url.split_once("://") else {
        return out;
    };
    if scheme.is_empty()
        || !scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '+')
    {
        return out;
    }
    out.push(("Scheme".to_string(), scheme.to_string()));
    let (authority, after) = match rest.find(['/', '?', '#']) {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, ""),
    };
    out.push(("Host".to_string(), authority.to_string()));
    let (path_and_query, fragment) = match after.split_once('#') {
        Some((a, f)) => (a, Some(f)),
        None => (after, None),
    };
    let (path, query) = match path_and_query.split_once('?') {
        Some((p, q)) => (p, Some(q)),
        None => (path_and_query, None),
    };
    if !path.is_empty() {
        out.push(("Path".to_string(), decode(path, Scope::Component)));
    }
    if let Some(q) = query.filter(|q| !q.is_empty()) {
        for pair in q.split('&').filter(|p| !p.is_empty()) {
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            out.push((decode(k, Scope::Form), decode(v, Scope::Form)));
        }
    }
    if let Some(f) = fragment.filter(|f| !f.is_empty()) {
        out.push(("Fragment".to_string(), decode(f, Scope::Component)));
    }
    out
}

pub fn ui(st: &mut State, ui: &mut egui::Ui, s: &Services, theme: &Theme) {
    form::page_title(ui, theme, &s.t("dt-url"), Some(&s.t("dt-url-sub")));
    ui.horizontal(|ui| {
        ui.selectable_value(&mut st.mode, Mode::Encode, s.t("dt-url-encode"));
        ui.selectable_value(&mut st.mode, Mode::Decode, s.t("dt-url-decode"));
        ui.separator();
        ui.selectable_value(&mut st.scope, Scope::Component, s.t("dt-url-component"));
        ui.selectable_value(&mut st.scope, Scope::Full, s.t("dt-url-full"));
        ui.selectable_value(&mut st.scope, Scope::Form, s.t("dt-url-form"));
    });
    ui.add_space(4.0);
    form::hint(
        ui,
        theme,
        &s.t(match st.scope {
            Scope::Component => "dt-url-component-hint",
            Scope::Full => "dt-url-full-hint",
            Scope::Form => "dt-url-form-hint",
        }),
    );
    ui.add_space(8.0);
    form::section(ui, theme, &s.t("dt-input"), |ui| {
        util::input(
            ui,
            s,
            theme,
            &mut st.input,
            &s.t(if st.mode == Mode::Encode {
                "dt-url-encode-hint"
            } else {
                "dt-url-decode-hint"
            }),
            5,
        );
        if st.mode == Mode::Encode && st.scope != Scope::Full {
            ui.checkbox(&mut st.strict, s.t("dt-url-strict"));
            form::hint(ui, theme, &s.t("dt-url-strict-hint"));
        }
    });
    let input = s.expand(&st.input).unwrap_or_else(|_| st.input.clone());
    let output = match st.mode {
        Mode::Encode => encode(&input, st.scope, st.strict),
        Mode::Decode => decode(&input, st.scope),
    };
    ui.add_space(8.0);
    form::section(ui, theme, &s.t("dt-output"), |ui| {
        util::output(ui, s, theme, &s.t("dt-url"), &output, 5);
        // Encoding then decoding has to give the input back; when it does
        // not, the input already held escapes that are not valid.
        if st.mode == Mode::Decode
            && !input.trim().is_empty()
            && !output.contains('\u{FFFD}')
            && encode(&output, st.scope, false) != input.trim()
            && input.contains('%')
        {
            form::hint(ui, theme, &s.t("dt-url-partial-hint"));
        }
    });
    // The parts of whichever side of the conversion is a URL.
    let url_side = if st.mode == Mode::Decode {
        &output
    } else {
        &input
    };
    let rows = parts(url_side);
    if !rows.is_empty() {
        ui.add_space(8.0);
        form::section(ui, theme, &s.t("dt-url-parts"), |ui| {
            egui::Grid::new("dt-url-parts")
                .num_columns(2)
                .spacing([12.0, 4.0])
                .striped(true)
                .show(ui, |ui| {
                    for (k, v) in &rows {
                        ui.label(egui::RichText::new(k).strong().color(theme.text_muted));
                        ui.horizontal(|ui| {
                            ui.add(
                                egui::Label::new(egui::RichText::new(v).monospace())
                                    .wrap()
                                    .selectable(true),
                            );
                        });
                        ui.end_row();
                    }
                });
            form::hint(ui, theme, &s.t("dt-url-parts-hint"));
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_component_escapes_everything_but_the_unreserved_set() {
        assert_eq!(
            encode("a b&c=d/e?f", Scope::Component, false),
            "a%20b%26c%3Dd%2Fe%3Ff"
        );
        assert_eq!(encode("keep-._~", Scope::Component, false), "keep-._~");
        assert_eq!(
            encode("keep-._~", Scope::Component, true),
            "keep%2D%2E%5F%7E"
        );
        // Non-ASCII goes out as its UTF-8 bytes.
        assert_eq!(encode("café", Scope::Component, false), "caf%C3%A9");
        assert_eq!(
            encode("日本", Scope::Component, false),
            "%E6%97%A5%E6%9C%AC"
        );
    }

    #[test]
    fn a_full_url_keeps_its_delimiters() {
        assert_eq!(
            encode(
                "https://x.test/a b/c?q=1&r=two three#frag",
                Scope::Full,
                false
            ),
            "https://x.test/a%20b/c?q=1&r=two%20three#frag"
        );
        // An escape that is already there is not escaped again.
        assert_eq!(
            encode("https://x.test/a%20b", Scope::Full, false),
            "https://x.test/a%20b"
        );
    }

    #[test]
    fn form_scope_uses_plus_for_spaces_both_ways() {
        assert_eq!(encode("a b+c", Scope::Form, false), "a+b%2Bc");
        assert_eq!(decode("a+b%2Bc", Scope::Form), "a b+c");
        // Outside a form, `+` is a literal.
        assert_eq!(decode("a+b", Scope::Component), "a+b");
    }

    #[test]
    fn decoding_round_trips_and_leaves_bad_escapes_alone() {
        for text in ["a b&c=d", "café", "日本語 テスト", "100% sure", "a/b?c#d"] {
            let e = encode(text, Scope::Component, false);
            assert_eq!(decode(&e, Scope::Component), text, "round trip of {text}");
        }
        assert_eq!(decode("%zz", Scope::Component), "%zz");
        assert_eq!(decode("%2", Scope::Component), "%2");
        assert_eq!(decode("", Scope::Component), "");
    }

    #[test]
    fn a_url_splits_into_readable_parts() {
        let rows = parts("https://api.test/v1/users?q=john+doe&city=K%C3%B6ln#top");
        assert_eq!(rows[0], ("Scheme".into(), "https".into()));
        assert_eq!(rows[1], ("Host".into(), "api.test".into()));
        assert_eq!(rows[2], ("Path".into(), "/v1/users".into()));
        assert_eq!(rows[3], ("q".into(), "john doe".into()));
        assert_eq!(rows[4], ("city".into(), "Köln".into()));
        assert_eq!(rows[5], ("Fragment".into(), "top".into()));
        assert!(parts("not a url").is_empty());
        assert!(parts("mailto:x@y.test").is_empty(), "no authority");
    }
}
