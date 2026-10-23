//! The shape key (docs/tools/logs.md §15.5): a message with its varying
//! parts replaced by placeholders, so `user 4821 not found` and
//! `user 9 not found` share one shape.
//!
//! Three things need it and it is written once: collapsing consecutive
//! repeats in the list, deduplicating Problems entries (§15.2), and
//! counting per shape when two time windows are compared (§15.8).
//!
//! It is deliberately cheap and textual. Nothing here parses the message;
//! a shape is a grouping key, not a claim about meaning.

use std::sync::LazyLock;

use regex::Regex;

/// Patterns in the order they must run: the specific before the general,
/// or a UUID is eaten as a run of numbers and letters.
static RULES: LazyLock<Vec<(Regex, &'static str)>> = LazyLock::new(|| {
    let r = |p: &str, to: &'static str| (Regex::new(p).expect("shape pattern"), to);
    vec![
        // Quoted strings first: their contents must not be shaped.
        r(r#""[^"]*""#, "<str>"),
        r(r"'[^']*'", "<str>"),
        r(
            r"\b[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}\b",
            "<uuid>",
        ),
        // A ULID is 26 Crockford base32 characters, upper case in practice.
        r(r"\b[0-9A-HJKMNP-TV-Z]{26}\b", "<id>"),
        r(r"\b[0-9a-fA-F]{16,}\b", "<hex>"),
        r(r"\b\d{1,3}(?:\.\d{1,3}){3}\b", "<ip>"),
        // A path: a separator with something either side, Windows or POSIX.
        r(r"[A-Za-z]:\\[^\s,;:]+", "<path>"),
        r(r"(?:/[\w.\-]+){2,}", "<path>"),
        // Timestamps inside the message, left after the parser took the
        // leading one.
        r(
            r"\b\d{4}-\d{2}-\d{2}[T ]\d{2}:\d{2}:\d{2}(?:\.\d+)?Z?\b",
            "<ts>",
        ),
        r(r"\b\d{2}:\d{2}:\d{2}(?:\.\d+)?\b", "<time>"),
        // Anything numeric last, including hex literals and decimals.
        r(r"\b0x[0-9a-fA-F]+\b", "<n>"),
        r(r"\b\d+(?:\.\d+)?\b", "<n>"),
    ]
});

/// `message` with its varying parts replaced. Runs of whitespace collapse
/// so wrapping or alignment does not split a shape.
pub fn key(message: &str) -> String {
    let mut out = message.to_string();
    for (re, to) in RULES.iter() {
        out = re.replace_all(&out, *to).into_owned();
    }
    static WS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").expect("ws"));
    WS.replace_all(out.trim(), " ").into_owned()
}

/// One row of the list when repeats are collapsed: either a record, or a
/// run of records sharing a level and a shape.
#[derive(Debug, Clone, PartialEq)]
pub enum Row {
    /// An index into the filtered list.
    One(usize),
    /// `first` is the index into the filtered list of the newest record in
    /// the run (what the row shows); `count` is how many it stands for, and
    /// `span` the positions it covers, so expanding is a slice.
    Run {
        first: usize,
        count: usize,
        span: (usize, usize),
    },
}

/// Fold consecutive records that share a level and a shape key. `shape_of`
/// answers for a position in the filtered list; it returns `None` for a
/// record that cannot be shaped, which never folds.
///
/// Only *consecutive* runs fold: a log is read in order, and joining
/// records that are minutes apart would misrepresent it.
pub fn fold<F>(len: usize, min_run: usize, mut shape_of: F) -> Vec<Row>
where
    F: FnMut(usize) -> Option<String>,
{
    let mut rows = Vec::new();
    let mut i = 0;
    while i < len {
        let Some(k) = shape_of(i) else {
            rows.push(Row::One(i));
            i += 1;
            continue;
        };
        let mut j = i + 1;
        while j < len && shape_of(j).as_deref() == Some(k.as_str()) {
            j += 1;
        }
        let count = j - i;
        if count >= min_run {
            // The newest of the run is the one worth showing.
            rows.push(Row::Run {
                first: j - 1,
                count,
                span: (i, j - 1),
            });
        } else {
            for p in i..j {
                rows.push(Row::One(p));
            }
        }
        i = j;
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_that_differ_only_in_their_values_share_a_shape() {
        assert_eq!(key("user 4821 not found"), key("user 9 not found"));
        assert_eq!(
            key("GET /v1/orders/7 took 31.5 ms"),
            key("GET /v1/orders/12 took 4 ms")
        );
        assert_eq!(
            key("session 3f2504e0-4f89-11d3-9a0c-0305e82c3301 expired"),
            key("session 00000000-0000-0000-0000-000000000000 expired")
        );
        assert_eq!(
            key("connection from 10.0.0.4 refused"),
            key("connection from 192.168.1.200 refused")
        );
        assert_eq!(
            key(r#"wrote "orders.csv" to disk"#),
            key(r#"wrote "customers-2026.csv" to disk"#)
        );
    }

    #[test]
    fn messages_that_differ_in_substance_do_not() {
        assert_ne!(key("user 1 not found"), key("order 1 not found"));
        assert_ne!(key("payment timeout"), key("payment refused"));
    }

    #[test]
    fn whitespace_and_alignment_do_not_split_a_shape() {
        assert_eq!(key("a  b\tc"), key("a b c"));
        assert_eq!(key("  padded  "), key("padded"));
    }

    #[test]
    fn a_run_folds_and_a_short_one_does_not() {
        // Positions 0..3 share a shape, 4 is different, 5..7 share another.
        let shapes = ["a", "a", "a", "a", "b", "c", "c", "x"];
        let rows = fold(shapes.len(), 3, |i| Some(shapes[i].to_string()));
        assert_eq!(
            rows,
            vec![
                Row::Run {
                    first: 3,
                    count: 4,
                    span: (0, 3)
                },
                Row::One(4),
                // A run of two is under the minimum, so both rows stay.
                Row::One(5),
                Row::One(6),
                Row::One(7),
            ]
        );
    }

    #[test]
    fn a_record_without_a_shape_never_folds() {
        let rows = fold(3, 2, |_| None);
        assert_eq!(rows, vec![Row::One(0), Row::One(1), Row::One(2)]);
    }

    #[test]
    fn folding_nothing_is_nothing() {
        assert!(fold(0, 2, |_| Some("a".into())).is_empty());
    }
}
