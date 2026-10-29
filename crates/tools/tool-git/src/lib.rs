//! The **Git activity**: repositories managed SourceTree-style, apart
//! from whatever the editor has open (docs/tools/git-activity.md).
//!
//! The activity's home tab is a dashboard; every other tab is one saved
//! repository (`hs_core::git_repos`), drawn by [`pane::RepoPane`]. The
//! sidebar is the saved list, in the user's order. Everything that
//! touches git goes through `hs-git` on the blocking pool; the UI thread
//! only ever reads the last answer.

mod blame;
mod conflict;
mod diffview;
mod history;
mod pane;
mod tree;

use std::borrow::Cow;
use std::path::{Path, PathBuf};

use hs_core::services::Services;
use hs_core::tool::{
    Accept, Action, OpenRequest, ToastLevel, Tool, ToolCategory, ToolContext, ToolDescriptor,
    ToolFactory,
};
use hs_ui::icons::ph;
use hs_ui::theme::Theme;
use serde::{Deserialize, Serialize};

pub static DESCRIPTOR: ToolDescriptor = ToolDescriptor {
    id: "git",
    title_key: "tool-git",
    icon: ph::GIT_BRANCH,
    category: ToolCategory::Code,
    accepts: &[Accept::LocalFile],
};

pub struct GitFactory;

impl ToolFactory for GitFactory {
    fn descriptor(&self) -> &ToolDescriptor {
        &DESCRIPTOR
    }
    fn create(&self, services: &Services, restore: Option<serde_json::Value>) -> Box<dyn Tool> {
        let state = restore
            .and_then(|v| serde_json::from_value::<State>(v).ok())
            .unwrap_or_default();
        Box::new(GitTool::new(services, state))
    }
}

/// What a tab remembers across restarts: the repository it shows.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct State {
    repo_id: String,
    /// Kept beside the id so a tab survives the entry being renamed or
    /// removed from the list (the folder is still there).
    path: Option<PathBuf>,
    name: String,
    language: String,
    icon: String,
    /// The uploaded icon's image URI, refreshed from the list each frame
    /// the tab is drawn (cheap: a stat); not saved.
    #[serde(skip)]
    image: Option<String>,
}

pub struct GitTool {
    id: egui::Id,
    state: State,
    /// `Some(title)` while this is the activity's home tab.
    home_title: Option<String>,
    pane: pane::RepoPane,
}

impl GitTool {
    fn new(services: &Services, state: State) -> Self {
        let mut t = Self {
            id: egui::Id::new(("git-tool", hs_core::new_id())),
            state,
            home_title: None,
            pane: pane::RepoPane::default(),
        };
        if let Some(p) = t.state.path.clone() {
            t.pane.set_root(services, p);
        }
        t
    }

    fn theme(s: &Services) -> Theme {
        Theme::from_settings(&s.settings.read().appearance)
    }

    fn is_home(&self) -> bool {
        self.home_title.is_some()
    }

    fn root(&self) -> Option<&Path> {
        self.state.path.as_deref()
    }

    /// Show the saved repository `entry` in this tab.
    fn show_entry(&mut self, s: &Services, entry: &hs_core::git_repos::RepoEntry) {
        self.state.repo_id = entry.id.clone();
        self.state.path = Some(entry.path.clone());
        self.state.name = entry.label();
        self.state.language = entry.language.clone();
        self.state.icon = entry.icon.clone();
        self.state.image = None;
        self.pane.set_root(s, entry.path.clone());
    }

    /// The glyph for this repository: the chosen icon, else its
    /// language's, else git's own branch mark.
    fn icon(&self) -> &'static str {
        if let Some(g) = hs_ui::icons::named(&self.state.icon) {
            return g;
        }
        if !self.state.language.is_empty() {
            return hs_ui::icons::language_icon(&self.state.language);
        }
        ph::GIT_BRANCH
    }

    fn dashboard(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        let s = cx.services;
        let theme = Self::theme(s);
        let tabs = s.open_tabs.of("git");
        let repos: Vec<hs_ui::dashboard::Entry> = s
            .git_repos
            .read()
            .all()
            .iter()
            .map(|r| hs_ui::dashboard::Entry {
                id: r.id.clone(),
                name: r.label(),
                detail: r.path.display().to_string(),
                icon: entry_icon(r).to_string(),
                image: entry_image(ui.ctx(), s, &r.id),
            })
            .collect();
        let credentials = s.git_credentials.read().all().len();
        let labels = hs_ui::dashboard::DashboardLabels {
            open_heading: &s.t("dash-open"),
            nothing_open: &s.t("dash-nothing-open"),
            no_preview: &s.t("dash-no-preview"),
        };
        let page = hs_ui::dashboard::Page {
            icon: DESCRIPTOR.icon,
            title: &s.t("tool-git"),
            subtitle: &s.t("git-dash-sub"),
            saved: &s.t("git-dash-saved"),
            empty: &s.t("git-no-repos"),
            start_here: Some(&s.t("dash-start-here")),
            salt: "git-dashboard",
            tabs: &tabs,
            labels: &labels,
        };
        let pick = hs_ui::dashboard::saved_dashboard(
            ui,
            &theme,
            &page,
            &[
                hs_ui::dashboard::Stat {
                    icon: ph::GIT_BRANCH,
                    value: &repos.len().to_string(),
                    label: &s.t("git-dash-repos"),
                },
                hs_ui::dashboard::Stat {
                    icon: ph::KEY,
                    value: &credentials.to_string(),
                    label: &s.t("git-dash-credentials"),
                },
                hs_ui::dashboard::Stat {
                    icon: ph::COLUMNS,
                    value: &tabs.len().to_string(),
                    label: &s.t("dash-open-tabs"),
                },
            ],
            &[
                format!("{} {}", ph::PLUS, s.t("git-add-repo")),
                format!("{} {}", ph::CLOUD_ARROW_DOWN, s.t("git-clone-from-host")),
                format!("{} {}", ph::KEY, s.t("git-credentials")),
                format!("{} {}", ph::GEAR, s.t("git-manage-repos")),
            ],
            &repos,
        );
        match pick {
            Some(hs_ui::dashboard::Pick::Action(0)) => cx.actions.push(Action::RunCommand {
                id: "settings:git:repo:new".into(),
            }),
            Some(hs_ui::dashboard::Pick::Action(1)) => cx.actions.push(Action::RunCommand {
                id: "settings:git:repo:new:remote".into(),
            }),
            Some(hs_ui::dashboard::Pick::Action(2)) => cx.actions.push(Action::RunCommand {
                id: "settings:git:credentials".into(),
            }),
            Some(hs_ui::dashboard::Pick::Action(_)) => cx.actions.push(Action::RunCommand {
                id: "settings:git".into(),
            }),
            Some(hs_ui::dashboard::Pick::Open(id)) => open_saved(cx, &id),
            Some(hs_ui::dashboard::Pick::Focus(tab_id)) => {
                cx.actions.push(Action::FocusTab { tab_id })
            }
            None => {}
        }
    }
}

/// One sidebar row, read from the store once per frame.
struct SidebarRow {
    id: String,
    name: String,
    path: String,
    icon: &'static str,
    /// The active tab shows this repository.
    here: bool,
    image: Option<String>,
}

/// The uploaded icon of saved entry `id` as an image URI, if it has one.
fn entry_image(ctx: &egui::Context, s: &Services, id: &str) -> Option<String> {
    if id.is_empty() {
        return None;
    }
    let file = s.git_repos.read().icon_file(id)?;
    hs_ui::icons::image_uri(ctx, &file)
}

/// The glyph a saved entry draws in lists.
fn entry_icon(r: &hs_core::git_repos::RepoEntry) -> &'static str {
    if let Some(g) = hs_ui::icons::named(&r.icon) {
        return g;
    }
    if !r.language.is_empty() {
        return hs_ui::icons::language_icon(&r.language);
    }
    ph::GIT_BRANCH
}

/// Open the saved repository `id` in its tab, or focus the tab that has
/// it: the shell matches the folder against `uri_affinity`.
fn open_saved(cx: &mut ToolContext, id: &str) {
    let path = cx
        .services
        .git_repos
        .read()
        .by_id(id)
        .map(|r| r.path.clone());
    if let Some(path) = path {
        cx.open(
            "git",
            Some(OpenRequest::File {
                uri: path.to_string_lossy().to_string(),
                line: None,
            }),
        );
    }
}

impl Tool for GitTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &DESCRIPTOR
    }

    fn wants_home_tab(&self) -> bool {
        true
    }
    fn home_is_dashboard(&self) -> bool {
        true
    }
    fn wants_dashboard_home(&self) -> bool {
        true
    }
    fn set_home_tab(&mut self, home: bool, services: &Services) {
        self.home_title = home.then(|| services.t("tool-git"));
    }

    fn title(&self) -> Cow<'_, str> {
        if let Some(t) = &self.home_title {
            return Cow::Borrowed(t);
        }
        if self.state.name.is_empty() {
            Cow::Borrowed("Git")
        } else {
            Cow::Borrowed(&self.state.name)
        }
    }

    fn subtitle(&self) -> Option<String> {
        self.root().map(|r| r.display().to_string())
    }

    fn tab_icon(&self) -> Option<&'static str> {
        if self.is_home() {
            return Some(ph::GIT_BRANCH);
        }
        Some(self.icon())
    }

    /// The uploaded icon, when the repository has one; read once per
    /// change of entry rather than per frame (`state.image`).
    fn tab_icon_uri(&self) -> Option<String> {
        if self.is_home() {
            return None;
        }
        self.state.image.clone()
    }

    fn header(&self) -> Option<hs_core::tool::ToolHeader> {
        if self.is_home() {
            return None;
        }
        let root = self.root()?;
        let language = hs_core::langver::display_name(&self.state.language);
        Some(
            hs_core::tool::ToolHeader::new(self.icon(), self.title().to_string())
                .fact("Language", language)
                .fact("Version", hs_core::langver::labelled(&self.state.language))
                .fact("URL", self.pane.remote_url())
                .fact("Path", root.display().to_string()),
        )
    }

    /// The dashboard has no folder; a repository tab has exactly one.
    fn project_roots(&self) -> Vec<PathBuf> {
        if self.is_home() {
            return Vec::new();
        }
        self.root().map(|r| r.to_path_buf()).into_iter().collect()
    }

    /// A request for the folder this tab shows is answered by this tab.
    fn uri_affinity(&self, uri: &str) -> Option<u8> {
        if self.is_home() {
            return None;
        }
        let root = self.root()?;
        let want =
            hs_core::projects::normalise(Path::new(uri.strip_prefix("file://").unwrap_or(uri)));
        (hs_core::projects::normalise(root) == want).then_some(0)
    }

    fn save_state(&self) -> serde_json::Value {
        serde_json::to_value(&self.state).unwrap_or_default()
    }

    fn open(&mut self, item: OpenRequest, cx: &mut ToolContext) -> bool {
        // The dashboard is the dashboard; a request opens a working tab
        // beside it (the shell knows to do that).
        if self.is_home() {
            return false;
        }
        let OpenRequest::File { uri, .. } = item else {
            return false;
        };
        let path = PathBuf::from(uri.strip_prefix("file://").unwrap_or(&uri));
        let saved = cx.services.git_repos.read().by_path(&path).cloned();
        match saved {
            Some(entry) => self.show_entry(cx.services, &entry),
            None => {
                // A folder that is a repository but is not in the list
                // yet (dropped on the activity, say) is shown all the
                // same; the list is a convenience, not a gate.
                if !path.join(".git").exists() {
                    return false;
                }
                let entry = hs_core::git_repos::RepoEntry::for_path(path);
                self.show_entry(cx.services, &entry);
                self.state.repo_id.clear();
            }
        }
        true
    }

    fn draws_toolbar(&self) -> bool {
        !self.is_home() && self.root().is_some()
    }

    fn toolbar(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        if self.is_home() || self.root().is_none() {
            return;
        }
        let theme = Self::theme(cx.services);
        self.pane.toolbar(ui, &theme, cx);
    }

    fn sidebar(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) -> bool {
        let s = cx.services;
        let theme = Self::theme(s);
        if hs_ui::form::sidebar_header(
            ui,
            &theme,
            &s.t("git-repos"),
            &[(ph::PLUS, &s.t("git-add-repo"))],
        )
        .is_some()
        {
            // One place manages a repository, as one place manages a
            // project: Settings -> Git with the Add Repository form open.
            cx.actions.push(Action::RunCommand {
                id: "settings:git:repo:new".into(),
            });
        }
        let mine = self.root().map(hs_core::projects::normalise);
        let rows: Vec<SidebarRow> = s
            .git_repos
            .read()
            .all()
            .iter()
            .map(|r| SidebarRow {
                id: r.id.clone(),
                name: r.label(),
                path: r.path.display().to_string(),
                icon: entry_icon(r),
                here: mine.as_deref() == Some(hs_core::projects::normalise(&r.path).as_str()),
                image: entry_image(ui.ctx(), s, &r.id),
            })
            .collect();
        if rows.is_empty() {
            hs_ui::form::note(ui, &theme, ph::LIGHTBULB, &s.t("git-no-repos"));
            return true;
        }
        let mut reorder = hs_ui::reorder::Reorder::new("git-repo-list");
        let mut open: Option<String> = None;
        let mut edit: Option<String> = None;
        let mut remove: Option<String> = None;
        egui::ScrollArea::vertical()
            .id_salt(self.id.with("repos"))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for (i, r) in rows.iter().enumerate() {
                    let SidebarRow {
                        id,
                        name,
                        path,
                        icon,
                        here,
                        image,
                    } = r;
                    reorder.row(ui, i, |ui| {
                        let row = hs_ui::form::list_row_image(
                            ui,
                            &theme,
                            *here,
                            image.as_deref(),
                            icon,
                            name,
                            Some(path),
                        );
                        if row.clicked() {
                            open = Some(id.clone());
                        }
                        row.context_menu(|ui| {
                            if ui.button(s.t("git-repo-edit")).clicked() {
                                edit = Some(id.clone());
                                ui.close();
                            }
                            ui.separator();
                            if ui.button(s.t("git-repo-remove")).clicked() {
                                remove = Some(id.clone());
                                ui.close();
                            }
                        });
                    });
                    ui.add_space(2.0);
                }
            });
        if let Some((from, to)) = reorder.finish(ui, theme.accent)
            && let Err(e) = s.git_repos.write().reorder(from, to)
        {
            cx.toast(ToastLevel::Error, e.to_string());
        }
        if let Some(id) = open {
            open_saved(cx, &id);
        }
        if let Some(id) = edit {
            cx.actions.push(Action::RunCommand {
                id: format!("settings:git:repo:{id}"),
            });
        }
        if let Some(id) = remove {
            cx.actions.push(Action::RunCommand {
                id: format!("git:remove:{id}"),
            });
        }
        true
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        if self.is_home() {
            self.dashboard(ui, cx);
            return;
        }
        let theme = Self::theme(cx.services);
        if self.root().is_none() {
            ui.add_space(24.0);
            ui.horizontal(|ui| {
                ui.add_space(24.0);
                hs_ui::form::note(ui, &theme, ph::LIGHTBULB, &cx.services.t("git-pick-repo"));
            });
            return;
        }
        // The list may have renamed or re-iconed this entry since the tab
        // was saved; a tab wears what the list says now.
        if !self.state.repo_id.is_empty()
            && let Some(e) = cx.services.git_repos.read().by_id(&self.state.repo_id)
        {
            self.state.name = e.label();
            self.state.icon = e.icon.clone();
            if !e.language.is_empty() {
                self.state.language = e.language.clone();
            }
        }
        if !self.state.repo_id.is_empty() {
            self.state.image = entry_image(ui.ctx(), cx.services, &self.state.repo_id);
        }
        hs_core::langver::ensure(&cx.services.runtime, &self.state.language);
        self.pane.ui(ui, &theme, cx);
    }
}
