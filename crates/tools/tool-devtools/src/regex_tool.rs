//! Regex tester: a pattern, some text, every match with its groups, and a
//! replacement preview — the loop a regex is always written in, without
//! leaving for a website that keeps what was pasted.
//!
//! The engine is the `regex` crate, which is what HyperStack's own filters
//! and highlight rules use, so a pattern that works here works there. Its
//! syntax is RE2-like: no backreferences, no look-around; the error text
//! from the crate says so when one is typed.

use regex::{Regex, RegexBuilder};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct State {
    pub pattern: String,
    pub text: String,
    pub replacement: String,
    pub case_insensitive: bool,
    pub multi_line: bool,
    pub dot_all: bool,
    pub ignore_whitespace: bool,
    /// Only the first match, as `find` rather than `find_iter`.
    pub first_only: bool,
}

impl Default for State {
    fn default() -> Self {
        Self {
            pattern: String::new(),
            text: String::new(),
            replacement: String::new(),
            case_insensitive: false,
            multi_line: true,
            dot_all: false,
            ignore_whitespace: false,
            first_only: false,
        }
    }
}

/// One match: where it is, what it is, and its groups by index and name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub start: usize,
    pub end: usize,
    pub text: String,
    /// `(name or index, text)` for every group that took part.
    pub groups: Vec<(String, String)>,
}

/// Compile the pattern with the chosen flags.
pub fn compile(st: &State) -> Result<Regex, String> {
    if st.pattern.is_empty() {
        return Err(String::new());
    }
    RegexBuilder::new(&st.pattern)
        .case_insensitive(st.case_insensitive)
        .multi_line(st.multi_line)
        .dot_matches_new_line(st.dot_all)
        .ignore_whitespace(st.ignore_whitespace)
        .size_limit(4 << 20)
        .build()
        .map_err(|e| e.to_string())
}

/// Every match of `re` in `text` (or the first), with groups.
pub fn matches(re: &Regex, text: &str, first_only: bool) -> Vec<Found> {
    let names: Vec<Option<&str>> = re.capture_names().collect();
    let mut out = Vec::new();
    for caps in re.captures_iter(text) {
        let m = caps.get(0).expect("group 0 always exists");
        let mut groups = Vec::new();
        for (i, name) in names.iter().enumerate().skip(1) {
            if let Some(g) = caps.get(i) {
                let label = match name {
                    Some(n) => format!("{i} ({n})"),
                    None => i.to_string(),
                };
                groups.push((label, g.as_str().to_string()));
            }
        }
        out.push(Found {
            start: m.start(),
            end: m.end(),
            text: m.as_str().to_string(),
            groups,
        });
        if first_only || out.len() >= 2000 {
            break;
        }
    }
    out
}

/// The text with every match replaced, `$1` / `$name` expanded as the
/// crate does it.
pub fn replace(re: &Regex, text: &str, replacement: &str, first_only: bool) -> String {
    if first_only {
        re.replace(text, replacement).into_owned()
    } else {
        re.replace_all(text, replacement).into_owned()
    }
}

pub fn ui(st: &mut State, ui: &mut egui::Ui, s: &hs_core::Services, theme: &hs_ui::theme::Theme) {
    use hs_ui::form;
    form::page_title(ui, theme, &s.t("dt-regex"), Some(&s.t("dt-regex-sub")));
    ui.add_space(4.0);
    form::section(ui, theme, &s.t("dt-regex-pattern"), |ui| {
        ui.add(
            egui::TextEdit::singleline(&mut st.pattern)
                .hint_text(s.t("dt-regex-pattern-hint"))
                .font(egui::TextStyle::Monospace)
                .desired_width(f32::INFINITY),
        );
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut st.case_insensitive, s.t("dt-regex-flag-i"));
            ui.checkbox(&mut st.multi_line, s.t("dt-regex-flag-m"));
            ui.checkbox(&mut st.dot_all, s.t("dt-regex-flag-s"));
            ui.checkbox(&mut st.ignore_whitespace, s.t("dt-regex-flag-x"));
            ui.checkbox(&mut st.first_only, s.t("dt-regex-first"));
        });
        form::hint(ui, theme, &s.t("dt-regex-hint"));
    });
    let compiled = compile(st);
    form::section(ui, theme, &s.t("dt-regex-text"), |ui| {
        ui.add(
            egui::TextEdit::multiline(&mut st.text)
                .hint_text(s.t("dt-regex-text-hint"))
                .font(egui::TextStyle::Monospace)
                .desired_rows(6)
                .desired_width(f32::INFINITY),
        );
    });
    match &compiled {
        Err(e) if e.is_empty() => {}
        Err(e) => {
            ui.label(egui::RichText::new(e).color(theme.error).monospace());
        }
        Ok(re) => {
            let found = matches(re, &st.text, st.first_only);
            let title = s
                .i18n
                .t_args("dt-regex-matches", &[("n", &found.len().to_string())]);
            form::section(ui, theme, &title, |ui| {
                if found.is_empty() {
                    form::hint(ui, theme, &s.t("dt-regex-none"));
                }
                // The text with the matches marked, so where they fall is
                // visible without counting characters.
                if !found.is_empty() {
                    let mut job = egui::text::LayoutJob::default();
                    let font = egui::FontId::monospace(12.5);
                    let mut at = 0;
                    for f in &found {
                        if f.start > at {
                            job.append(
                                &st.text[at..f.start],
                                0.0,
                                egui::TextFormat::simple(font.clone(), theme.text),
                            );
                        }
                        job.append(
                            &st.text[f.start..f.end],
                            0.0,
                            egui::TextFormat {
                                font_id: font.clone(),
                                color: theme.text,
                                background: theme.accent.linear_multiply(0.35),
                                ..Default::default()
                            },
                        );
                        at = f.end;
                    }
                    if at < st.text.len() {
                        job.append(
                            &st.text[at..],
                            0.0,
                            egui::TextFormat::simple(font, theme.text),
                        );
                    }
                    egui::ScrollArea::vertical()
                        .id_salt("dt-regex-marked")
                        .max_height(180.0)
                        .show(ui, |ui| {
                            ui.add(egui::Label::new(job).wrap());
                        });
                    ui.add_space(6.0);
                }
                egui::ScrollArea::vertical()
                    .id_salt("dt-regex-list")
                    .max_height(220.0)
                    .show(ui, |ui| {
                        for (i, f) in found.iter().enumerate() {
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new(format!("{}", i + 1))
                                        .small()
                                        .color(theme.text_muted),
                                );
                                ui.label(
                                    egui::RichText::new(format!("{}..{}", f.start, f.end))
                                        .small()
                                        .monospace()
                                        .color(theme.text_muted),
                                );
                                ui.label(egui::RichText::new(&f.text).monospace());
                                if ui
                                    .small_button(hs_ui::icons::ph::COPY)
                                    .on_hover_text(s.t("dt-copy"))
                                    .clicked()
                                {
                                    ui.ctx().copy_text(f.text.clone());
                                }
                            });
                            for (label, text) in &f.groups {
                                ui.horizontal(|ui| {
                                    ui.add_space(24.0);
                                    ui.label(
                                        egui::RichText::new(format!("${label}"))
                                            .small()
                                            .monospace()
                                            .color(theme.accent),
                                    );
                                    ui.label(egui::RichText::new(text).monospace());
                                });
                            }
                        }
                    });
            });
            form::section(ui, theme, &s.t("dt-regex-replace"), |ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut st.replacement)
                        .hint_text(s.t("dt-regex-replace-hint"))
                        .font(egui::TextStyle::Monospace)
                        .desired_width(f32::INFINITY),
                );
                if !st.replacement.is_empty() || !found.is_empty() {
                    let out = replace(re, &st.text, &st.replacement, st.first_only);
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(s.t("dt-regex-replace-result"))
                                .small()
                                .color(theme.text_muted),
                        );
                        if ui
                            .small_button(hs_ui::icons::ph::COPY)
                            .on_hover_text(s.t("dt-copy"))
                            .clicked()
                        {
                            ui.ctx().copy_text(out.clone());
                        }
                    });
                    let mut shown = out;
                    ui.add(
                        egui::TextEdit::multiline(&mut shown)
                            .font(egui::TextStyle::Monospace)
                            .desired_rows(4)
                            .desired_width(f32::INFINITY)
                            .interactive(false),
                    );
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_carry_their_groups_by_index_and_name() {
        let st = State {
            pattern: r"(?P<key>\w+)=(\d+)".into(),
            ..Default::default()
        };
        let re = compile(&st).unwrap();
        let found = matches(&re, "a=1 b=22 c=x", false);
        assert_eq!(found.len(), 2);
        assert_eq!(found[1].text, "b=22");
        assert_eq!(found[1].groups[0], ("1 (key)".to_string(), "b".to_string()));
        assert_eq!(found[1].groups[1], ("2".to_string(), "22".to_string()));
        assert_eq!(replace(&re, "a=1 b=22", "$key:$2", false), "a:1 b:22");
        assert_eq!(matches(&re, "a=1 b=22", true).len(), 1);
    }

    #[test]
    fn flags_apply_and_a_bad_pattern_reports() {
        let st = State {
            pattern: "^line".into(),
            multi_line: false,
            ..Default::default()
        };
        let re = compile(&st).unwrap();
        assert_eq!(matches(&re, "line\nline", false).len(), 1);
        let st = State {
            pattern: "LINE".into(),
            case_insensitive: true,
            ..Default::default()
        };
        assert_eq!(matches(&compile(&st).unwrap(), "line", false).len(), 1);
        let st = State {
            pattern: "(unclosed".into(),
            ..Default::default()
        };
        assert!(compile(&st).unwrap_err().contains("unclosed"));
        assert_eq!(compile(&State::default()).unwrap_err(), "");
    }
}
