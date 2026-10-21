//! Text diff: two texts side by side, the lines that differ marked, and
//! the result as a unified diff to copy — for the "why does staging differ
//! from production" moment, with configuration pasted from both.
//!
//! The comparison is `similar`'s line diff, with a whitespace-insensitive
//! mode for files that only differ in indentation or line endings.

use serde::{Deserialize, Serialize};
use similar::{ChangeTag, TextDiff};

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default)]
pub struct State {
    pub left: String,
    pub right: String,
    pub left_name: String,
    pub right_name: String,
    pub ignore_whitespace: bool,
    /// The last diff, keyed by a hash of its inputs: the Myers pass ran
    /// on every frame before, O(n·m) while the user typed.
    #[serde(skip)]
    cache: Option<(u64, std::sync::Arc<Vec<Line>>)>,
}

/// Inputs larger than this are not diffed live; the pass would freeze the
/// window for seconds.
const MAX_LIVE_BYTES: usize = 2 * 1024 * 1024;

fn input_key(st: &State) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    st.left.hash(&mut h);
    st.right.hash(&mut h);
    st.ignore_whitespace.hash(&mut h);
    h.finish()
}

/// One line of the result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub tag: Tag,
    pub old: Option<usize>,
    pub new: Option<usize>,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tag {
    Equal,
    Delete,
    Insert,
}

/// The changed lines (and their context) in order.
pub fn lines(left: &str, right: &str, ignore_whitespace: bool) -> Vec<Line> {
    let (l, r) = if ignore_whitespace {
        (normalise(left), normalise(right))
    } else {
        (left.to_string(), right.to_string())
    };
    let diff = TextDiff::from_lines(&l, &r);
    let mut out = Vec::new();
    for change in diff.iter_all_changes() {
        let tag = match change.tag() {
            ChangeTag::Equal => Tag::Equal,
            ChangeTag::Delete => Tag::Delete,
            ChangeTag::Insert => Tag::Insert,
        };
        out.push(Line {
            tag,
            old: change.old_index().map(|i| i + 1),
            new: change.new_index().map(|i| i + 1),
            text: change.value().trim_end_matches(['\n', '\r']).to_string(),
        });
    }
    out
}

/// Every line trimmed and blank lines dropped, so indentation and line
/// endings stop counting.
fn normalise(text: &str) -> String {
    let mut out = String::new();
    for l in text.lines() {
        let t = l.trim();
        if !t.is_empty() {
            out.push_str(t);
            out.push('\n');
        }
    }
    out
}

/// How many lines were deleted and inserted.
pub fn counts(lines: &[Line]) -> (usize, usize) {
    let removed = lines.iter().filter(|l| l.tag == Tag::Delete).count();
    let added = lines.iter().filter(|l| l.tag == Tag::Insert).count();
    (added, removed)
}

/// A unified diff of the two texts, for the clipboard.
pub fn unified(left: &str, right: &str, left_name: &str, right_name: &str) -> String {
    let a = if left_name.is_empty() {
        "left"
    } else {
        left_name
    };
    let b = if right_name.is_empty() {
        "right"
    } else {
        right_name
    };
    TextDiff::from_lines(left, right)
        .unified_diff()
        .context_radius(3)
        .header(a, b)
        .to_string()
}

pub fn ui(st: &mut State, ui: &mut egui::Ui, s: &hs_core::Services, theme: &hs_ui::theme::Theme) {
    use hs_ui::form;
    form::page_title(ui, theme, &s.t("dt-diff"), Some(&s.t("dt-diff-sub")));
    ui.add_space(4.0);
    let half = ((ui.available_width() - 24.0) / 2.0).max(200.0);
    ui.horizontal_top(|ui| {
        for (text, name, hint) in [
            (&mut st.left, &mut st.left_name, "dt-diff-left"),
            (&mut st.right, &mut st.right_name, "dt-diff-right"),
        ] {
            ui.vertical(|ui| {
                ui.set_width(half);
                ui.add(
                    egui::TextEdit::singleline(name)
                        .hint_text(s.t(hint))
                        .desired_width(f32::INFINITY),
                );
                ui.add(
                    egui::TextEdit::multiline(text)
                        .font(egui::TextStyle::Monospace)
                        .desired_rows(10)
                        .desired_width(f32::INFINITY),
                );
            });
        }
    });
    ui.horizontal(|ui| {
        ui.checkbox(&mut st.ignore_whitespace, s.t("dt-diff-ignore-ws"));
        if ui.button(s.t("dt-diff-swap")).clicked() {
            std::mem::swap(&mut st.left, &mut st.right);
            std::mem::swap(&mut st.left_name, &mut st.right_name);
        }
        if ui.button(s.t("dt-diff-copy-unified")).clicked() {
            ui.ctx()
                .copy_text(unified(&st.left, &st.right, &st.left_name, &st.right_name));
        }
    });
    if st.left.is_empty() && st.right.is_empty() {
        form::hint(ui, theme, &s.t("dt-diff-hint"));
        return;
    }
    if st.left.len() + st.right.len() > MAX_LIVE_BYTES {
        form::hint(ui, theme, &s.t("dt-diff-too-large"));
        return;
    }
    let key = input_key(st);
    let lines = match &st.cache {
        Some((k, cached)) if *k == key => cached.clone(),
        _ => {
            let fresh = std::sync::Arc::new(lines(&st.left, &st.right, st.ignore_whitespace));
            st.cache = Some((key, fresh.clone()));
            fresh
        }
    };
    let (added, removed) = counts(&lines);
    let title = if added == 0 && removed == 0 {
        s.t("dt-diff-same")
    } else {
        s.i18n.t_args(
            "dt-diff-counts",
            &[
                ("added", &added.to_string()),
                ("removed", &removed.to_string()),
            ],
        )
    };
    form::section(ui, theme, &title, |ui| {
        let font = egui::FontId::monospace(12.5);
        let row_h = 18.0;
        egui::ScrollArea::both()
            .id_salt("dt-diff-rows")
            .max_height(360.0)
            .show_rows(ui, row_h, lines.len(), |ui, range| {
                ui.set_min_width(ui.available_width());
                for i in range {
                    let l = &lines[i];
                    let (rect, _) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width().max(600.0), row_h),
                        egui::Sense::hover(),
                    );
                    let p = ui.painter();
                    let (bg, sign, colour) = match l.tag {
                        Tag::Equal => (None, " ", theme.text),
                        Tag::Delete => (Some(theme.error.linear_multiply(0.16)), "-", theme.error),
                        Tag::Insert => (
                            Some(theme.success.linear_multiply(0.16)),
                            "+",
                            theme.success,
                        ),
                    };
                    if let Some(c) = bg {
                        p.rect_filled(rect, egui::CornerRadius::ZERO, c);
                    }
                    let y = rect.center().y;
                    for (no, x) in [(l.old, rect.left() + 36.0), (l.new, rect.left() + 76.0)] {
                        if let Some(no) = no {
                            p.text(
                                egui::pos2(x, y),
                                egui::Align2::RIGHT_CENTER,
                                no.to_string(),
                                font.clone(),
                                theme.text_muted,
                            );
                        }
                    }
                    p.text(
                        egui::pos2(rect.left() + 88.0, y),
                        egui::Align2::LEFT_CENTER,
                        sign,
                        font.clone(),
                        colour,
                    );
                    p.text(
                        egui::pos2(rect.left() + 100.0, y),
                        egui::Align2::LEFT_CENTER,
                        &l.text,
                        font.clone(),
                        theme.text,
                    );
                }
            });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changed_lines_are_marked_and_numbered() {
        let ls = lines("a\nb\nc\n", "a\nB\nc\nd\n", false);
        let tags: Vec<Tag> = ls.iter().map(|l| l.tag).collect();
        assert_eq!(
            tags,
            [
                Tag::Equal,
                Tag::Delete,
                Tag::Insert,
                Tag::Equal,
                Tag::Insert
            ]
        );
        assert_eq!(ls[1].old, Some(2));
        assert_eq!(ls[2].new, Some(2));
        assert_eq!(counts(&ls), (2, 1));
    }

    #[test]
    fn whitespace_can_stop_counting() {
        let a = "  key: 1\r\n\r\nvalue\n";
        let b = "key: 1\nvalue\n";
        assert!(counts(&lines(a, b, false)).0 > 0);
        assert_eq!(counts(&lines(a, b, true)), (0, 0));
    }

    #[test]
    fn the_unified_diff_names_both_sides() {
        let u = unified("x\n", "y\n", "staging", "prod");
        assert!(u.starts_with("--- staging\n+++ prod\n"), "{u}");
        assert!(u.contains("-x\n+y\n"));
    }
}
