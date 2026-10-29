//! The conflict editor: an unmerged file block by block - Ours, Theirs,
//! Both per block - and **Save and Mark Resolved**, which writes the
//! file without markers and stages it (docs/tools/git-activity.md §5.3).
//! The parsing and the resolution are `hs_git::conflict`; this is only
//! the surface.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use hs_core::services::Services;
use hs_core::tool::{ToastLevel, ToolContext};
use hs_git::conflict::{Choice, Parsed, Segment};
use hs_ui::icons::ph;
use hs_ui::theme::Theme;
use parking_lot::Mutex;

type Slot<T> = Arc<Mutex<Option<T>>>;

#[derive(Default)]
pub struct ConflictView {
    pub path: String,
    parsed: Option<Parsed>,
    choices: Vec<Choice>,
    read_slot: Slot<(String, Result<String, String>)>,
    /// Save finished: `Ok` means the file is resolved and staged.
    save_slot: Slot<Result<(), String>>,
    saving: bool,
}

impl ConflictView {
    pub fn load(&mut self, s: &Services, root: PathBuf, path: String) {
        self.path = path.clone();
        self.parsed = None;
        self.choices.clear();
        let slot = self.read_slot.clone();
        s.runtime.spawn_blocking(move || {
            let r = std::fs::read_to_string(root.join(&path)).map_err(|e| e.to_string());
            *slot.lock() = Some((path, r));
        });
    }

    /// Whether a save just finished, so the caller refreshes its status.
    fn poll(&mut self, cx: &mut ToolContext) -> bool {
        let got = self.read_slot.lock().take();
        if let Some((path, r)) = got
            && path == self.path
        {
            match r {
                Ok(text) => {
                    let parsed = hs_git::conflict::parse(&text);
                    self.choices = vec![Choice::Unresolved; parsed.conflicts()];
                    self.parsed = Some(parsed);
                }
                Err(e) => cx.toast(ToastLevel::Error, e),
            }
        }
        let saved = self.save_slot.lock().take();
        match saved {
            Some(Ok(())) => {
                self.saving = false;
                cx.toast(ToastLevel::Success, cx.services.t("git-conflict-resolved"));
                true
            }
            Some(Err(e)) => {
                self.saving = false;
                cx.toast(ToastLevel::Error, e);
                false
            }
            None => false,
        }
    }

    /// Returns true when the file was resolved and staged this frame.
    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        theme: &Theme,
        cx: &mut ToolContext,
        root: &Path,
    ) -> bool {
        let done = self.poll(cx);
        let s = cx.services;
        egui::Frame::new()
            .inner_margin(egui::Margin::symmetric(12, 6))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(ph::WARNING).color(theme.warning));
                    ui.label(egui::RichText::new(&self.path).strong());
                    if let Some(p) = &self.parsed {
                        let left = self
                            .choices
                            .iter()
                            .filter(|c| **c == Choice::Unresolved)
                            .count();
                        ui.label(
                            egui::RichText::new(s.i18n.t_args(
                                "git-conflict-blocks",
                                &[
                                    ("total", &p.conflicts().to_string()),
                                    ("left", &left.to_string()),
                                ],
                            ))
                            .color(theme.text_muted),
                        );
                    }
                });
                // The verbs on their own row: beside the count they ran
                // into it in a narrow column (seen on screen).
                ui.horizontal(|ui| {
                    if self.parsed.is_some() {
                        let left = self
                            .choices
                            .iter()
                            .filter(|c| **c == Choice::Unresolved)
                            .count();
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let can = left == 0 && !self.saving;
                            if ui
                                .add_enabled(
                                    can,
                                    egui::Button::new(format!(
                                        "{} {}",
                                        ph::CHECK,
                                        s.t("git-conflict-save")
                                    ))
                                    .fill(if can {
                                        theme.accent
                                    } else {
                                        theme.surface
                                    }),
                                )
                                .clicked()
                            {
                                self.save(s, root.to_path_buf());
                            }
                            if ui.button(s.t("git-conflict-all-ours")).clicked() {
                                self.choices.iter_mut().for_each(|c| *c = Choice::Ours);
                            }
                            if ui.button(s.t("git-conflict-all-theirs")).clicked() {
                                self.choices.iter_mut().for_each(|c| *c = Choice::Theirs);
                            }
                        });
                    }
                });
            });
        ui.separator();
        let Some(parsed) = self.parsed.clone() else {
            ui.spinner();
            return done;
        };
        let font = egui::TextStyle::Monospace.resolve(ui.style());
        egui::ScrollArea::both()
            .id_salt(ui.id().with(("git-conflict", &self.path)))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                let mut n = 0;
                for seg in &parsed.segments {
                    match seg {
                        Segment::Context(text) => {
                            for line in text.lines() {
                                ui.label(
                                    egui::RichText::new(line)
                                        .font(font.clone())
                                        .color(theme.text_muted),
                                );
                            }
                        }
                        Segment::Conflict(c) => {
                            let i = n;
                            n += 1;
                            let choice = self.choices.get(i).cloned().unwrap_or(Choice::Unresolved);
                            egui::Frame::new()
                                .fill(theme.surface)
                                .stroke(egui::Stroke::new(
                                    1.0,
                                    if choice == Choice::Unresolved {
                                        theme.warning
                                    } else {
                                        theme.success
                                    },
                                ))
                                .inner_margin(egui::Margin::same(6))
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            egui::RichText::new(format!("#{}", i + 1)).strong(),
                                        );
                                        for (label, want) in [
                                            (
                                                format!(
                                                    "{} {}",
                                                    s.t("git-conflict-ours"),
                                                    c.ours_label
                                                ),
                                                Choice::Ours,
                                            ),
                                            (
                                                format!(
                                                    "{} {}",
                                                    s.t("git-conflict-theirs"),
                                                    c.theirs_label
                                                ),
                                                Choice::Theirs,
                                            ),
                                            (s.t("git-conflict-both"), Choice::Both),
                                        ] {
                                            if ui
                                                .add(egui::Button::selectable(
                                                    choice == want,
                                                    label,
                                                ))
                                                .clicked()
                                                && let Some(slot) = self.choices.get_mut(i)
                                            {
                                                *slot = want;
                                            }
                                        }
                                    });
                                    side(
                                        ui,
                                        theme,
                                        &font,
                                        &c.ours_label,
                                        &c.ours,
                                        theme.success,
                                        choice == Choice::Ours || choice == Choice::Both,
                                    );
                                    side(
                                        ui,
                                        theme,
                                        &font,
                                        &c.theirs_label,
                                        &c.theirs,
                                        theme.accent,
                                        choice == Choice::Theirs || choice == Choice::Both,
                                    );
                                });
                            ui.add_space(4.0);
                        }
                    }
                }
            });
        done
    }

    fn save(&mut self, s: &Services, root: PathBuf) {
        let Some(parsed) = self.parsed.clone() else {
            return;
        };
        let text = match hs_git::conflict::resolve(&parsed, &self.choices) {
            Ok(t) => t,
            Err(_) => return,
        };
        self.saving = true;
        let path = self.path.clone();
        let slot = self.save_slot.clone();
        s.runtime.spawn_blocking(move || {
            let r = std::fs::write(root.join(&path), text)
                .map_err(|e| e.to_string())
                .and_then(|()| hs_git::write::stage(&root, &[&path]).map_err(|e| e.to_string()));
            *slot.lock() = Some(r);
        });
    }
}

fn side(
    ui: &mut egui::Ui,
    theme: &Theme,
    font: &egui::FontId,
    label: &str,
    text: &str,
    colour: egui::Color32,
    chosen: bool,
) {
    let bg = if chosen {
        colour.linear_multiply(0.16)
    } else {
        egui::Color32::TRANSPARENT
    };
    egui::Frame::new()
        .fill(bg)
        .inner_margin(egui::Margin::symmetric(6, 2))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(egui::RichText::new(label).small().color(colour));
            for line in text.lines() {
                ui.label(
                    egui::RichText::new(line)
                        .font(font.clone())
                        .color(theme.text),
                );
            }
        });
}
