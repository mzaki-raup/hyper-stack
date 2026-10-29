//! The diff painter every view shares: hunks as rows of monospace text
//! with line numbers, coloured added/removed backgrounds, and - for a
//! work-tree or index diff - a Stage / Unstage / Discard button per hunk
//! (docs/tools/git-activity.md §5.3).

use hs_core::services::Services;
use hs_git::diff::LineKind;
use hs_git::{FileDiff, Side};
use hs_ui::icons::ph;
use hs_ui::theme::Theme;

/// What the reader asked of one hunk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HunkPick {
    Stage(usize),
    Unstage(usize),
    Discard(usize),
}

/// Paint `d`. `side` says which verbs a hunk offers; `None` (a commit's
/// diff) offers none. Returns the hunk verb clicked, if any.
pub fn show(
    ui: &mut egui::Ui,
    theme: &Theme,
    s: &Services,
    d: &FileDiff,
    side: Option<Side>,
    busy: bool,
) -> Option<HunkPick> {
    let mut pick = None;
    if d.binary {
        ui.label(egui::RichText::new(s.t("git-binary")).color(theme.text_muted));
        return None;
    }
    for n in &d.notes {
        ui.label(egui::RichText::new(n).color(theme.text_muted).italics());
    }
    if d.hunks.is_empty() {
        ui.label(egui::RichText::new(s.t("git-no-changes")).color(theme.text_muted));
        return None;
    }
    let added_bg = theme.success.linear_multiply(0.16);
    let removed_bg = theme.error.linear_multiply(0.16);
    egui::ScrollArea::both()
        .id_salt(ui.id().with(("git-diff", &d.path)))
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            let font = egui::TextStyle::Monospace.resolve(ui.style());
            let row_h = ui.fonts_mut(|f| f.row_height(&font)) + 4.0;
            let gutter = 96.0;
            for (hi, h) in d.hunks.iter().enumerate() {
                let (rect, _) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), row_h + 4.0),
                    egui::Sense::hover(),
                );
                ui.painter().rect_filled(rect, 0.0, theme.surface);
                ui.painter().text(
                    rect.left_center() + egui::vec2(8.0, 0.0),
                    egui::Align2::LEFT_CENTER,
                    &h.header,
                    font.clone(),
                    theme.text_muted,
                );
                // The verbs sit at the right edge of the hunk header.
                if let Some(side) = side {
                    let mut child = ui.new_child(
                        egui::UiBuilder::new()
                            .max_rect(rect.shrink2(egui::vec2(6.0, 1.0)))
                            .layout(egui::Layout::right_to_left(egui::Align::Center)),
                    );
                    match side {
                        Side::Staged => {
                            if child
                                .add_enabled(
                                    !busy,
                                    egui::Button::new(format!(
                                        "{} {}",
                                        ph::MINUS,
                                        s.t("git-unstage")
                                    ))
                                    .small(),
                                )
                                .clicked()
                            {
                                pick = Some(HunkPick::Unstage(hi));
                            }
                        }
                        Side::Unstaged => {
                            if child
                                .add_enabled(
                                    !busy,
                                    egui::Button::new(format!(
                                        "{} {}",
                                        ph::ARROW_COUNTER_CLOCKWISE,
                                        s.t("git-discard-button")
                                    ))
                                    .small(),
                                )
                                .clicked()
                            {
                                pick = Some(HunkPick::Discard(hi));
                            }
                            if child
                                .add_enabled(
                                    !busy,
                                    egui::Button::new(format!("{} {}", ph::PLUS, s.t("git-stage")))
                                        .small(),
                                )
                                .clicked()
                            {
                                pick = Some(HunkPick::Stage(hi));
                            }
                        }
                        Side::Untracked => {}
                    }
                }
                for l in &h.lines {
                    let (rect, _) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width().max(rect.width()), row_h),
                        egui::Sense::hover(),
                    );
                    let (bg, sign, colour) = match l.kind {
                        LineKind::Added => (Some(added_bg), '+', theme.text),
                        LineKind::Removed => (Some(removed_bg), '-', theme.text),
                        LineKind::Context => (None, ' ', theme.text),
                        LineKind::NoNewline => (None, '\\', theme.text_muted),
                    };
                    if let Some(bg) = bg {
                        ui.painter().rect_filled(rect, 0.0, bg);
                    }
                    let nums = format!(
                        "{:>4} {:>4}",
                        l.old.map(|n| n.to_string()).unwrap_or_default(),
                        l.new.map(|n| n.to_string()).unwrap_or_default()
                    );
                    ui.painter().text(
                        rect.left_center() + egui::vec2(8.0, 0.0),
                        egui::Align2::LEFT_CENTER,
                        nums,
                        font.clone(),
                        theme.text_muted,
                    );
                    ui.painter().text(
                        rect.left_center() + egui::vec2(gutter, 0.0),
                        egui::Align2::LEFT_CENTER,
                        format!("{sign} {}", l.text),
                        font.clone(),
                        colour,
                    );
                }
            }
        });
    pick
}

/// The "+n -m" pair after a file name.
pub fn counts(ui: &mut egui::Ui, theme: &Theme, d: &FileDiff) {
    ui.label(egui::RichText::new(format!("+{}", d.added())).color(theme.success));
    ui.label(egui::RichText::new(format!("-{}", d.removed())).color(theme.error));
}
