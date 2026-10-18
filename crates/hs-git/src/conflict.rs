//! Line-level conflict parsing and resolution (docs/tools/git.md §9).
//!
//! A conflicted file is split into context and conflict blocks, the shape Git
//! writes: `<<<<<<< ours`, optionally `||||||| base` (diff3), `=======`,
//! `>>>>>>> theirs`. The editor chooses a result per block and [`resolve`]
//! writes the file without markers. Nothing here touches the index; the
//! caller stages the resolved file.
//!
//! A block with no closing marker is kept as context rather than lost: a
//! file that merely contains a line starting with `<<<<<<<` must survive a
//! save unchanged.

/// One conflict block, with the marker labels (`HEAD`, a branch name, …).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    pub ours: String,
    pub theirs: String,
    /// The `|||||||` section of a diff3 conflict, when present.
    pub base: Option<String>,
    pub ours_label: String,
    pub theirs_label: String,
}

/// A piece of the file: text shared by both sides, or a conflict.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Segment {
    Context(String),
    Conflict(Conflict),
}

/// A parsed conflicted file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parsed {
    pub segments: Vec<Segment>,
}

impl Parsed {
    /// How many conflict blocks the file has.
    pub fn conflicts(&self) -> usize {
        self.segments
            .iter()
            .filter(|s| matches!(s, Segment::Conflict(_)))
            .count()
    }
}

/// What the user chose for one block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Choice {
    /// Nothing chosen yet; saving is refused.
    Unresolved,
    Ours,
    Theirs,
    /// Ours followed by Theirs.
    Both,
    /// Text typed in the editor, written as it is.
    Custom(String),
}

/// Why a file could not be resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolveError {
    /// This block has no choice yet.
    Unresolved(usize),
}

fn bare(line: &str) -> &str {
    line.trim_end_matches(['\r', '\n'])
}

fn flush(segments: &mut Vec<Segment>, context: &mut String) {
    if !context.is_empty() {
        segments.push(Segment::Context(std::mem::take(context)));
    }
}

/// Split `text` into context and conflict blocks.
pub fn parse(text: &str) -> Parsed {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let mut segments = Vec::new();
    let mut context = String::new();
    let mut i = 0;
    while i < lines.len() {
        if bare(lines[i]).starts_with("<<<<<<<")
            && let Some((conflict, next)) = parse_block(&lines, i)
        {
            flush(&mut segments, &mut context);
            segments.push(Segment::Conflict(conflict));
            i = next;
            continue;
        }
        context.push_str(lines[i]);
        i += 1;
    }
    flush(&mut segments, &mut context);
    Parsed { segments }
}

/// Read one conflict starting at `start`; `None` when the block is not
/// closed, which leaves its lines to be kept as context.
fn parse_block(lines: &[&str], start: usize) -> Option<(Conflict, usize)> {
    let ours_label = bare(lines[start])
        .trim_start_matches("<<<<<<<")
        .trim()
        .to_string();
    let mut ours = String::new();
    let mut base: Option<String> = None;
    let mut theirs = String::new();
    let mut in_base = false;
    let mut at_separator = false;
    let mut i = start + 1;
    while i < lines.len() {
        let line = bare(lines[i]);
        if !at_separator && line.starts_with("|||||||") {
            in_base = true;
            base = Some(String::new());
            i += 1;
            continue;
        }
        if !at_separator && line.starts_with("=======") {
            at_separator = true;
            in_base = false;
            i += 1;
            continue;
        }
        if at_separator && line.starts_with(">>>>>>>") {
            let theirs_label = line.trim_start_matches(">>>>>>>").trim().to_string();
            return Some((
                Conflict {
                    ours,
                    theirs,
                    base,
                    ours_label,
                    theirs_label,
                },
                i + 1,
            ));
        }
        if at_separator {
            theirs.push_str(lines[i]);
        } else if in_base {
            if let Some(b) = &mut base {
                b.push_str(lines[i]);
            }
        } else {
            ours.push_str(lines[i]);
        }
        i += 1;
    }
    None
}

/// Write the file with one choice applied to every conflict.
pub fn resolve(parsed: &Parsed, choices: &[Choice]) -> Result<String, ResolveError> {
    let mut out = String::new();
    let mut index = 0;
    for segment in &parsed.segments {
        match segment {
            Segment::Context(text) => out.push_str(text),
            Segment::Conflict(conflict) => {
                match choices.get(index).unwrap_or(&Choice::Unresolved) {
                    Choice::Unresolved => return Err(ResolveError::Unresolved(index)),
                    Choice::Ours => out.push_str(&conflict.ours),
                    Choice::Theirs => out.push_str(&conflict.theirs),
                    Choice::Both => {
                        out.push_str(&conflict.ours);
                        out.push_str(&conflict.theirs);
                    }
                    Choice::Custom(text) => {
                        out.push_str(text);
                        // A typed result without a final newline must not run
                        // into the context line that follows.
                        if !text.is_empty() && !text.ends_with('\n') {
                            out.push('\n');
                        }
                    }
                }
                index += 1;
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TWO_WAY: &str =
        "line one\n<<<<<<< HEAD\nours a\nours b\n=======\ntheirs a\n>>>>>>> feature\nline two\n";

    #[test]
    fn splits_context_and_both_sides() {
        let parsed = parse(TWO_WAY);
        assert_eq!(parsed.conflicts(), 1);
        let Segment::Conflict(c) = &parsed.segments[1] else {
            panic!("expected a conflict: {:?}", parsed.segments);
        };
        assert_eq!(c.ours_label, "HEAD");
        assert_eq!(c.theirs_label, "feature");
        assert_eq!(c.ours, "ours a\nours b\n");
        assert_eq!(c.theirs, "theirs a\n");
        assert_eq!(c.base, None);
    }

    #[test]
    fn reads_a_diff3_base_section() {
        let text = "<<<<<<< ours\nside\n||||||| base\ncommon\n=======\ntheirs\n>>>>>>> theirs\n";
        let parsed = parse(text);
        let Segment::Conflict(c) = &parsed.segments[0] else {
            panic!("expected a conflict");
        };
        assert_eq!(c.base.as_deref(), Some("common\n"));
        assert_eq!(c.ours, "side\n");
        assert_eq!(c.theirs, "theirs\n");
    }

    #[test]
    fn a_block_without_an_end_stays_as_text() {
        let text = "keep\n<<<<<<< HEAD\nhalf a conflict\n";
        let parsed = parse(text);
        assert_eq!(parsed.conflicts(), 0);
        assert_eq!(
            parsed.segments,
            vec![Segment::Context(text.to_string())],
            "saving must not lose the file"
        );
    }

    #[test]
    fn resolves_every_choice() {
        let parsed = parse(TWO_WAY);
        assert_eq!(
            resolve(&parsed, &[Choice::Ours]).unwrap(),
            "line one\nours a\nours b\nline two\n"
        );
        assert_eq!(
            resolve(&parsed, &[Choice::Theirs]).unwrap(),
            "line one\ntheirs a\nline two\n"
        );
        assert_eq!(
            resolve(&parsed, &[Choice::Both]).unwrap(),
            "line one\nours a\nours b\ntheirs a\nline two\n"
        );
        assert_eq!(
            resolve(&parsed, &[Choice::Custom("typed".into())]).unwrap(),
            "line one\ntyped\nline two\n"
        );
        assert_eq!(
            resolve(&parsed, &[Choice::Unresolved]),
            Err(ResolveError::Unresolved(0))
        );
    }

    #[test]
    fn keeps_crlf_and_a_missing_final_newline() {
        let text = "one\r\n<<<<<<< HEAD\r\nours\r\n=======\r\ntheirs\r\n>>>>>>> x\r\ntwo";
        let parsed = parse(text);
        assert_eq!(
            resolve(&parsed, &[Choice::Ours]).unwrap(),
            "one\r\nours\r\ntwo"
        );
    }
}
