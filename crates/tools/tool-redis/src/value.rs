//! What a Redis string actually holds (docs/tools/redis.md §5): the server
//! stores bytes, so the viewer guesses between JSON, a number, text and
//! binary, and unwraps gzip before guessing again. Nothing is rewritten: the
//! guess only picks the viewer and the syntax highlighting.

/// How the value is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    Json,
    Number,
    Text,
    /// Not valid UTF-8: shown as a hex dump.
    Binary,
}

impl Shape {
    /// Language id for the editor's highlighting.
    pub fn language(self) -> &'static str {
        match self {
            Shape::Json => "json",
            _ => "text",
        }
    }
}

/// A value decoded for display.
#[derive(Debug, Clone, PartialEq)]
pub struct Decoded {
    pub shape: Shape,
    /// Text to show. For [`Shape::Binary`] this is the hex dump.
    pub text: String,
    /// The value was gzip-compressed and has been expanded for display.
    pub gzip: bool,
    /// Bytes of the value as stored.
    pub stored_len: usize,
}

/// Decide how to show a stored value.
pub fn decode(bytes: &[u8]) -> Decoded {
    let stored_len = bytes.len();
    let (body, gzip) = match gunzip(bytes) {
        Some(plain) => (plain, true),
        None => (bytes.to_vec(), false),
    };
    let Ok(text) = String::from_utf8(body) else {
        return Decoded {
            shape: Shape::Binary,
            text: hex_dump(bytes),
            gzip: false,
            stored_len,
        };
    };
    let trimmed = text.trim();
    let shape = if is_json(trimmed) {
        Shape::Json
    } else if !trimmed.is_empty() && trimmed.parse::<f64>().is_ok() {
        Shape::Number
    } else {
        Shape::Text
    };
    let text = match shape {
        // Pretty-print JSON without reordering its keys.
        Shape::Json => hs_core::fmt::pretty_json(&text).unwrap_or(text),
        _ => text,
    };
    Decoded {
        shape,
        text,
        gzip,
        stored_len,
    }
}

/// A value is JSON when it parses as an object or an array. A bare number or
/// a quoted word is valid JSON too, but showing it as JSON helps nobody.
fn is_json(s: &str) -> bool {
    let first = s.as_bytes().first();
    matches!(first, Some(b'{') | Some(b'[')) && serde_json::from_str::<serde_json::Value>(s).is_ok()
}

/// Expand a gzip member, giving up on anything that is not one.
fn gunzip(bytes: &[u8]) -> Option<Vec<u8>> {
    use std::io::Read;
    // 1f 8b 08 is gzip with deflate; anything else is the value itself.
    if bytes.len() < 18 || bytes[0] != 0x1f || bytes[1] != 0x8b || bytes[2] != 0x08 {
        return None;
    }
    let mut out = Vec::new();
    flate2::read::GzDecoder::new(bytes)
        // A corrupt member expands as far as it can; the viewer wants none of it.
        .take(MAX_EXPANDED)
        .read_to_end(&mut out)
        .ok()?;
    Some(out)
}

/// A compressed value will not be expanded past this, so a zip bomb in a key
/// cannot take the UI down.
const MAX_EXPANDED: u64 = 64 * 1024 * 1024;

/// `offset  hex bytes  |ascii|`, the way a hex editor shows bytes.
pub fn hex_dump(bytes: &[u8]) -> String {
    let mut out = String::new();
    for (row, chunk) in bytes.chunks(16).enumerate() {
        out.push_str(&format!("{:08x}  ", row * 16));
        for (i, b) in chunk.iter().enumerate() {
            out.push_str(&format!("{b:02x} "));
            if i == 7 {
                out.push(' ');
            }
        }
        for i in chunk.len()..16 {
            out.push_str("   ");
            if i == 7 {
                out.push(' ');
            }
        }
        out.push_str(" |");
        for b in chunk {
            out.push(match b {
                0x20..=0x7e => *b as char,
                _ => '.',
            });
        }
        out.push_str("|\n");
    }
    out
}

/// "00:14:52" from milliseconds, or "—" for a key that never expires.
pub fn format_ttl(ms: Option<i64>) -> String {
    let Some(ms) = ms else {
        return "—".into();
    };
    let secs = (ms / 1000).max(0);
    let (d, h, m, s) = (
        secs / 86_400,
        (secs % 86_400) / 3600,
        (secs % 3600) / 60,
        secs % 60,
    );
    if d > 0 {
        format!("{d}d {h:02}:{m:02}:{s:02}")
    } else {
        format!("{h:02}:{m:02}:{s:02}")
    }
}

/// "1.4 kB", "2.1 MB": sizes a human reads rather than byte counts.
pub fn format_bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "kB", "MB", "GB", "TB"];
    let mut v = n as f64;
    let mut unit = 0;
    while v >= 1024.0 && unit + 1 < UNITS.len() {
        v /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{n} B")
    } else {
        format!("{v:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_is_recognised_and_laid_out_without_reordering() {
        let d = decode(br#"{"b":1,"a":2}"#);
        assert_eq!(d.shape, Shape::Json);
        // Keys keep the order the value had.
        assert!(d.text.find("\"b\"").unwrap() < d.text.find("\"a\"").unwrap());
        assert!(d.text.contains('\n'), "it should be laid out");
        assert_eq!(d.shape.language(), "json");
        // A bare scalar is not shown as JSON.
        assert_eq!(decode(b"42").shape, Shape::Number);
        assert_eq!(decode(b"-1.5e3").shape, Shape::Number);
        assert_eq!(decode(b"hello").shape, Shape::Text);
        assert_eq!(decode(b"").shape, Shape::Text);
        // Broken JSON stays text rather than being repaired.
        assert_eq!(decode(b"{oops").shape, Shape::Text);
    }

    #[test]
    fn bytes_that_are_not_text_become_a_hex_dump() {
        let d = decode(&[0xff, 0xfe, 0x00]);
        assert_eq!(d.shape, Shape::Binary);
        assert!(d.text.starts_with("00000000  ff fe 00"));
        assert!(d.text.contains("|...|"));
        assert_eq!(d.stored_len, 3);
    }

    #[test]
    fn a_gzip_value_is_expanded_before_it_is_read() {
        // gzip of `{"a":1}`, produced by Python's gzip module.
        let gz: [u8; 27] = [
            0x1f, 0x8b, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02, 0xff, 0xab, 0x56, 0x4a, 0x54,
            0xb2, 0x32, 0xac, 0x05, 0x00, 0xaf, 0xac, 0x1b, 0x56, 0x07, 0x00, 0x00, 0x00,
        ];
        let d = decode(&gz);
        assert!(d.gzip, "it should notice the gzip wrapper");
        assert_eq!(d.shape, Shape::Json);
        assert!(d.text.contains("\"a\""));
        assert_eq!(d.stored_len, 27);
        // Something that only starts like gzip is left alone.
        assert!(!decode(&[0x1f, 0x8b, 0x08, 0x00]).gzip);
    }

    #[test]
    fn a_ttl_counts_down_in_readable_units() {
        assert_eq!(format_ttl(None), "—");
        assert_eq!(format_ttl(Some(892_000)), "00:14:52");
        assert_eq!(format_ttl(Some(90_061_000)), "1d 01:01:01");
        assert_eq!(format_ttl(Some(0)), "00:00:00");
    }

    #[test]
    fn sizes_read_as_sizes() {
        assert_eq!(format_bytes(142), "142 B");
        assert_eq!(format_bytes(2048), "2.0 kB");
        assert_eq!(format_bytes(5 * 1024 * 1024), "5.0 MB");
    }
}
