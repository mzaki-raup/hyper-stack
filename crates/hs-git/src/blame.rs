//! `git blame --porcelain`, parsed into one record per line
//! (docs/tools/git-activity.md §5.3, Blame).
//!
//! The porcelain format prints a header for every line - `<sha> <orig>
//! <final> [<count>]` - followed by the commit's attributes the first
//! time that commit appears, then the line's text after a tab. A later
//! line of the same commit repeats only the header, so the attributes
//! are remembered per sha.

use std::collections::HashMap;

/// One line of the file and the commit that last touched it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlameLine {
    pub id: String,
    pub author: String,
    /// Unix seconds (author time).
    pub time: i64,
    pub summary: String,
    /// Line number in the blamed file, 1-based.
    pub line: u32,
    pub text: String,
}

#[derive(Default, Clone)]
struct Attrs {
    author: String,
    time: i64,
    summary: String,
}

pub fn parse(text: &str) -> Vec<BlameLine> {
    let mut out = Vec::new();
    let mut attrs: HashMap<String, Attrs> = HashMap::new();
    let mut lines = text.lines();
    while let Some(header) = lines.next() {
        let mut parts = header.split(' ');
        let Some(id) = parts.next().filter(|s| s.len() == 40) else {
            continue;
        };
        let line: u32 = parts.nth(1).and_then(|n| n.parse().ok()).unwrap_or(0);
        let mut cur = attrs.get(id).cloned().unwrap_or_default();
        let mut text = String::new();
        for l in lines.by_ref() {
            if let Some(t) = l.strip_prefix('\t') {
                text = t.to_string();
                break;
            }
            if let Some(v) = l.strip_prefix("author ") {
                cur.author = v.to_string();
            } else if let Some(v) = l.strip_prefix("author-time ") {
                cur.time = v.trim().parse().unwrap_or(0);
            } else if let Some(v) = l.strip_prefix("summary ") {
                cur.summary = v.to_string();
            }
        }
        attrs.insert(id.to_string(), cur.clone());
        out.push(BlameLine {
            id: id.to_string(),
            author: cur.author,
            time: cur.time,
            summary: cur.summary,
            line,
            text,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa 1 1 2
author Ann
author-mail <ann@x>
author-time 1700000000
author-tz +0000
committer Ann
committer-mail <ann@x>
committer-time 1700000000
committer-tz +0000
summary first
filename f.rs
\tfn main() {
aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa 2 2
\t}
bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb 1 3 1
author Bob
author-mail <bob@x>
author-time 1700000500
author-tz +0000
committer Bob
committer-mail <bob@x>
committer-time 1700000500
committer-tz +0000
summary second
previous aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa f.rs
filename f.rs
\t// end
";

    #[test]
    fn porcelain_lines_carry_their_commit_even_when_repeated() {
        let b = parse(SAMPLE);
        assert_eq!(b.len(), 3);
        assert_eq!(b[0].author, "Ann");
        assert_eq!(b[0].line, 1);
        assert_eq!(b[0].text, "fn main() {");
        assert_eq!(
            b[1].author, "Ann",
            "the second line repeats only the header"
        );
        assert_eq!(b[1].line, 2);
        assert_eq!(b[1].summary, "first");
        assert_eq!(b[2].author, "Bob");
        assert_eq!(b[2].time, 1700000500);
        assert_eq!(b[2].text, "// end");
    }
}
