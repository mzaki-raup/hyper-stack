//! The History view: the commit graph (lanes painted from
//! `hs_git::graph`), branch and tag chips, author, date and subject;
//! the selected commit's details and changed files below it; a file's
//! diff goes to the right column (docs/tools/git-activity.md §5.3).

use std::path::PathBuf;
use std::sync::Arc;

use hs_core::services::Services;
use hs_core::tool::{ToastLevel, ToolContext};
use hs_git::graph::Row;
use hs_git::{ChangedFile, Commit, CommitDetails};
use hs_ui::icons::ph;
use hs_ui::theme::Theme;
use parking_lot::Mutex;

type Slot<T> = Arc<Mutex<Option<T>>>;

/// Commits read per page; Load More adds another.
const PAGE: usize = 300;
const LANE_W: f32 = 14.0;
const ROW_H: f32 = 24.0;

/// Lane colours, cycled by column.
const LANE_COLOURS: [egui::Color32; 6] = [
    egui::Color32::from_rgb(0xf0, 0x88, 0x3e),
    egui::Color32::from_rgb(0x60, 0xa5, 0xfa),
    egui::Color32::from_rgb(0x4a, 0xde, 0x80),
    egui::Color32::from_rgb(0xc0, 0x84, 0xfc),
    egui::Color32::from_rgb(0xfb, 0xbf, 0x24),
    egui::Color32::from_rgb(0xf8, 0x71, 0x71),
];

/// What the reader asked for.
pub enum Pick {
    /// Show this file of this commit in the diff column.
    FileDiff {
        id: String,
        first_parent: Option<String>,
        file: ChangedFile,
    },
    /// Blame this file at this commit.
    Blame {
        id: String,
        path: String,
    },
    Checkout(String),
    Cherry(String),
    BranchAt(String),
    TagAt(String),
}

pub struct HistoryView {
    root: Option<PathBuf>,
    /// The path the log is limited to (File History), or none.
    pub path: Option<String>,
    commits: Vec<Commit>,
    rows: Vec<Row>,
    /// The page being read, so Load More and the first read share a slot.
    log_slot: Slot<(usize, Result<Vec<Commit>, String>)>,
    exhausted: bool,
    loading: bool,
    selected: Option<usize>,
    details: Option<Result<CommitDetails, String>>,
    details_slot: Slot<(String, Result<CommitDetails, String>)>,
    pub search: String,
    /// Whether the graph and the details refresh on the next status
    /// change (a commit made in the File Status view).
    pub stale: bool,
}

impl Default for HistoryView {
    fn default() -> Self {
        Self {
            root: None,
            path: None,
            commits: Vec::new(),
            rows: Vec::new(),
            log_slot: Slot::default(),
            exhausted: false,
            loading: false,
            selected: None,
            details: None,
            details_slot: Slot::default(),
            search: String::new(),
            stale: true,
        }
    }
}

impl HistoryView {
    pub fn set_root(&mut self, root: PathBuf) {
        *self = Self::default();
        self.root = Some(root);
    }

    /// Read the log again from the top (after a commit, a pull, …).
    pub fn reload(&mut self, s: &Services) {
        self.commits.clear();
        self.rows.clear();
        self.exhausted = false;
        self.selected = None;
        self.details = None;
        self.stale = false;
        self.load_page(s);
    }

    fn load_page(&mut self, s: &Services) {
        let Some(root) = self.root.clone() else {
            return;
        };
        if self.loading || self.exhausted {
            return;
        }
        self.loading = true;
        let skip = self.commits.len();
        let path = self.path.clone();
        let slot = self.log_slot.clone();
        s.runtime.spawn_blocking(move || {
            let r = hs_git::log_with(&root, path.is_none(), path.as_deref(), skip, PAGE)
                .map_err(|e| e.to_string());
            *slot.lock() = Some((skip, r));
        });
    }

    fn select(&mut self, s: &Services, i: usize) {
        self.selected = Some(i);
        self.details = None;
        let Some(root) = self.root.clone() else {
            return;
        };
        let Some(c) = self.commits.get(i) else {
            return;
        };
        let id = c.id.clone();
        let parent = c.parents.first().cloned();
        let slot = self.details_slot.clone();
        s.runtime.spawn_blocking(move || {
            let d =
                hs_git::commit_details(&root, &id, parent.as_deref()).map_err(|e| e.to_string());
            *slot.lock() = Some((id, d));
        });
    }

    fn poll(&mut self, cx: &mut ToolContext) {
        let page = self.log_slot.lock().take();
        if let Some((skip, r)) = page {
            self.loading = false;
            match r {
                Ok(mut more) if skip == self.commits.len() => {
                    if more.len() < PAGE {
                        self.exhausted = true;
                    }
                    self.commits.append(&mut more);
                    self.rows = hs_git::graph::lanes(&self.commits);
                }
                Ok(_) => {}
                Err(e) => cx.toast(ToastLevel::Error, e),
            }
        }
        let d = self.details_slot.lock().take();
        if let Some((id, d)) = d
            && self
                .selected
                .and_then(|i| self.commits.get(i))
                .is_some_and(|c| c.id == id)
        {
            self.details = Some(d);
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, theme: &Theme, cx: &mut ToolContext) -> Option<Pick> {
        self.poll(cx);
        let s = cx.services;
        if self.stale {
            self.reload(s);
        }
        if self.loading {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(150));
        }
        let mut pick = None;
        let height = ui.available_height();
        egui::Panel::top(ui.id().with("git-history-list"))
            .default_size((height * 0.55).max(140.0))
            .size_range(100.0..=(height - 120.0).max(100.0))
            .resizable(true)
            .show(ui, |ui| {
                pick = self.list(ui, theme, s);
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(theme.panel_bg))
            .show(ui, |ui| {
                if let Some(p) = self.details_ui(ui, theme, s) {
                    pick = Some(p);
                }
            });
        pick
    }

    fn list(&mut self, ui: &mut egui::Ui, theme: &Theme, s: &Services) -> Option<Pick> {
        egui::Frame::new()
            .inner_margin(egui::Margin::symmetric(8, 6))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(ph::MAGNIFYING_GLASS).color(theme.text_muted));
                    ui.add(
                        egui::TextEdit::singleline(&mut self.search)
                            .hint_text(s.t("git-history-search-hint"))
                            .desired_width(260.0),
                    );
                    if let Some(p) = &self.path {
                        ui.label(
                            egui::RichText::new(format!("{} {p}", ph::FILE))
                                .color(theme.text_muted),
                        );
                        if ui.small_button(ph::X).clicked() {
                            self.path = None;
                            self.stale = true;
                        }
                    }
                    if self.loading {
                        ui.spinner();
                    }
                });
            });
        if self.commits.is_empty() && !self.loading {
            ui.horizontal(|ui| {
                ui.add_space(12.0);
                ui.label(egui::RichText::new(s.t("git-no-commits")).color(theme.text_muted));
            });
            return None;
        }
        let needle = self.search.trim().to_lowercase();
        let shown: Vec<usize> = (0..self.commits.len())
            .filter(|&i| {
                if needle.is_empty() {
                    return true;
                }
                let c = &self.commits[i];
                c.subject.to_lowercase().contains(&needle)
                    || c.author.to_lowercase().contains(&needle)
                    || c.id.starts_with(&needle)
                    || c.refs.iter().any(|r| r.to_lowercase().contains(&needle))
            })
            .collect();
        let graph_w =
            self.rows.iter().map(Row::width).max().unwrap_or(1).min(12) as f32 * LANE_W + 8.0;
        let mut select: Option<usize> = None;
        let mut load_more = false;
        let mut menu_pick: Option<Pick> = None;
        egui::ScrollArea::both()
            .id_salt(ui.id().with("git-history-rows"))
            .auto_shrink([false, false])
            .show_rows(ui, ROW_H, shown.len() + 1, |ui, range| {
                ui.set_min_width(ui.available_width());
                for k in range {
                    if k == shown.len() {
                        // The row after the last: Load More, or nothing.
                        if !self.exhausted
                            && needle.is_empty()
                            && ui
                                .add_enabled(
                                    !self.loading,
                                    egui::Button::new(s.t("git-load-more")).small(),
                                )
                                .clicked()
                        {
                            load_more = true;
                        }
                        continue;
                    }
                    let i = shown[k];
                    let (rect, resp) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width(), ROW_H),
                        egui::Sense::click(),
                    );
                    let selected = self.selected == Some(i);
                    let fill = if selected {
                        theme.accent.linear_multiply(0.30)
                    } else if resp.hovered() {
                        theme.surface_hover
                    } else {
                        egui::Color32::TRANSPARENT
                    };
                    ui.painter().rect_filled(rect, 0.0, fill);
                    // The graph strip: lines above and below, then the dot.
                    // With a search on, rows are not neighbours and the
                    // lines would lie, so only the dot is drawn.
                    if let Some(row) = self.rows.get(i) {
                        let x =
                            |lane: usize| rect.left() + 8.0 + lane as f32 * LANE_W + LANE_W / 2.0;
                        let p = ui.painter();
                        if needle.is_empty() {
                            for &l in &row.above {
                                if l != row.lane && !row.joins.contains(&l) {
                                    p.line_segment(
                                        [
                                            egui::pos2(x(l), rect.top()),
                                            egui::pos2(x(l), rect.center().y),
                                        ],
                                        egui::Stroke::new(1.5, lane_colour(l)),
                                    );
                                }
                            }
                            for &l in &row.below {
                                if !row.parents.contains(&l) {
                                    p.line_segment(
                                        [
                                            egui::pos2(x(l), rect.center().y),
                                            egui::pos2(x(l), rect.bottom()),
                                        ],
                                        egui::Stroke::new(1.5, lane_colour(l)),
                                    );
                                }
                            }
                            if row.above.contains(&row.lane) {
                                p.line_segment(
                                    [
                                        egui::pos2(x(row.lane), rect.top()),
                                        egui::pos2(x(row.lane), rect.center().y),
                                    ],
                                    egui::Stroke::new(1.5, lane_colour(row.lane)),
                                );
                            }
                            for &j in &row.joins {
                                p.line_segment(
                                    [
                                        egui::pos2(x(j), rect.top()),
                                        egui::pos2(x(row.lane), rect.center().y),
                                    ],
                                    egui::Stroke::new(1.5, lane_colour(j)),
                                );
                            }
                            for &pl in &row.parents {
                                p.line_segment(
                                    [
                                        egui::pos2(x(row.lane), rect.center().y),
                                        egui::pos2(x(pl), rect.bottom()),
                                    ],
                                    egui::Stroke::new(1.5, lane_colour(pl)),
                                );
                            }
                        }
                        p.circle_filled(
                            egui::pos2(x(row.lane), rect.center().y),
                            4.0,
                            lane_colour(row.lane),
                        );
                    }
                    let c = &self.commits[i];
                    let mut cursor = rect.left() + graph_w + 6.0;
                    let font = egui::TextStyle::Body.resolve(ui.style());
                    for r in &c.refs {
                        let (text, colour) = ref_chip(r, theme);
                        let galley = ui.painter().layout_no_wrap(
                            text,
                            egui::FontId::proportional(font.size - 1.5),
                            theme.text,
                        );
                        let chip = egui::Rect::from_min_size(
                            egui::pos2(cursor, rect.center().y - galley.size().y / 2.0 - 2.0),
                            galley.size() + egui::vec2(10.0, 4.0),
                        );
                        ui.painter()
                            .rect_filled(chip, 4.0, colour.linear_multiply(0.35));
                        ui.painter().rect_stroke(
                            chip,
                            4.0,
                            egui::Stroke::new(1.0, colour),
                            egui::StrokeKind::Inside,
                        );
                        ui.painter()
                            .galley(chip.min + egui::vec2(5.0, 2.0), galley, theme.text);
                        cursor = chip.right() + 6.0;
                    }
                    let meta_w = 230.0;
                    let subject_rect = egui::Rect::from_min_max(
                        egui::pos2(cursor, rect.top()),
                        egui::pos2(rect.right() - meta_w, rect.bottom()),
                    );
                    let galley =
                        ui.painter()
                            .layout_no_wrap(c.subject.clone(), font.clone(), theme.text);
                    ui.painter().with_clip_rect(subject_rect).galley(
                        egui::pos2(subject_rect.left(), rect.center().y - galley.size().y / 2.0),
                        galley,
                        theme.text,
                    );
                    let meta = format!("{}  {}  {}", c.short, c.author, local_date(c.time));
                    ui.painter().text(
                        egui::pos2(rect.right() - 8.0, rect.center().y),
                        egui::Align2::RIGHT_CENTER,
                        meta,
                        egui::FontId::proportional(font.size - 1.0),
                        theme.text_muted,
                    );
                    if resp.clicked() {
                        select = Some(i);
                    }
                    resp.context_menu(|ui| {
                        if ui.button(s.t("git-menu-copy-hash")).clicked() {
                            ui.ctx().copy_text(c.id.clone());
                            ui.close();
                        }
                        ui.separator();
                        if ui.button(s.t("git-menu-checkout-commit")).clicked() {
                            menu_pick = Some(Pick::Checkout(c.id.clone()));
                            ui.close();
                        }
                        if ui.button(s.t("git-menu-cherry-pick")).clicked() {
                            menu_pick = Some(Pick::Cherry(c.id.clone()));
                            ui.close();
                        }
                        if ui.button(s.t("git-menu-branch-here")).clicked() {
                            menu_pick = Some(Pick::BranchAt(c.id.clone()));
                            ui.close();
                        }
                        if ui.button(s.t("git-menu-tag-here")).clicked() {
                            menu_pick = Some(Pick::TagAt(c.id.clone()));
                            ui.close();
                        }
                    });
                }
            });
        // Up and Down step through the rows shown.
        if !ui.ctx().egui_wants_keyboard_input() && !shown.is_empty() {
            let (up, down) = ui.input(|i| {
                (
                    i.key_pressed(egui::Key::ArrowUp),
                    i.key_pressed(egui::Key::ArrowDown),
                )
            });
            if up || down {
                let cur = self
                    .selected
                    .and_then(|sel| shown.iter().position(|&i| i == sel));
                let k = match (cur, down) {
                    (None, _) => 0,
                    (Some(k), true) => (k + 1).min(shown.len() - 1),
                    (Some(k), false) => k.saturating_sub(1),
                };
                select = Some(shown[k]);
            }
        }
        if let Some(i) = select {
            self.select(s, i);
        }
        if load_more {
            self.load_page(s);
        }
        menu_pick
    }

    fn details_ui(&mut self, ui: &mut egui::Ui, theme: &Theme, s: &Services) -> Option<Pick> {
        let Some(i) = self.selected else {
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                ui.add_space(12.0);
                ui.label(egui::RichText::new(s.t("git-pick-commit")).color(theme.text_muted));
            });
            return None;
        };
        let c = self.commits.get(i)?.clone();
        let mut pick = None;
        egui::ScrollArea::vertical()
            .id_salt(ui.id().with("git-commit-details"))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                egui::Frame::new()
                    .inner_margin(egui::Margin::symmetric(12, 8))
                    .show(ui, |ui| match &self.details {
                        None => {
                            ui.spinner();
                        }
                        Some(Err(e)) => {
                            ui.label(egui::RichText::new(e).color(theme.error));
                        }
                        Some(Ok(d)) => {
                            ui.add(
                                egui::Label::new(egui::RichText::new(&d.message).strong()).wrap(),
                            );
                            ui.add_space(4.0);
                            ui.label(
                                egui::RichText::new(format!(
                                    "{} <{}> \u{00b7} {}",
                                    c.author,
                                    c.email,
                                    local_date(c.time)
                                ))
                                .color(theme.text_muted)
                                .small(),
                            );
                            ui.horizontal_wrapped(|ui| {
                                ui.label(
                                    egui::RichText::new(format!("{} {}", ph::GIT_COMMIT, c.id))
                                        .monospace()
                                        .small()
                                        .color(theme.text_muted),
                                );
                                if ui
                                    .small_button(ph::COPY)
                                    .on_hover_text(s.t("copy"))
                                    .clicked()
                                {
                                    ui.ctx().copy_text(c.id.clone());
                                }
                                for p in &c.parents {
                                    ui.label(
                                        egui::RichText::new(format!(
                                            "{} {}",
                                            ph::ARROW_UP,
                                            &p[..p.len().min(7)]
                                        ))
                                        .monospace()
                                        .small()
                                        .color(theme.text_muted),
                                    );
                                }
                            });
                            ui.add_space(6.0);
                            ui.label(
                                egui::RichText::new(s.i18n.t_args(
                                    "git-changed-files",
                                    &[("count", &d.files.len().to_string())],
                                ))
                                .strong(),
                            );
                            for f in &d.files {
                                ui.horizontal(|ui| {
                                    let colour = match f.status {
                                        'A' => theme.success,
                                        'D' => theme.error,
                                        'M' => theme.warning,
                                        _ => theme.accent,
                                    };
                                    ui.label(
                                        egui::RichText::new(f.status.to_string())
                                            .monospace()
                                            .color(colour),
                                    );
                                    let label = match &f.orig_path {
                                        Some(o) => format!("{o} \u{2192} {}", f.path),
                                        None => f.path.clone(),
                                    };
                                    if ui.add(egui::Button::new(label).frame(false)).clicked() {
                                        pick = Some(Pick::FileDiff {
                                            id: c.id.clone(),
                                            first_parent: c.parents.first().cloned(),
                                            file: f.clone(),
                                        });
                                    }
                                    if f.status != 'D'
                                        && ui
                                            .small_button(ph::USER_FOCUS)
                                            .on_hover_text(s.t("git-blame"))
                                            .clicked()
                                    {
                                        pick = Some(Pick::Blame {
                                            id: c.id.clone(),
                                            path: f.path.clone(),
                                        });
                                    }
                                });
                            }
                        }
                    });
            });
        pick
    }
}

fn lane_colour(lane: usize) -> egui::Color32 {
    LANE_COLOURS[lane % LANE_COLOURS.len()]
}

/// A ref decoration as chip text and colour: HEAD and local branches in
/// the accent, remotes muted, tags in the warning tone.
fn ref_chip(r: &str, theme: &Theme) -> (String, egui::Color32) {
    if let Some(t) = r.strip_prefix("tag: ") {
        return (format!("{} {t}", ph::TAG), theme.warning);
    }
    if let Some(b) = r.strip_prefix("HEAD -> ") {
        return (format!("{} {b}", ph::GIT_BRANCH), theme.accent);
    }
    if r == "HEAD" {
        return (r.to_string(), theme.accent);
    }
    if r.contains('/') {
        return (format!("{} {r}", ph::CLOUD), theme.text_muted);
    }
    (format!("{} {r}", ph::GIT_BRANCH), theme.success)
}

/// "2026-10-29 14:05" in local time.
pub fn local_date(unix: i64) -> String {
    use chrono::TimeZone as _;
    chrono::Local
        .timestamp_opt(unix, 0)
        .single()
        .map(|t| t.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_default()
}
