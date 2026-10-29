//! Blame: every line of a file with the commit that last touched it,
//! the gutter coloured by age (docs/tools/git-activity.md §5.3).

use std::path::PathBuf;
use std::sync::Arc;

use hs_core::services::Services;
use hs_core::tool::{ToastLevel, ToolContext};
use hs_git::blame::BlameLine;
use hs_ui::icons::ph;
use hs_ui::theme::Theme;
use parking_lot::Mutex;

type Slot<T> = Arc<Mutex<Option<T>>>;

#[derive(Default)]
pub struct BlameView {
    pub path: String,
    /// The revision blamed, or the work tree.
    pub rev: Option<String>,
    lines: Vec<BlameLine>,
    loading: bool,
    slot: Slot<(String, Result<Vec<BlameLine>, String>)>,
}

impl BlameView {
    pub fn load(&mut self, s: &Services, root: PathBuf, path: String, rev: Option<String>) {
        self.path = path.clone();
        self.rev = rev.clone();
        self.lines.clear();
        self.loading = true;
        let slot = self.slot.clone();
        s.runtime.spawn_blocking(move || {
            let r = hs_git::blame(&root, &path, rev.as_deref()).map_err(|e| e.to_string());
            *slot.lock() = Some((path, r));
        });
    }

    fn poll(&mut self, cx: &mut ToolContext) {
        let got = self.slot.lock().take();
        if let Some((path, r)) = got
            && path == self.path
        {
            self.loading = false;
            match r {
                Ok(l) => self.lines = l,
                Err(e) => cx.toast(ToastLevel::Error, e),
            }
        }
    }

    /// Returns the commit id the reader clicked in the gutter, to be
    /// shown in History.
    pub fn ui(&mut self, ui: &mut egui::Ui, theme: &Theme, cx: &mut ToolContext) -> Option<String> {
        self.poll(cx);
        let s = cx.services;
        let mut pick = None;
        egui::Frame::new()
            .inner_margin(egui::Margin::symmetric(12, 6))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(ph::USER_FOCUS).color(theme.text_muted));
                    ui.label(egui::RichText::new(&self.path).strong());
                    ui.label(
                        egui::RichText::new(match &self.rev {
                            Some(r) => format!("{} {}", s.t("git-blame-at"), &r[..r.len().min(7)]),
                            None => s.t("git-blame-worktree"),
                        })
                        .color(theme.text_muted),
                    );
                    if self.loading {
                        ui.spinner();
                    }
                });
            });
        ui.separator();
        if self.lines.is_empty() {
            return None;
        }
        let (oldest, newest) = self.lines.iter().fold((i64::MAX, i64::MIN), |(lo, hi), l| {
            (lo.min(l.time), hi.max(l.time))
        });
        let span = (newest - oldest).max(1) as f32;
        let font = egui::TextStyle::Monospace.resolve(ui.style());
        let row_h = ui.fonts_mut(|f| f.row_height(&font)) + 4.0;
        let gutter_w = 300.0;
        egui::ScrollArea::both()
            .id_salt(ui.id().with(("git-blame", &self.path)))
            .auto_shrink([false, false])
            .show_rows(ui, row_h, self.lines.len(), |ui, range| {
                ui.set_min_width(ui.available_width());
                let mut last_id = String::new();
                if range.start > 0 {
                    last_id = self.lines[range.start - 1].id.clone();
                }
                for i in range {
                    let l = &self.lines[i];
                    let (rect, resp) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width().max(gutter_w + 400.0), row_h),
                        egui::Sense::click(),
                    );
                    // Newer lines warmer: the accent at full strength for
                    // the newest, fading to the surface for the oldest.
                    let age = (l.time - oldest) as f32 / span;
                    let tint = theme.accent.linear_multiply(0.05 + 0.25 * age);
                    let gutter = egui::Rect::from_min_size(rect.min, egui::vec2(gutter_w, row_h));
                    ui.painter().rect_filled(gutter, 0.0, tint);
                    if l.id != last_id {
                        let text = format!(
                            "{}  {:<16}  {}",
                            &l.id[..l.id.len().min(7)],
                            truncate(&l.author, 16),
                            crate::history::local_date(l.time)
                        );
                        ui.painter().with_clip_rect(gutter).text(
                            egui::pos2(gutter.left() + 8.0, rect.center().y),
                            egui::Align2::LEFT_CENTER,
                            text,
                            font.clone(),
                            theme.text,
                        );
                        last_id = l.id.clone();
                    }
                    ui.painter().text(
                        egui::pos2(gutter.right() + 8.0, rect.center().y),
                        egui::Align2::LEFT_CENTER,
                        format!("{:>5}", l.line),
                        font.clone(),
                        theme.text_muted,
                    );
                    ui.painter().text(
                        egui::pos2(gutter.right() + 60.0, rect.center().y),
                        egui::Align2::LEFT_CENTER,
                        &l.text,
                        font.clone(),
                        theme.text,
                    );
                    let resp = resp.on_hover_text(format!("{}\n{}", l.summary, l.author));
                    if resp.clicked()
                        && resp
                            .interact_pointer_pos()
                            .is_some_and(|p| gutter.contains(p))
                    {
                        pick = Some(l.id.clone());
                    }
                }
            });
        pick
    }
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let mut t: String = s.chars().take(n - 1).collect();
        t.push('\u{2026}');
        t
    }
}
