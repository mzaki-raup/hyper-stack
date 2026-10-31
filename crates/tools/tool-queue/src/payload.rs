//! A message body as the Messages pane shows it (docs/tools/queue.md §4):
//! JSON pretty-printed, text as text, anything else as a hex dump with
//! its length. The same `hs_core` sniffing and formatting the database
//! cell viewer uses, so a payload reads the way a cell does.

/// How many 16-byte rows of a binary payload are shown.
const HEX_ROWS: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Body {
    Json(String),
    Text(String),
    /// What it looks like ("PNG image"), its length, and a hex dump of
    /// the start.
    Binary {
        label: String,
        len: usize,
        hex: String,
    },
    Empty,
}

pub fn render(bytes: &[u8]) -> Body {
    if bytes.is_empty() {
        return Body::Empty;
    }
    let s = hs_core::sniff::sniff(bytes);
    if s.kind == hs_core::sniff::Kind::Text
        && let Ok(text) = std::str::from_utf8(bytes)
    {
        return match hs_core::fmt::pretty_json(text) {
            Some(pretty) => Body::Json(pretty),
            None => Body::Text(text.to_string()),
        };
    }
    Body::Binary {
        label: s.label.to_string(),
        len: bytes.len(),
        hex: hs_core::sniff::hex_dump(bytes, HEX_ROWS),
    }
}

/// One line for a table cell: the start of the text, or what the bytes
/// are.
pub fn preview(bytes: &[u8], max_chars: usize) -> String {
    match render(bytes) {
        Body::Empty => String::new(),
        Body::Json(_) | Body::Text(_) => {
            let text = String::from_utf8_lossy(bytes);
            let one_line: String = text
                .chars()
                .map(|c| if c.is_control() { ' ' } else { c })
                .take(max_chars)
                .collect();
            if text.chars().count() > max_chars {
                format!("{one_line}…")
            } else {
                one_line
            }
        }
        Body::Binary { label, len, .. } => format!("{label}, {len} bytes"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_is_pretty_printed_and_text_kept() {
        assert_eq!(render(br#"{"a":1}"#), Body::Json("{\n  \"a\": 1\n}".into()));
        assert_eq!(render(b"hello"), Body::Text("hello".into()));
        assert_eq!(render(b""), Body::Empty);
    }

    #[test]
    fn binary_shows_what_it_is_and_how_long() {
        let Body::Binary { len, hex, .. } = render(&[0, 1, 2, 0xff]) else {
            panic!("not binary");
        };
        assert_eq!(len, 4);
        assert!(hex.contains("ff"));
    }

    #[test]
    fn a_preview_is_one_bounded_line() {
        assert_eq!(preview(b"a\nb", 10), "a b");
        assert_eq!(preview(b"abcdef", 3), "abc…");
        assert!(preview(&[0, 0xff], 10).contains("2 bytes"));
    }
}
