//! The **Git activity**: repositories managed SourceTree-style, apart
//! from whatever the editor has open (docs/tools/git-activity.md).
//!
//! The activity's home tab is a dashboard; every other tab is one saved
//! repository (`hs_core::git_repos`), drawn by [`pane::RepoPane`]. The
//! sidebar is the saved list, in the user's order. Everything that
//! touches git goes through `hs-git` on the blocking pool; the UI thread
//! only ever reads the last answer.

pub mod auth;
mod blame;
mod conflict;
mod diffview;
mod files;
mod history;
pub mod host;
pub mod hosts;
mod pane;
mod tree;
mod verbs;

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
    /// The uploaded icon's image URI, registered once per change of entry
    /// the first time the tab is drawn; not saved.
    #[serde(skip)]
    image: Option<String>,
    /// The uploaded icon's file, read when the tab is made or shown, so a
    /// tab restored at start-up and not yet drawn still names its picture
    /// (the Git sidebar registers it); not saved.
    #[serde(skip)]
    image_file: Option<PathBuf>,
    /// The repository id and icon generation (`git_repos::icon_generation`)
    /// `image_file` was looked up for; the disk is read again only when
    /// this changes, never per frame. Not saved.
    #[serde(skip)]
    picture_key: Option<(String, u64)>,
    /// `image` still has to be registered with the context (the entry
    /// changed since the tab was last drawn). Not saved.
    #[serde(skip)]
    image_pending: bool,
    /// The tab's picture URI, worked out when the entry changes so
    /// `tab_icon_uri` never touches the disk. Not saved.
    #[serde(skip)]
    picture: Option<String>,
    /// A host tab (FEAT-0100): the Git credential it shows, its inner
    /// repository tabs and which of them is in front.
    host_id: String,
    host_tabs: Vec<host::RepoTabState>,
    host_active: usize,
    /// The parent folder the host's last clone went into (CR-0053).
    host_folder: String,
    /// What this tab is taken by - `repo:<id>` or `path:<folder>` - so the
    /// shell never binds a host to a tab that already shows a repository
    /// (BUG-0044). Derived from the fields above, never saved.
    #[serde(skip)]
    bound: String,
}

/// The key a repository tab is bound by: the saved entry's id, else the
/// folder it was opened from.
fn bound_key(repo_id: &str, path: Option<&std::path::Path>) -> String {
    if !repo_id.is_empty() {
        format!("repo:{repo_id}")
    } else if let Some(p) = path {
        format!("path:{}", p.display())
    } else {
        String::new()
    }
}

/// What the shell's one-tab-per-connection rule sees: the host of a host
/// tab, the repository of a repository tab, nothing for an empty tab.
fn connection_of(state: &State) -> Option<&str> {
    if !state.host_id.is_empty() {
        return Some(state.host_id.as_str());
    }
    (!state.bound.is_empty()).then_some(state.bound.as_str())
}

/// What the dashboard knows about one saved repository: whether its
/// work tree is dirty and how far ahead / behind its upstream it is.
/// Probed on the blocking pool when the dashboard is on screen, kept
/// for a while, never on the UI thread.
#[derive(Clone, Default)]
struct RepoFigures {
    dirty: bool,
    ahead: u32,
    behind: u32,
}

pub struct GitTool {
    id: egui::Id,
    state: State,
    /// Dashboard figures by entry id, with when they were read.
    figures: std::collections::HashMap<String, (f64, RepoFigures)>,
    figures_slot: std::sync::Arc<parking_lot::Mutex<Vec<(String, RepoFigures)>>>,
    probing: std::collections::HashSet<String>,
    /// `Some(title)` while this is the activity's home tab.
    home_title: Option<String>,
    pane: pane::RepoPane,
    /// The host this tab shows, when it is a host tab (FEAT-0100).
    host: Option<host::HostView>,
}

impl GitTool {
    fn new(services: &Services, state: State) -> Self {
        let mut t = Self {
            id: egui::Id::new(("git-tool", hs_core::new_id())),
            state,
            figures: Default::default(),
            figures_slot: Default::default(),
            probing: Default::default(),
            home_title: None,
            pane: pane::RepoPane::default(),
            host: None,
        };
        if let Some(p) = t.state.path.clone() {
            t.pane.set_root(services, p);
        }
        t.refresh_from_entry(services);
        t.rebind();
        if !t.state.host_id.is_empty() {
            let id = t.state.host_id.clone();
            let tabs = t.state.host_tabs.clone();
            let folder = t.state.host_folder.clone();
            if t.become_host(services, &id) {
                if let Some(h) = t.host.as_mut() {
                    for saved in &tabs {
                        h.restore_tab(services, saved);
                    }
                    h.active = t.state.host_active.min(h.tabs.len().saturating_sub(1));
                    h.clone_parent = (!folder.is_empty()).then(|| PathBuf::from(&folder));
                }
            } else {
                // The credential is gone: the tab stays, and says so.
                t.state.host_tabs.clear();
            }
        }
        t
    }

    /// Make this tab the host tab of credential `id` (FEAT-0100). `false`
    /// when no such credential exists.
    fn become_host(&mut self, s: &Services, id: &str) -> bool {
        let Some(cred) = s.git_credentials.read().by_id(id).cloned() else {
            return false;
        };
        if self.host.as_ref().is_some_and(|h| h.cred.id == id) {
            if let Some(h) = self.host.as_mut() {
                h.cred = cred;
            }
            return true;
        }
        self.state = State {
            host_id: id.to_string(),
            name: cred.label(),
            ..Default::default()
        };
        self.pane = pane::RepoPane::default();
        self.host = Some(host::HostView::new(cred));
        true
    }

    fn is_host(&self) -> bool {
        !self.state.host_id.is_empty()
    }

    /// Recompute what this tab is bound by (BUG-0044).
    fn rebind(&mut self) {
        self.state.bound = bound_key(&self.state.repo_id, self.state.path.as_deref());
    }

    /// The host tab's own layout: the repository column, a rule, then the
    /// inner strip and the pane of the repository in front.
    fn host_ui(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        let theme = Self::theme(cx.services);
        let Some(h) = self.host.as_mut() else {
            ui.add_space(24.0);
            ui.horizontal(|ui| {
                ui.add_space(24.0);
                hs_ui::form::note(ui, &theme, ph::WARNING, &cx.services.t("git-host-gone"));
            });
            return;
        };
        h.poll(cx);
        let full = ui.available_size();
        // A real width, not `available_width`: inside a horizontal layout
        // that is unbounded, and the column would grow every frame.
        let left = (full.x * 0.22).clamp(220.0, 320.0);
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(left, full.y),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_min_width(left);
                    ui.set_max_width(left);
                    ui.add_space(hs_ui::form::PANE_GAP);
                    h.column(ui, &theme, cx);
                },
            );
            ui.separator();
            let right = (full.x - left - 16.0).max(200.0);
            ui.allocate_ui_with_layout(
                egui::vec2(right, full.y),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_min_width(right);
                    ui.set_max_width(right);
                    h.body(ui, &theme, cx);
                },
            );
        });
        self.state.host_tabs = h.saved_tabs();
        self.state.host_active = h.active;
        self.state.host_folder = h
            .clone_parent
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        self.state.name = h.cred.label();
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
        self.rebind();
        self.state.name = entry.label();
        self.state.language = entry.language.clone();
        self.state.icon = entry.icon.clone();
        self.state.picture_key = None;
        self.refresh_picture(s);
        self.pane.set_root(s, entry.path.clone());
        self.pane.set_credential(&entry.credential);
    }

    /// Take the name, icon, language and picture file from the saved
    /// entry, which may have changed since the tab was saved; a tab wears
    /// what the list says now, drawn yet or not.
    fn refresh_from_entry(&mut self, s: &Services) {
        if self.state.repo_id.is_empty() {
            return;
        }
        let repos = s.git_repos.read();
        if let Some(e) = repos.by_id(&self.state.repo_id) {
            self.state.name = e.label();
            self.state.icon = e.icon.clone();
            if !e.language.is_empty() {
                self.state.language = e.language.clone();
            }
        }
        drop(repos);
        self.refresh_picture(s);
    }

    /// Look the uploaded picture up again, only when the entry or its icon
    /// changed since the last look (`picture_key`); per frame this is a
    /// comparison, never disk I/O.
    fn refresh_picture(&mut self, s: &Services) {
        let key = (
            self.state.repo_id.clone(),
            hs_core::git_repos::icon_generation(),
        );
        if self.state.picture_key.as_ref() == Some(&key) {
            return;
        }
        self.state.image = None;
        self.state.image_file = if key.0.is_empty() {
            None
        } else {
            s.git_repos.read().icon_file(&key.0)
        };
        self.state.picture = repo_picture(&self.state);
        self.state.image_pending = self.state.image_file.is_some();
        self.state.picture_key = Some(key);
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

    /// Read the dashboard's figures for every saved repository whose
    /// answer is older than a minute; each probe is one `git status`.
    fn probe_figures(&mut self, s: &Services, now: f64) {
        const FRESH: f64 = 60.0;
        for (id, f) in self.figures_slot.lock().drain(..) {
            self.probing.remove(&id);
            self.figures.insert(id, (now, f));
        }
        let entries: Vec<(String, PathBuf)> = s
            .git_repos
            .read()
            .all()
            .iter()
            .map(|r| (r.id.clone(), r.path.clone()))
            .collect();
        for (id, path) in entries {
            let fresh = self
                .figures
                .get(&id)
                .is_some_and(|(at, _)| now - at < FRESH);
            if fresh || self.probing.contains(&id) || !path.join(".git").exists() {
                continue;
            }
            self.probing.insert(id.clone());
            let slot = self.figures_slot.clone();
            s.runtime.spawn_blocking(move || {
                let f = hs_git::status(&path)
                    .map(|st| RepoFigures {
                        dirty: !st.files.is_empty(),
                        ahead: st.ahead,
                        behind: st.behind,
                    })
                    .unwrap_or_default();
                slot.lock().push((id, f));
            });
        }
    }

    fn dashboard(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        let s = cx.services;
        let theme = Self::theme(s);
        let tabs = s.open_tabs.of("git");
        let now = ui.input(|i| i.time);
        self.probe_figures(s, now);
        if !self.probing.is_empty() {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(300));
        }
        let dirty = self.figures.values().filter(|(_, f)| f.dirty).count();
        let (ahead, behind) = self
            .figures
            .values()
            .fold((0u32, 0u32), |(a, b), (_, f)| (a + f.ahead, b + f.behind));
        let repos: Vec<hs_ui::dashboard::Entry> = s
            .git_repos
            .read()
            .all()
            .iter()
            .map(|r| {
                // The picture, else the language's logo in its colour, as
                // the sidebar row and the tab (CR-0047).
                let mark = repo_conn_icon(&r.icon, &r.language);
                let image = entry_image(ui.ctx(), s, &r.id);
                hs_ui::dashboard::Entry {
                    id: r.id.clone(),
                    name: r.label(),
                    detail: r.path.display().to_string(),
                    icon: mark.glyph.to_string(),
                    tint: image.is_none().then(|| mark.brand()).flatten(),
                    image: image.or_else(|| mark.uri()),
                }
            })
            .collect();
        // The hosts, as the sidebar's Hosts section draws them (CR-0056).
        let hosts: Vec<hs_ui::dashboard::Entry> = s
            .git_credentials
            .read()
            .all()
            .iter()
            .map(|c| {
                let mark = host_conn_icon(c.kind);
                hs_ui::dashboard::Entry {
                    id: c.id.clone(),
                    name: c.label(),
                    detail: c.host(),
                    icon: mark.glyph.to_string(),
                    tint: mark.uri().is_none().then(|| mark.brand()).flatten(),
                    image: mark.uri(),
                }
            })
            .collect();
        let credentials = hosts.len();
        let hosts_heading = s.t("git-dash-hosts");
        let hosts_empty = s.t("git-dash-no-hosts");
        let remove_label = format!("{} {}", hs_ui::icons::TRASH, s.t("git-repo-remove"));
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
            extra: Some(hs_ui::dashboard::Extra {
                heading: &hosts_heading,
                empty: &hosts_empty,
                entries: &hosts,
            }),
            // The sidebar row's Remove From List, on the card too.
            entry_menu: Some(&remove_label),
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
                    icon: ph::PENCIL_SIMPLE,
                    value: &dirty.to_string(),
                    label: &s.t("git-dash-dirty"),
                },
                hs_ui::dashboard::Stat {
                    icon: ph::ARROWS_DOWN_UP,
                    value: &format!("\u{2191}{ahead} \u{2193}{behind}"),
                    label: &s.t("git-dash-ahead-behind"),
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
                // The sidebar's Add Host, here too (CR-0056).
                format!("{} {}", ph::PLUS, s.t("git-add-host")),
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
            Some(hs_ui::dashboard::Pick::Action(1 | 3)) => cx.actions.push(Action::RunCommand {
                id: "settings:git:credentials".into(),
            }),
            Some(hs_ui::dashboard::Pick::Action(2)) => cx.actions.push(Action::RunCommand {
                id: "settings:git:repo:new:remote".into(),
            }),
            Some(hs_ui::dashboard::Pick::Action(_)) => cx.actions.push(Action::RunCommand {
                id: "settings:git:repos".into(),
            }),
            // A host card opens the host tab, one per host (FEAT-0100).
            Some(hs_ui::dashboard::Pick::OpenExtra(id)) => {
                cx.open("git", Some(OpenRequest::Connection { id }))
            }
            Some(hs_ui::dashboard::Pick::Open(id)) => open_saved(cx, &id),
            // The same confirmed removal the sidebar row's menu asks for.
            Some(hs_ui::dashboard::Pick::Menu(id)) => cx.actions.push(Action::RunCommand {
                id: format!("git:remove:{id}"),
            }),
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
    /// The chosen glyph key and the detected language (CR-0047).
    icon: (String, String),
    /// The active tab shows this repository.
    here: bool,
    image: Option<String>,
}

/// The uploaded icon of saved entry `id` as an image URI, if it has one.
/// The repository's uploaded picture: the URI registered while the tab
/// was drawn, else the same URI named from the file without a context,
/// so a restored tab that was never drawn (the Open Here card, its strip
/// tab) shows what the sidebar shows.
fn repo_picture(state: &State) -> Option<String> {
    state.image.clone().or_else(|| {
        state
            .image_file
            .as_deref()
            .map(hs_ui::icons::image_uri_name)
    })
}

fn entry_image(ctx: &egui::Context, s: &Services, id: &str) -> Option<String> {
    if id.is_empty() {
        return None;
    }
    let file = s.git_repos.read().icon_file(id)?;
    hs_ui::icons::image_uri(ctx, &file)
}

/// The forge's mark for a host: its logo where one is vendored, the
/// branch glyph otherwise. The sidebar's Hosts section, the host tab and
/// Settings → Git's credential list all draw it (CR-0054).
pub fn host_conn_icon(kind: hs_core::git_hosts::HostKind) -> hs_ui::icons::ConnIcon {
    // Every forge its own mark; Gitea no longer borrows Forgejo's (CR-0079).
    hs_ui::icons::git_host_icon(kind)
}

/// What a repository wears in the sidebar, on its tab and in its header
/// band when it has no uploaded picture (CR-0047): the chosen glyph, else
/// the detected language's logo in its own colour - the same mark the
/// Editor gives a project of that language. Settings → Git's repository
/// list draws the same (CR-0054).
pub fn repo_conn_icon(icon: &str, language: &str) -> hs_ui::icons::ConnIcon {
    if let Some(g) = hs_ui::icons::named(icon) {
        hs_ui::icons::ConnIcon::glyph(g)
    } else if !language.is_empty() {
        hs_ui::icons::language_conn_icon(language)
    } else {
        hs_ui::icons::ConnIcon::glyph(ph::GIT_BRANCH)
    }
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
        if self.is_host() {
            return Cow::Borrowed(&self.state.name);
        }
        if self.state.name.is_empty() {
            Cow::Borrowed("Git")
        } else {
            Cow::Borrowed(&self.state.name)
        }
    }

    fn subtitle(&self) -> Option<String> {
        if let Some(h) = &self.host {
            return Some(h.cred.host());
        }
        self.root().map(|r| r.display().to_string())
    }

    fn tab_icon(&self) -> Option<&'static str> {
        if self.is_home() {
            return Some(ph::GIT_BRANCH);
        }
        if let Some(h) = &self.host {
            return Some(host_conn_icon(h.cred.kind).glyph);
        }
        Some(self.icon())
    }

    /// The uploaded icon, when the repository has one, else the language's
    /// logo (CR-0047); a host tab wears the forge's. Read once per change
    /// of entry rather than per frame (`state.image`).
    fn tab_icon_uri(&self) -> Option<String> {
        if self.is_home() {
            return None;
        }
        if let Some(h) = &self.host {
            return host_conn_icon(h.cred.kind).uri();
        }
        self.state
            .picture
            .clone()
            .or_else(|| repo_conn_icon(&self.state.icon, &self.state.language).uri())
    }

    /// The language's colour for the glyph, where the language has one and
    /// there is no picture to draw (CR-0047).
    fn tab_icon_colour(&self) -> Option<egui::Color32> {
        if self.is_home() {
            return None;
        }
        if let Some(h) = &self.host {
            return host_conn_icon(h.cred.kind).brand();
        }
        if self.state.image.is_some() || self.state.image_file.is_some() {
            return None;
        }
        repo_conn_icon(&self.state.icon, &self.state.language).brand()
    }

    /// The host credential a host tab is bound to, so opening the host
    /// again focuses this tab (FEAT-0100) - and the repository a
    /// repository tab shows, so a host never takes that tab over
    /// (BUG-0044).
    fn current_connection(&self) -> Option<&str> {
        connection_of(&self.state)
    }

    fn header(&self) -> Option<hs_core::tool::ToolHeader> {
        if self.is_home() {
            return None;
        }
        if let Some(h) = &self.host {
            return Some(
                host_conn_icon(h.cred.kind)
                    .header(h.cred.label())
                    .fact("Kind", h.cred.kind.name().to_string())
                    .fact("URL", h.cred.host())
                    .fact("User", h.cred.username.clone())
                    .fact("Open", h.tabs.len().to_string()),
            );
        }
        let root = self.root()?;
        let language = hs_core::langver::display_name(&self.state.language);
        // The repository's picture, chosen glyph or language logo in its
        // colour, as its row and tab show it (CR-0036, CR-0047).
        let header =
            repo_conn_icon(&self.state.icon, &self.state.language).header(self.title().to_string());
        let header = match &self.state.image {
            Some(uri) => header.icon_image(uri.clone()),
            None => header,
        };
        Some(
            header
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
        if let Some(h) = &self.host {
            return h.active_tab().map(|t| t.path.clone()).into_iter().collect();
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
        // A Git credential opens as a host tab (FEAT-0100); the shell has
        // already made sure there is at most one per host.
        if let OpenRequest::Connection { id } = &item {
            return self.become_host(cx.services, id);
        }
        if self.is_host() {
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
                self.rebind();
            }
        }
        true
    }

    fn draws_toolbar(&self) -> bool {
        if let Some(h) = &self.host {
            return h.active_tab().is_some_and(|t| t.path.join(".git").exists());
        }
        !self.is_home() && self.root().is_some()
    }

    /// Every verb as a palette command (owner's rule: every action has
    /// a palette command); the shortcut column is the user's to fill in
    /// Settings → Shortcuts.
    fn commands(&self) -> Vec<hs_core::tool::CommandDescriptor> {
        if self.is_home() || self.root().is_none() {
            return Vec::new();
        }
        [
            ("git.ai", "Git: Toggle AI Assistant"),
            ("git.tree", "Git: Toggle Tree Column"),
            ("git.refresh", "Git: Refresh"),
            ("git.fetch", "Git: Fetch"),
            ("git.pull", "Git: Pull"),
            ("git.push", "Git: Push"),
            ("git.status", "Git: File Status"),
            ("git.history", "Git: History"),
            ("git.files", "Git: Files"),
            ("git.files.new-file", "Git: New File"),
            ("git.files.new-folder", "Git: New Folder"),
            ("git.files.rename", "Git: Rename File"),
            ("git.files.delete", "Git: Delete File"),
            ("git.files.upload", "Git: Upload Files"),
            ("git.branch", "Git: Branch…"),
            ("git.merge", "Git: Merge…"),
            ("git.rebase", "Git: Rebase…"),
            ("git.stash", "Git: Stash…"),
            ("git.tag", "Git: Tag…"),
            ("git.flow", "Git: Git Flow…"),
            ("git.terminal", "Git: Open Terminal Here"),
        ]
        .into_iter()
        .map(|(id, title)| hs_core::tool::CommandDescriptor {
            id: id.into(),
            title: title.into(),
            shortcut_action: None,
        })
        .collect()
    }

    fn run_command(&mut self, id: &str, cx: &mut ToolContext) -> bool {
        if let Some(h) = self.host.as_mut() {
            return h.active_pane_mut().is_some_and(|p| p.run_command(id, cx));
        }
        if self.is_home() || self.root().is_none() {
            return false;
        }
        self.pane.run_command(id, cx)
    }

    fn toolbar(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        let theme = Self::theme(cx.services);
        if let Some(h) = self.host.as_mut() {
            if let Some(p) = h.active_pane_mut() {
                p.toolbar(ui, &theme, cx);
            }
            return;
        }
        if self.is_home() || self.root().is_none() {
            return;
        }
        self.pane.toolbar(ui, &theme, cx);
    }

    fn sidebar(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) -> bool {
        let s = cx.services;
        let theme = Self::theme(s);
        if hs_ui::form::sidebar_head(
            ui,
            &theme,
            &s.t("git-repos"),
            &[hs_ui::form::SideAction {
                icon: ph::PLUS,
                label: &s.t("git-add-repo"),
                tip: "",
            }],
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
                icon: (r.icon.clone(), r.language.clone()),
                here: mine.as_deref() == Some(hs_core::projects::normalise(&r.path).as_str()),
                image: entry_image(ui.ctx(), s, &r.id),
            })
            .collect();
        // The hosts (FEAT-0100): the Git credentials themselves, so a
        // credential saved in Settings → Git is a host row at once.
        let hosts: Vec<hs_core::git_hosts::GitCredential> = s.git_credentials.read().all().to_vec();
        let host_here = self.state.host_id.clone();
        if rows.is_empty() {
            hs_ui::form::note(ui, &theme, ph::LIGHTBULB, &s.t("git-no-repos"));
        }
        let mut reorder = hs_ui::reorder::Reorder::new("git-repo-list");
        let mut open: Option<String> = None;
        let mut edit: Option<String> = None;
        let mut remove: Option<String> = None;
        let mut open_host: Option<String> = None;
        let mut add_host = false;
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
                        // The language's logo in its colour where there is
                        // no picture and no chosen glyph (CR-0047).
                        let mark = repo_conn_icon(&icon.0, &icon.1);
                        let row = match image.as_deref() {
                            Some(uri) => hs_ui::form::list_row_image(
                                ui,
                                &theme,
                                *here,
                                Some(uri),
                                mark.glyph,
                                name,
                                Some(path),
                            ),
                            None => hs_ui::form::conn_list_row(
                                ui,
                                &theme,
                                *here,
                                mark,
                                name,
                                Some(path),
                            ),
                        };
                        if row.clicked() {
                            open = Some(id.clone());
                        }
                        row.context_menu(|ui| {
                            if ui
                                .button(format!("{} {}", hs_ui::icons::EDIT, s.t("git-repo-edit")))
                                .clicked()
                            {
                                edit = Some(id.clone());
                                ui.close();
                            }
                            ui.separator();
                            if ui
                                .button(format!(
                                    "{} {}",
                                    hs_ui::icons::TRASH,
                                    s.t("git-repo-remove")
                                ))
                                .clicked()
                            {
                                remove = Some(id.clone());
                                ui.close();
                            }
                        });
                    });
                    ui.add_space(2.0);
                }
                ui.add_space(hs_ui::form::PANE_GAP);
                if hs_ui::form::sidebar_head(
                    ui,
                    &theme,
                    &s.t("git-hosts"),
                    &[hs_ui::form::SideAction {
                        icon: ph::PLUS,
                        label: &s.t("git-add-host"),
                        tip: "",
                    }],
                )
                .is_some()
                {
                    add_host = true;
                }
                if hosts.is_empty() {
                    hs_ui::form::note(ui, &theme, ph::LIGHTBULB, &s.t("git-no-hosts"));
                }
                for c in &hosts {
                    let row = hs_ui::form::conn_list_row(
                        ui,
                        &theme,
                        host_here == c.id,
                        host_conn_icon(c.kind),
                        &c.label(),
                        Some(&c.host()),
                    );
                    if row.clicked() {
                        open_host = Some(c.id.clone());
                    }
                    ui.add_space(2.0);
                }
            });
        if add_host {
            cx.actions.push(Action::RunCommand {
                id: "settings:git:credentials".into(),
            });
        }
        if let Some(id) = open_host {
            // One tab per host: the shell focuses the one that has it.
            cx.open("git", Some(OpenRequest::Connection { id }));
        }
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
        if self.is_host() {
            self.host_ui(ui, cx);
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
        self.refresh_from_entry(cx.services);
        if self.state.image_pending {
            self.state.image_pending = false;
            self.state.image = self
                .state
                .image_file
                .as_deref()
                .and_then(|f| hs_ui::icons::image_uri(ui.ctx(), f));
            self.state.picture = repo_picture(&self.state);
        }
        hs_core::langver::ensure(&cx.services.runtime, &self.state.language);
        self.pane.ui(ui, &theme, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A restored tab that was never drawn has no registered URI yet, but
    /// names its picture from the saved file, the same URI the sidebar
    /// registers; the registered one wins once drawn (Open Here card).
    #[test]
    fn a_restored_tab_names_its_picture_before_it_is_drawn() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("repo.png");
        std::fs::write(&file, b"png").unwrap();
        let mut state = State {
            image_file: Some(file.clone()),
            ..Default::default()
        };
        assert_eq!(
            repo_picture(&state),
            Some(hs_ui::icons::image_uri_name(&file))
        );
        state.image = Some("bytes://drawn".into());
        assert_eq!(repo_picture(&state).as_deref(), Some("bytes://drawn"));
        assert_eq!(repo_picture(&State::default()), None);
    }

    /// Every button carries a glyph before its label, and every text field
    /// is `form::line_edit` (CR-0063, CR-0061).
    #[test]
    fn no_label_only_button_or_bare_field_in_the_git_ui() {
        for src in [
            include_str!("conflict.rs"),
            include_str!("files.rs"),
            include_str!("history.rs"),
            include_str!("pane.rs"),
            include_str!("tree.rs"),
            include_str!("verbs.rs"),
            include_str!("host.rs"),
            include_str!("blame.rs"),
            include_str!("diffview.rs"),
        ] {
            for bad in [
                concat!("ui.button(s.", "t("),
                concat!("small_button(s.", "t("),
                concat!("Button::new(s.", "t("),
                concat!("TextEdit::", "singleline"),
                concat!(".selectable", "_label("),
                concat!(".selectable", "_value("),
            ] {
                assert!(!src.contains(bad), "found {bad}");
            }
        }
    }

    /// CR-0047: a repository with no picture and no chosen glyph wears its
    /// language's logo in the language's colour; a chosen glyph stays plain.
    #[test]
    fn a_repository_wears_its_language_unless_a_glyph_was_chosen() {
        let rust = repo_conn_icon("", "rust");
        assert!(rust.uri().is_some(), "the Rust logo");
        assert_eq!(rust.glyph, hs_ui::icons::language_icon("rust"));
        let chosen = repo_conn_icon("star", "rust");
        assert!(chosen.uri().is_none() && chosen.brand().is_none());
        let bare = repo_conn_icon("", "");
        assert_eq!(bare.glyph, ph::GIT_BRANCH);
    }

    /// BUG-0044: a repository tab is taken, so the shell's "bind a host to
    /// an unbound tab" step passes it by; an empty tab is free; a host tab
    /// answers with its host.
    #[test]
    fn a_repository_tab_counts_as_bound_and_an_empty_tab_does_not() {
        let mut st = State::default();
        assert_eq!(connection_of(&st), None);
        st.repo_id = "01REPO".into();
        st.path = Some(PathBuf::from("D:/work/app"));
        st.bound = bound_key(&st.repo_id, st.path.as_deref());
        assert_eq!(connection_of(&st), Some("repo:01REPO"));
        // Opened from a folder that is not in the list: the folder is the key.
        st.repo_id.clear();
        st.bound = bound_key(&st.repo_id, st.path.as_deref());
        assert_eq!(connection_of(&st), Some("path:D:/work/app"));
        let host = State {
            host_id: "01HOST".into(),
            ..Default::default()
        };
        assert_eq!(connection_of(&host), Some("01HOST"));
        assert!(bound_key("", None).is_empty());
    }

    /// FEAT-0100: a host tab wears its forge's logo where one is vendored.
    #[test]
    fn a_host_tab_wears_the_forges_logo() {
        use hs_core::git_hosts::HostKind;
        assert!(host_conn_icon(HostKind::GitLab).uri().is_some());
        assert!(host_conn_icon(HostKind::Forgejo).uri().is_some());
        assert_eq!(host_conn_icon(HostKind::Azure).glyph, ph::GIT_BRANCH);
        // CR-0079: GitHub, Gitea, Bitbucket and Azure DevOps too, and
        // Gitea no longer wears Forgejo's mark.
        for kind in [
            HostKind::GitHub,
            HostKind::Gitea,
            HostKind::Bitbucket,
            HostKind::Azure,
        ] {
            assert!(host_conn_icon(kind).uri().is_some(), "{kind:?}");
        }
        assert_ne!(
            host_conn_icon(HostKind::Gitea).uri(),
            host_conn_icon(HostKind::Forgejo).uri()
        );
    }
}
