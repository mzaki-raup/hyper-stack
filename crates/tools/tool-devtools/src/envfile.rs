//! The `.env` editor (docs/tools/devtools.md §2 Workspace helpers):
//! open a `.env`, see it as a table with secret-looking values masked,
//! edit it, and write it back keeping comments and blank lines where
//! they were.

use hs_core::Services;
use hs_ui::form;
use hs_ui::icons::ph;
use hs_ui::theme::Theme;

/// One line of the file: a pair, or something to keep as it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Line {
    Pair {
        key: String,
        value: String,
        /// `export FOO=bar` keeps its `export`.
        export: bool,
        /// The quote the value was written with, so it is written back
        /// the same way.
        quote: Option<char>,
    },
    /// A comment or a blank line, kept verbatim.
    Other(String),
}

/// Keys whose value is masked until it is revealed.
pub fn looks_secret(key: &str) -> bool {
    let k = key.to_ascii_uppercase();
    [
        "SECRET",
        "PASSWORD",
        "PASSWD",
        "TOKEN",
        "KEY",
        "CREDENTIAL",
        "PRIVATE",
        "SALT",
        "AUTH",
        "SESSION",
        "COOKIE",
        "DSN",
        "CONNECTION_STRING",
    ]
    .iter()
    .any(|needle| k.contains(needle))
}

/// Parse a `.env` into lines. Nothing is dropped: a line this does not
/// understand comes back as `Other` and is written out unchanged.
pub fn parse(text: &str) -> Vec<Line> {
    text.lines()
        .map(|raw| {
            let trimmed = raw.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                return Line::Other(raw.to_string());
            }
            let (export, rest) = match trimmed.strip_prefix("export ") {
                Some(r) => (true, r.trim_start()),
                None => (false, trimmed),
            };
            let Some((key, value)) = rest.split_once('=') else {
                return Line::Other(raw.to_string());
            };
            let key = key.trim();
            if key.is_empty()
                || !key
                    .chars()
                    .all(|c| c.is_alphanumeric() || c == '_' || c == '.')
            {
                return Line::Other(raw.to_string());
            }
            let value = value.trim();
            let (quote, value) = match value.chars().next() {
                Some(q @ ('"' | '\'')) if value.len() >= 2 && value.ends_with(q) => {
                    (Some(q), &value[1..value.len() - 1])
                }
                _ => (None, value),
            };
            Line::Pair {
                key: key.to_string(),
                value: value.to_string(),
                export,
                quote,
            }
        })
        .collect()
}

/// Write the lines back out.
pub fn render(lines: &[Line]) -> String {
    let mut out = String::new();
    for l in lines {
        match l {
            Line::Other(raw) => out.push_str(raw),
            Line::Pair {
                key,
                value,
                export,
                quote,
            } => {
                if *export {
                    out.push_str("export ");
                }
                out.push_str(key);
                out.push('=');
                // A value that needs quoting gets them even if it had
                // none: a space or a `#` would otherwise change meaning.
                let needs = value.contains([' ', '\t', '#', '"', '\''])
                    || value.is_empty() && quote.is_some();
                match quote.or(needs.then_some('"')) {
                    Some(q) => {
                        out.push(q);
                        out.push_str(&value.replace(q, &format!("\\{q}")));
                        out.push(q);
                    }
                    None => out.push_str(value),
                }
            }
        }
        out.push('\n');
    }
    out
}

#[derive(Default)]
pub struct State {
    pub path: Option<std::path::PathBuf>,
    pub lines: Vec<Line>,
    pub error: Option<String>,
    pub dirty: bool,
    /// Keys whose value is shown in the clear this session.
    pub revealed: Vec<String>,
    pub mask: bool,
}

pub fn open(state: &mut State, path: std::path::PathBuf) {
    match std::fs::read_to_string(&path) {
        Ok(text) => {
            state.lines = parse(&text);
            state.path = Some(path);
            state.error = None;
            state.dirty = false;
            state.revealed.clear();
        }
        Err(e) => state.error = Some(e.to_string()),
    }
}

pub fn ui(state: &mut State, ui: &mut egui::Ui, s: &Services, theme: &Theme) {
    use hs_ui::table::{EditColumn, edit_table};
    use hs_ui::tip::TipExt;
    // Masking is on until the user turns it off (a .env is usually full
    // of secrets, and this view is as shareable as a screenshot).
    if state.path.is_none() && !state.mask {
        state.mask = true;
    }
    crate::util::page_title(ui, s, theme, "dt-env", Some("dt-env-sub"));
    form::section(ui, theme, &s.t("dt-env"), |ui| {
        form::hint(ui, theme, &s.t("dt-env-hint"));
        ui.add_space(form::ROW_GAP);
        crate::util::button_row(ui, |ui| {
            if form::button(ui, theme, ph::FOLDER_OPEN, &s.t("dt-env-open")).clicked()
                && let Some(p) = rfd::FileDialog::new()
                    .add_filter("env", &["env", "txt"])
                    .pick_file()
            {
                open(state, p);
            }
            if let Some(p) = &state.path {
                ui.add(
                    egui::Label::new(egui::RichText::new(p.to_string_lossy()).small().monospace())
                        .truncate(),
                );
            }
        });
        if let Some(e) = &state.error {
            form::note(ui, theme, ph::WARNING, e);
        }
        if state.path.is_none() {
            return;
        }
        ui.add_space(form::ROW_GAP);
        form::centred_row(ui, |ui| {
            ui.checkbox(&mut state.mask, s.t("dt-env-mask"));
            ui.add_space(form::OPTION_GAP);
            let pairs = state
                .lines
                .iter()
                .filter(|l| matches!(l, Line::Pair { .. }))
                .count();
            ui.label(
                egui::RichText::new(s.i18n.t_args("dt-env-count", &[("n", &pairs.to_string())]))
                    .small()
                    .color(theme.text_muted),
            );
        });
        ui.add_space(form::ROW_GAP);
        // The rows of the table are the pairs; comments and blank lines
        // stay in `lines` where they are, unseen.
        let pairs: Vec<usize> = state
            .lines
            .iter()
            .enumerate()
            .filter(|(_, l)| matches!(l, Line::Pair { .. }))
            .map(|(i, _)| i)
            .collect();
        let (key_col, value_col, actions_col, empty) = (
            s.t("dt-env-col-key"),
            s.t("dt-env-col-value"),
            s.t("dt-col-actions"),
            s.t("dt-env-none"),
        );
        let (show, hide) = (s.t("dt-env-show"), s.t("dt-env-hide"));
        let mut reveal: Option<String> = None;
        let mut remove: Option<usize> = None;
        let mut dirty = false;
        let mask = state.mask;
        let revealed = &state.revealed;
        let lines = &mut state.lines;
        egui::ScrollArea::vertical()
            .id_salt("dt-env-rows")
            .max_height(360.0)
            .show(ui, |ui| {
                edit_table(
                    ui,
                    theme,
                    "dt-env-table",
                    &[
                        EditColumn::new(&key_col, 220.0),
                        EditColumn::new(&value_col, 380.0),
                        EditColumn::new(&actions_col, 0.0),
                    ],
                    pairs.len(),
                    Some(&empty),
                    |ui, r, c| {
                        let i = pairs[r];
                        let Some(Line::Pair { key, value, .. }) = lines.get_mut(i) else {
                            return;
                        };
                        let secret = looks_secret(key);
                        let hidden = mask && secret && !revealed.iter().any(|x| x == key);
                        match c {
                            0 => {
                                let w = ui.available_width();
                                if ui
                                    .add(
                                        form::line_edit(key)
                                            .font(egui::TextStyle::Monospace)
                                            .desired_width(w),
                                    )
                                    .changed()
                                {
                                    dirty = true;
                                }
                            }
                            1 => {
                                let w = ui.available_width();
                                if hidden {
                                    let mut dots = "•".repeat(value.chars().count().clamp(6, 24));
                                    ui.add_enabled(
                                        false,
                                        form::line_edit(&mut dots).desired_width(w),
                                    );
                                } else if ui
                                    .add(
                                        form::line_edit(value)
                                            .font(egui::TextStyle::Monospace)
                                            .desired_width(w),
                                    )
                                    .changed()
                                {
                                    dirty = true;
                                }
                            }
                            _ => {
                                // Show/Hide is drawn on every row, disabled on
                                // a key that is not secret-looking, so the
                                // column stays aligned (CR-0061).
                                let (icon, tip) = if hidden {
                                    (hs_ui::icons::EYE, &show)
                                } else {
                                    (hs_ui::icons::EYE_SLASH, &hide)
                                };
                                if ui
                                    .add_enabled_ui(secret, |ui| {
                                        form::icon_button(ui, theme, icon, 14.0, false)
                                    })
                                    .inner
                                    .tip(tip.as_str())
                                    .clicked()
                                {
                                    reveal = Some(key.clone());
                                }
                                if form::icon_button(ui, theme, hs_ui::icons::TRASH, 14.0, false)
                                    .tip(s.t("delete"))
                                    .clicked()
                                {
                                    remove = Some(i);
                                }
                            }
                        }
                    },
                );
            });
        if dirty {
            state.dirty = true;
        }
        if let Some(k) = reveal {
            match state.revealed.iter().position(|x| *x == k) {
                Some(i) => {
                    state.revealed.remove(i);
                }
                None => state.revealed.push(k),
            }
        }
        if let Some(i) = remove {
            state.lines.remove(i);
            state.dirty = true;
        }
        ui.add_space(form::ROW_GAP);
        crate::util::button_row(ui, |ui| {
            if form::button(ui, theme, ph::PLUS, &s.t("dt-env-add")).clicked() {
                state.lines.push(Line::Pair {
                    key: "NEW_KEY".into(),
                    value: String::new(),
                    export: false,
                    quote: None,
                });
                state.dirty = true;
            }
            if crate::util::button_if(ui, theme, state.dirty, ph::FLOPPY_DISK, &s.t("save"))
                .clicked()
                && let Some(p) = state.path.clone()
            {
                match std::fs::write(&p, render(&state.lines)) {
                    Ok(()) => {
                        state.dirty = false;
                        state.error = None;
                    }
                    Err(e) => state.error = Some(e.to_string()),
                }
            }
            if form::button(ui, theme, hs_ui::icons::COPY, &s.t("dt-copy")).clicked() {
                ui.ctx().copy_text(render(&state.lines));
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "# the database\nDB_HOST=localhost\nexport DB_PASSWORD=\"s3cr3t pw\"\nEMPTY=\n\nAPI_KEY='abc123'\nnot a pair\nPORT=8080\n";

    #[test]
    fn a_file_parses_into_pairs_and_keeps_the_rest() {
        let lines = parse(SAMPLE);
        assert_eq!(lines.len(), 8);
        assert_eq!(lines[0], Line::Other("# the database".into()));
        assert_eq!(
            lines[1],
            Line::Pair {
                key: "DB_HOST".into(),
                value: "localhost".into(),
                export: false,
                quote: None
            }
        );
        assert_eq!(
            lines[2],
            Line::Pair {
                key: "DB_PASSWORD".into(),
                value: "s3cr3t pw".into(),
                export: true,
                quote: Some('"')
            }
        );
        assert_eq!(lines[4], Line::Other(String::new()));
        assert_eq!(lines[6], Line::Other("not a pair".into()));
    }

    #[test]
    fn rendering_puts_it_back_as_it_was() {
        assert_eq!(render(&parse(SAMPLE)), SAMPLE);
    }

    #[test]
    fn a_value_that_needs_quotes_gets_them() {
        let lines = vec![Line::Pair {
            key: "MSG".into(),
            value: "hello world # not a comment".into(),
            export: false,
            quote: None,
        }];
        assert_eq!(render(&lines), "MSG=\"hello world # not a comment\"\n");
    }

    #[test]
    fn secret_looking_keys_are_recognised() {
        for k in [
            "DB_PASSWORD",
            "api_key",
            "STRIPE_SECRET",
            "GITHUB_TOKEN",
            "SESSION_COOKIE",
            "aws_private_key",
        ] {
            assert!(looks_secret(k), "{k}");
        }
        for k in ["DB_HOST", "PORT", "LOG_LEVEL", "NODE_ENV"] {
            assert!(!looks_secret(k), "{k}");
        }
    }
}
