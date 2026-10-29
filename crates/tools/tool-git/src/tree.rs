//! The left tree: Workspace (File Status, History), Branches, Remotes,
//! Tags, Stashes (docs/tools/git-activity.md §5.3). Read-only in phase
//! 2; the verbs on its rows arrive with phase 3.

use std::path::PathBuf;
use std::sync::Arc;

use hs_core::services::Services;
use hs_core::tool::{ToastLevel, ToolContext};
use hs_git::{Branch, Remote, Stash};
use hs_ui::icons::ph;
use hs_ui::theme::Theme;
use parking_lot::Mutex;

type Slot<T> = Arc<Mutex<Option<T>>>;

/// Which centre view the Workspace section points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum View {
    #[default]
    Status,
    History,
}

/// Everything the tree lists, read in one go on the blocking pool.
#[derive(Debug, Default, Clone)]
pub struct Refs {
    pub branches: Vec<Branch>,
    pub remotes: Vec<Remote>,
    pub tags: Vec<String>,
    pub stashes: Vec<Stash>,
}

/// What the reader clicked.
pub enum Pick {
    View(View),
}

#[derive(Default)]
pub struct Tree {
    root: Option<PathBuf>,
    pub refs: Refs,
    slot: Slot<Result<Refs, String>>,
    pub view: View,
    open_branches: bool,
    open_remotes: bool,
    open_tags: bool,
    open_stashes: bool,
    /// Remotes whose branch list is unfolded.
    open_remote: Vec<String>,
}

impl Tree {
    pub fn set_root(&mut self, root: PathBuf) {
        *self = Self {
            root: Some(root),
            open_branches: true,
            open_remotes: true,
            open_tags: false,
            open_stashes: true,
            ..Self::default()
        };
    }

    /// Read branches, remotes, tags and stashes again.
    pub fn refresh(&mut self, s: &Services) {
        let Some(root) = self.root.clone() else {
            return;
        };
        let slot = self.slot.clone();
        s.runtime.spawn_blocking(move || {
            let r = (|| -> Result<Refs, String> {
                Ok(Refs {
                    branches: hs_git::branches(&root).map_err(|e| e.to_string())?,
                    remotes: hs_git::write::remotes(&root).map_err(|e| e.to_string())?,
                    tags: hs_git::write::tags(&root).map_err(|e| e.to_string())?,
                    stashes: hs_git::write::stash_list(&root).map_err(|e| e.to_string())?,
                })
            })();
            *slot.lock() = Some(r);
        });
    }

    fn poll(&mut self, cx: &mut ToolContext) {
        let got = self.slot.lock().take();
        match got {
            Some(Ok(r)) => self.refs = r,
            Some(Err(e)) => cx.toast(ToastLevel::Error, e),
            None => {}
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, theme: &Theme, cx: &mut ToolContext) -> Option<Pick> {
        self.poll(cx);
        let s = cx.services;
        let mut pick = None;
        egui::ScrollArea::vertical()
            .id_salt(ui.id().with("git-tree"))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                section(ui, theme, &s.t("git-tree-workspace"), None);
                for (view, icon, key) in [
                    (View::Status, ph::FILES, "git-tree-file-status"),
                    (
                        View::History,
                        ph::CLOCK_COUNTER_CLOCKWISE,
                        "git-tree-history",
                    ),
                ] {
                    if row(ui, theme, self.view == view, icon, &s.t(key), None).clicked() {
                        self.view = view;
                        pick = Some(Pick::View(view));
                    }
                }
                let n = self.refs.branches.iter().filter(|b| !b.remote).count();
                if section(
                    ui,
                    theme,
                    &s.t("git-tree-branches"),
                    Some((n, &mut self.open_branches)),
                ) {
                    for b in self.refs.branches.iter().filter(|b| !b.remote) {
                        let mut label = b.name.clone();
                        if b.ahead > 0 {
                            label.push_str(&format!(" \u{2191}{}", b.ahead));
                        }
                        if b.behind > 0 {
                            label.push_str(&format!(" \u{2193}{}", b.behind));
                        }
                        let icon = if b.current { ph::CHECK } else { ph::GIT_BRANCH };
                        row(ui, theme, b.current, icon, &label, b.upstream.as_deref())
                            .on_hover_text(&b.subject);
                    }
                }
                let n = self.refs.remotes.len();
                if section(
                    ui,
                    theme,
                    &s.t("git-tree-remotes"),
                    Some((n, &mut self.open_remotes)),
                ) {
                    for r in &self.refs.remotes {
                        let open = self.open_remote.contains(&r.name);
                        let icon = if open {
                            ph::CARET_DOWN
                        } else {
                            ph::CARET_RIGHT
                        };
                        if row(ui, theme, false, icon, &r.name, Some(&r.url)).clicked() {
                            if open {
                                self.open_remote.retain(|n| n != &r.name);
                            } else {
                                self.open_remote.push(r.name.clone());
                            }
                        }
                        if open {
                            let prefix = format!("{}/", r.name);
                            for b in self
                                .refs
                                .branches
                                .iter()
                                .filter(|b| b.remote && b.name.starts_with(&prefix))
                            {
                                ui.horizontal(|ui| {
                                    ui.add_space(16.0);
                                    row(ui, theme, false, ph::CLOUD, &b.name[prefix.len()..], None)
                                        .on_hover_text(&b.subject);
                                });
                            }
                        }
                    }
                }
                let n = self.refs.tags.len();
                if section(
                    ui,
                    theme,
                    &s.t("git-tree-tags"),
                    Some((n, &mut self.open_tags)),
                ) {
                    for t in &self.refs.tags {
                        row(ui, theme, false, ph::TAG, t, None);
                    }
                }
                let n = self.refs.stashes.len();
                if section(
                    ui,
                    theme,
                    &s.t("git-tree-stashes"),
                    Some((n, &mut self.open_stashes)),
                ) {
                    for st in &self.refs.stashes {
                        row(ui, theme, false, ph::ARCHIVE, &st.message, Some(&st.id));
                    }
                }
            });
        pick
    }
}

/// A section heading; with a count and an open flag it folds. Returns
/// whether its rows are to be drawn.
fn section(
    ui: &mut egui::Ui,
    theme: &Theme,
    title: &str,
    fold: Option<(usize, &mut bool)>,
) -> bool {
    let (rect, resp) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 26.0), egui::Sense::click());
    ui.painter().rect_filled(rect, 0.0, theme.surface);
    let mut x = rect.left() + 8.0;
    let open = match fold {
        Some((count, open)) => {
            if resp.clicked() {
                *open = !*open;
            }
            let caret = if *open {
                ph::CARET_DOWN
            } else {
                ph::CARET_RIGHT
            };
            ui.painter().text(
                egui::pos2(x, rect.center().y),
                egui::Align2::LEFT_CENTER,
                caret,
                egui::FontId::proportional(12.0),
                theme.text_muted,
            );
            x += 16.0;
            ui.painter().text(
                egui::pos2(rect.right() - 8.0, rect.center().y),
                egui::Align2::RIGHT_CENTER,
                count.to_string(),
                egui::FontId::proportional(12.0),
                theme.text_muted,
            );
            *open
        }
        None => true,
    };
    ui.painter().text(
        egui::pos2(x, rect.center().y),
        egui::Align2::LEFT_CENTER,
        title,
        egui::FontId::proportional(13.0),
        theme.text,
    );
    open
}

fn row(
    ui: &mut egui::Ui,
    theme: &Theme,
    selected: bool,
    icon: &str,
    label: &str,
    detail: Option<&str>,
) -> egui::Response {
    let (rect, resp) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 24.0), egui::Sense::click());
    let fill = if selected {
        theme.accent.linear_multiply(0.30)
    } else if resp.hovered() {
        theme.surface_hover
    } else {
        egui::Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, 3.0, fill);
    let p = ui.painter().with_clip_rect(rect);
    p.text(
        egui::pos2(rect.left() + 12.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        icon,
        egui::FontId::proportional(13.0),
        theme.text_muted,
    );
    let text = match detail {
        Some(d) => format!("{label}  {d}"),
        None => label.to_string(),
    };
    let galley = p.layout_no_wrap(text, egui::FontId::proportional(13.0), theme.text);
    p.galley(
        egui::pos2(rect.left() + 32.0, rect.center().y - galley.size().y / 2.0),
        galley,
        theme.text,
    );
    resp
}
