//! A run's log laid out so its steps can be told apart (FEAT-0095). The
//! servers write one flat stream with their own step markers in it -
//! GitHub and Forgejo `##[group]…##[endgroup]`, Jenkins `[Pipeline] { (Stage)`,
//! this crate's own `===== job =====` banners between a pipeline's jobs,
//! plus the `[command]` / `+ cmd` echoes - and this turns those into
//! headers, groups, commands and indented body lines, with a blank line
//! before each step. Pure text in, lines out; the pane draws them.

/// What a line is, which decides how it is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A job banner: the biggest break in the log.
    Header,
    /// A step or stage: a group opened by the server.
    Group,
    /// A command the step ran.
    Command,
    /// The step's own output.
    Plain,
    /// The server's bookkeeping (`[Pipeline] sh`), kept but quiet.
    Meta,
    /// Air before a header or group.
    Blank,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub kind: Kind,
    /// How many groups deep the line sits; body lines indent by it.
    pub depth: usize,
    pub text: String,
}

/// Lay the log out. Every input line becomes at most one output line, so
/// nothing is lost; `Blank` lines are added, group closers are dropped.
pub fn lines(text: &str) -> Vec<Line> {
    let mut out: Vec<Line> = Vec::with_capacity(text.lines().count() + 16);
    let mut depth = 0usize;
    let blank_before = |out: &mut Vec<Line>| {
        if out.last().is_some_and(|l| l.kind != Kind::Blank) {
            out.push(Line {
                kind: Kind::Blank,
                depth: 0,
                text: String::new(),
            });
        }
    };
    for raw in text.lines() {
        let line = strip_timestamp(raw.trim_end_matches('\r'));
        if let Some(name) = line
            .strip_prefix("===== ")
            .and_then(|r| r.strip_suffix(" ====="))
        {
            depth = 0;
            blank_before(&mut out);
            out.push(Line {
                kind: Kind::Header,
                depth: 0,
                text: name.to_string(),
            });
            continue;
        }
        if let Some(name) = group_open(line) {
            blank_before(&mut out);
            out.push(Line {
                kind: Kind::Group,
                depth,
                text: name.to_string(),
            });
            depth += 1;
            continue;
        }
        if group_close(line) {
            depth = depth.saturating_sub(1);
            continue;
        }
        if let Some(cmd) = line
            .strip_prefix("[command]")
            .or_else(|| line.strip_prefix("+ "))
            .or_else(|| line.strip_prefix("$ "))
        {
            out.push(Line {
                kind: Kind::Command,
                depth,
                text: cmd.trim_start().to_string(),
            });
            continue;
        }
        let kind = if line.starts_with("[Pipeline]") || line.starts_with("##[") {
            Kind::Meta
        } else {
            Kind::Plain
        };
        out.push(Line {
            kind,
            depth,
            text: line.to_string(),
        });
    }
    out
}

/// The name a line opens a group with, if it does.
fn group_open(line: &str) -> Option<&str> {
    if let Some(n) = line
        .strip_prefix("##[group]")
        .or_else(|| line.strip_prefix("::group::"))
        .or_else(|| line.strip_prefix("##[section]"))
    {
        return Some(n.trim());
    }
    // Jenkins: `[Pipeline] { (Build)`; a brace with no stage name is a
    // block that is not a step of its own.
    line.strip_prefix("[Pipeline] { (")
        .and_then(|r| r.strip_suffix(')'))
        .map(str::trim)
}

fn group_close(line: &str) -> bool {
    matches!(line, "##[endgroup]" | "::endgroup::" | "[Pipeline] }")
}

/// GitHub's job logs start every line with `2026-11-05T10:00:00.1234567Z `;
/// the time is not what anyone reads the log for.
fn strip_timestamp(line: &str) -> &str {
    let b = line.as_bytes();
    if b.len() > 20
        && b[4] == b'-'
        && b[7] == b'-'
        && b[10] == b'T'
        && b[..4].iter().all(u8::is_ascii_digit)
        && let Some(i) = line.find(' ')
        && line[..i].ends_with('Z')
    {
        return &line[i + 1..];
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(text: &str) -> Vec<(Kind, usize, String)> {
        lines(text)
            .into_iter()
            .map(|l| (l.kind, l.depth, l.text))
            .collect()
    }

    #[test]
    fn github_groups_become_steps_with_air_between_and_indented_bodies() {
        let log = "===== build =====\n\
                   2026-11-05T10:00:00.1234567Z ##[group]Run cargo test\n\
                   2026-11-05T10:00:00.1234567Z [command]cargo test\n\
                   2026-11-05T10:00:01.0000000Z running 3 tests\n\
                   2026-11-05T10:00:01.0000000Z ##[endgroup]\n\
                   ##[group]Post job\ndone\n##[endgroup]\n";
        assert_eq!(
            kinds(log),
            vec![
                (Kind::Header, 0, "build".into()),
                (Kind::Blank, 0, String::new()),
                (Kind::Group, 0, "Run cargo test".into()),
                (Kind::Command, 1, "cargo test".into()),
                (Kind::Plain, 1, "running 3 tests".into()),
                (Kind::Blank, 0, String::new()),
                (Kind::Group, 0, "Post job".into()),
                (Kind::Plain, 1, "done".into()),
            ]
        );
    }

    #[test]
    fn jenkins_stages_nest_and_pipeline_noise_stays_quiet() {
        let log = "Started by user dev\n[Pipeline] {\n[Pipeline] { (Build)\n[Pipeline] sh\n+ make\nok\n[Pipeline] }\n[Pipeline] }\nFinished: SUCCESS\n";
        assert_eq!(
            kinds(log),
            vec![
                (Kind::Plain, 0, "Started by user dev".into()),
                (Kind::Meta, 0, "[Pipeline] {".into()),
                (Kind::Blank, 0, String::new()),
                (Kind::Group, 0, "Build".into()),
                (Kind::Meta, 1, "[Pipeline] sh".into()),
                (Kind::Command, 1, "make".into()),
                (Kind::Plain, 1, "ok".into()),
                (Kind::Plain, 0, "Finished: SUCCESS".into()),
            ]
        );
    }

    #[test]
    fn a_log_with_no_markers_is_left_as_it_was() {
        let log = "line one\r\nline two\n";
        assert_eq!(
            kinds(log),
            vec![
                (Kind::Plain, 0, "line one".into()),
                (Kind::Plain, 0, "line two".into()),
            ]
        );
        assert_eq!(
            strip_timestamp("2026-11-05 not a stamp"),
            "2026-11-05 not a stamp"
        );
    }
}
