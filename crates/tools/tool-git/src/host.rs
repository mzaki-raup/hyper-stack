//! A **host tab** (docs/tools/git-activity.md §4b, FEAT-0100): one Git
//! credential, the repositories it can see in a column on the left, and
//! an inner strip of repository tabs on the right, each drawn by
//! [`RepoPane`]. A repository opened from the column is cloned on first
//! open into the clone folder and recorded in the workspace's repository
//! list, so the plain sidebar and the dashboard see it too and the next
//! open is instant. One tab per host and one inner tab per repository.
//!
//! Everything that talks to the host or to git runs on the runtime or the
//! blocking pool; the UI thread only reads the last answer out of a slot.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use hs_core::git_hosts::GitCredential;
use hs_core::git_repos::{RepoEntry, Source};
use hs_core::services::Services;
use hs_core::tool::{ToastLevel, ToolContext};
use hs_ui::form;
use hs_ui::icons::ph;
use hs_ui::theme::Theme;
use hs_ui::tip::TipExt;
use parking_lot::Mutex;

use crate::hosts::Project;
use crate::pane::RepoPane;

type Slot<T> = Arc<Mutex<Option<T>>>;

/// A clone on its way: the project, where it lands, the outcome.
type CloneJob = (Project, PathBuf, Slot<Result<(), String>>);

/// Inner tab identities, unique for the life of the process.
static NEXT_TAB: AtomicU64 = AtomicU64::new(1);

/// One repository open inside a host tab.
pub struct RepoTab {
    pub uid: u64,
    /// `owner/name`, as the host names it.
    pub project: String,
    pub name: String,
    pub path: PathBuf,
    pub pane: RepoPane,
}

/// What a host tab remembers of an inner tab across restarts.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct RepoTabState {
    pub project: String,
    pub name: String,
    pub path: PathBuf,
}

pub struct HostView {
    pub cred: GitCredential,
    projects: Option<Result<Vec<Project>, String>>,
    slot: Slot<Result<Vec<Project>, String>>,
    busy: bool,
    filter: String,
    /// A clone on its way.
    cloning: Option<CloneJob>,
    pub tabs: Vec<RepoTab>,
    pub active: usize,
    /// The parent folder the last clone from this host went into, offered
    /// again next time (CR-0053).
    pub clone_parent: Option<PathBuf>,
}

impl HostView {
    pub fn new(cred: GitCredential) -> Self {
        Self {
            cred,
            projects: None,
            slot: Default::default(),
            busy: false,
            filter: String::new(),
            cloning: None,
            tabs: Vec::new(),
            active: 0,
            clone_parent: None,
        }
    }

    /// The inner tabs as the layout saves them.
    pub fn saved_tabs(&self) -> Vec<RepoTabState> {
        self.tabs
            .iter()
            .map(|t| RepoTabState {
                project: t.project.clone(),
                name: t.name.clone(),
                path: t.path.clone(),
            })
            .collect()
    }

    /// Bring a saved inner tab back. A folder that is gone comes back as
    /// a tab all the same, and says so in the pane.
    pub fn restore_tab(&mut self, s: &Services, saved: &RepoTabState) {
        self.add_tab(s, &saved.project, &saved.name, saved.path.clone());
    }

    pub fn active_pane_mut(&mut self) -> Option<&mut RepoPane> {
        let i = self.active.min(self.tabs.len().checked_sub(1)?);
        self.tabs.get_mut(i).map(|t| &mut t.pane)
    }

    pub fn active_tab(&self) -> Option<&RepoTab> {
        self.tabs
            .get(self.active.min(self.tabs.len().checked_sub(1)?))
    }

    fn add_tab(&mut self, s: &Services, project: &str, name: &str, path: PathBuf) {
        if let Some(i) = self.tabs.iter().position(|t| t.project == project) {
            self.active = i;
            return;
        }
        let mut pane = RepoPane::default();
        if path.join(".git").exists() {
            pane.set_root(s, path.clone());
        }
        pane.set_credential(&self.cred.id);
        self.tabs.push(RepoTab {
            uid: NEXT_TAB.fetch_add(1, Ordering::Relaxed),
            project: project.to_string(),
            name: name.to_string(),
            path,
            pane,
        });
        self.active = self.tabs.len() - 1;
    }

    /// Ask the host for its repositories. `Err` when the credential's
    /// token cannot be read (a locked vault); the caller toasts it.
    pub fn fetch(&mut self, s: &Services) -> Result<(), String> {
        if !self.cred.kind.lists_projects() {
            self.projects = Some(Err(s.t("git-host-no-api")));
            return Ok(());
        }
        let token = crate::auth::resolve_cred(s, &self.cred)?
            .map(|r| r.token)
            .unwrap_or_default();
        let cred = self.cred.clone();
        let slot = self.slot.clone();
        self.busy = true;
        s.runtime.spawn(async move {
            let http = hs_core::ai::http_client();
            *slot.lock() = Some(crate::hosts::list_projects(&http, &cred, &token).await);
        });
        Ok(())
    }

    /// Take what the runtime left: the listing, and a finished clone,
    /// which is recorded in the repository list and opened.
    pub fn poll(&mut self, cx: &mut ToolContext) {
        if let Some(r) = self.slot.lock().take() {
            self.projects = Some(r);
            self.busy = false;
        }
        let done = self
            .cloning
            .as_ref()
            .and_then(|(_, _, slot)| slot.lock().take());
        if let Some(r) = done
            && let Some((project, dest, _)) = self.cloning.take()
        {
            match r {
                Ok(()) => {
                    let name = self.register(cx, &project, &dest);
                    // A fresh clone is a project too: the Editor lists it
                    // without being opened (CR-0053).
                    if let Err(e) = cx.services.projects.write().add_root(&dest, &name) {
                        cx.toast(ToastLevel::Error, e.to_string());
                    }
                    self.add_tab(cx.services, &project.full_name, &name, dest);
                }
                Err(e) => cx.toast(ToastLevel::Error, e),
            }
        }
    }

    /// Where a project is cloned to: `<clone folder>/<host>/<owner>/<name>`,
    /// the clone folder from Settings → Git, or `repos/` in the data
    /// folder when none is set.
    pub fn dest_for(s: &Services, cred: &GitCredential, project: &str) -> PathBuf {
        let base = s.settings.read().git.clone_folder.trim().to_string();
        let base = if base.is_empty() {
            s.paths.root().join("repos")
        } else {
            PathBuf::from(base)
        };
        let mut out = base.join(slug(&cred.label()));
        for seg in project.split('/').filter(|p| !p.is_empty()) {
            out = out.join(slug(seg));
        }
        out
    }

    /// Where a clone lands under the parent folder the user picked: the
    /// project's own name, slugged (CR-0053).
    pub fn clone_dest(parent: &Path, project: &str) -> PathBuf {
        let name = project
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .unwrap_or(project);
        parent.join(slug(name))
    }

    /// Record a cloned project in the workspace's repository list, and
    /// give back the name the tab shows.
    fn register(&self, cx: &mut ToolContext, project: &Project, dest: &Path) -> String {
        let s = cx.services;
        let name = project
            .full_name
            .rsplit('/')
            .next()
            .unwrap_or(&project.full_name)
            .to_string();
        let known = s.git_repos.read().by_path(dest).map(|e| e.label());
        if let Some(label) = known {
            return label;
        }
        let mut entry = RepoEntry::for_path(dest.to_path_buf());
        entry.name = name.clone();
        entry.source = Source::Remote;
        entry.host_url = self.cred.host();
        entry.project = project.full_name.clone();
        entry.url = project.clone_url.clone();
        entry.credential = self.cred.id.clone();
        if let Err(e) = s.git_repos.write().upsert(entry) {
            cx.toast(ToastLevel::Error, e.to_string());
        }
        name
    }

    /// Open a project from the column: focus its tab, open the clone it
    /// already has, or clone it first as a Job.
    pub fn open_project(&mut self, cx: &mut ToolContext, p: &Project) {
        if let Some(i) = self.tabs.iter().position(|t| t.project == p.full_name) {
            self.active = i;
            return;
        }
        let s = cx.services;
        let host = self.cred.host();
        let listed = s
            .git_repos
            .read()
            .all()
            .iter()
            .find(|e| e.project == p.full_name && e.host_url == host && e.is_cloned())
            .map(|e| (e.label(), e.path.clone()));
        if let Some((name, path)) = listed {
            self.add_tab(s, &p.full_name, &name, path);
            return;
        }
        let dest = Self::dest_for(s, &self.cred, &p.full_name);
        if dest.join(".git").exists() {
            let name = self.register(cx, p, &dest);
            self.add_tab(cx.services, &p.full_name, &name, dest);
            return;
        }
        if self.cloning.is_some() {
            cx.toast(ToastLevel::Info, s.t("git-host-clone-busy"));
            return;
        }
        // The user picks the parent folder (CR-0053): the last pick for
        // this host is offered first, else the default layout's parent.
        let suggested = self
            .clone_parent
            .clone()
            .or_else(|| dest.parent().map(Path::to_path_buf))
            .filter(|d| d.is_dir());
        let mut dialog = rfd::FileDialog::new().set_title(s.t("git-host-clone-into"));
        if let Some(d) = suggested {
            dialog = dialog.set_directory(d);
        }
        let Some(parent) = dialog.pick_folder() else {
            return;
        };
        self.clone_parent = Some(parent.clone());
        let dest = Self::clone_dest(&parent, &p.full_name);
        if dest.join(".git").exists() {
            let name = self.register(cx, p, &dest);
            self.add_tab(cx.services, &p.full_name, &name, dest);
            return;
        }
        let resolved = match crate::auth::resolve_cred(s, &self.cred) {
            Ok(r) => r,
            Err(e) => {
                cx.toast(ToastLevel::Error, e);
                return;
            }
        };
        let slot: Slot<Result<(), String>> = Default::default();
        let done = slot.clone();
        let (url, target) = (p.clone_url.clone(), dest.clone());
        let depth = s.settings.read().git.shallow_clones.then_some(1);
        let title = s.i18n.t_args("git-host-cloning", &[("name", &p.full_name)]);
        s.jobs.spawn(title, "git", move |h| async move {
            let cancel = h.clone();
            let progress = h.clone();
            let r = tokio::task::spawn_blocking(move || {
                crate::auth::with_env(resolved.as_ref(), |env| {
                    hs_git::write::clone(
                        &url,
                        &target,
                        depth,
                        env,
                        &|| cancel.is_cancelled(),
                        &mut |line| progress.set_message(line.to_string()),
                    )
                    .map_err(|e| e.to_string())
                })
                .and_then(|r| r)
            })
            .await
            .map_err(|e| e.to_string())
            .and_then(|r| r);
            *done.lock() = Some(r.clone());
            r
        });
        self.cloning = Some((p.clone(), dest, slot));
    }

    /// The repository column: the host's listing, Mine then Shared, with
    /// a filter and Refresh.
    pub fn column(&mut self, ui: &mut egui::Ui, theme: &Theme, cx: &mut ToolContext) {
        let s = cx.services;
        if self.projects.is_none()
            && !self.busy
            && let Err(e) = self.fetch(s)
        {
            self.projects = Some(Err(e));
        }
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(s.t("git-host-repos"))
                    .small()
                    .strong()
                    .color(theme.text_muted),
            );
            hs_ui::toolbar::hard_right(ui, |ui| {
                if form::icon_button(ui, theme, ph::ARROW_CLOCKWISE, 14.0, false)
                    .tip(s.t("git-host-refresh"))
                    .clicked()
                    && let Err(e) = self.fetch(s)
                {
                    cx.toast(ToastLevel::Error, e);
                }
            });
        });
        ui.add_space(4.0);
        ui.add(
            egui::TextEdit::singleline(&mut self.filter)
                .hint_text(s.t("git-host-filter"))
                .desired_width(f32::INFINITY),
        );
        ui.add_space(form::PANE_GAP);
        if self.busy {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.weak(s.t("git-host-loading"));
            });
            return;
        }
        let projects = match &self.projects {
            None => return,
            Some(Err(e)) => {
                form::note(ui, theme, ph::WARNING, &hs_core::fmt::sentence(e));
                return;
            }
            Some(Ok(p)) => p.clone(),
        };
        if projects.is_empty() {
            form::note(ui, theme, ph::INFO, &s.t("git-host-empty"));
            return;
        }
        let needle = self.filter.trim().to_lowercase();
        let shown: Vec<&Project> = projects
            .iter()
            .filter(|p| needle.is_empty() || p.full_name.to_lowercase().contains(&needle))
            .collect();
        let mine_prefix = format!("{}/", self.cred.username.trim());
        let (mine, shared): (Vec<&Project>, Vec<&Project>) = shown.into_iter().partition(|p| {
            !self.cred.username.trim().is_empty() && p.full_name.starts_with(&mine_prefix)
        });
        let here = self.active_tab().map(|t| t.project.clone());
        let cloning = self.cloning.as_ref().map(|(p, _, _)| p.full_name.clone());
        let mut open: Option<Project> = None;
        egui::ScrollArea::vertical()
            .id_salt(("git-host-repos", &self.cred.id))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for (key, group) in [("git-host-mine", &mine), ("git-host-shared", &shared)] {
                    if group.is_empty() {
                        continue;
                    }
                    ui.label(
                        egui::RichText::new(s.t(key))
                            .small()
                            .color(theme.text_muted),
                    );
                    ui.add_space(2.0);
                    for p in group {
                        let glyph = if p.private { ph::LOCK } else { ph::GIT_BRANCH };
                        let subtitle = if cloning.as_deref() == Some(p.full_name.as_str()) {
                            s.t("git-host-cloning-row")
                        } else {
                            p.description.clone()
                        };
                        let row = form::list_row(
                            ui,
                            theme,
                            here.as_deref() == Some(p.full_name.as_str()),
                            glyph,
                            &p.full_name,
                            (!subtitle.is_empty()).then_some(subtitle.as_str()),
                        );
                        if row.clicked() {
                            open = Some((*p).clone());
                        }
                        ui.add_space(2.0);
                    }
                    ui.add_space(form::PANE_GAP);
                }
            });
        if let Some(p) = open {
            self.open_project(cx, &p);
        }
    }

    /// The inner strip and the active repository's pane.
    pub fn body(&mut self, ui: &mut egui::Ui, theme: &Theme, cx: &mut ToolContext) {
        let s = cx.services;
        if self.tabs.is_empty() {
            ui.add_space(24.0);
            ui.horizontal(|ui| {
                ui.add_space(24.0);
                form::note(ui, theme, ph::LIGHTBULB, &s.t("git-host-pick"));
            });
            return;
        }
        struct Item {
            id: u64,
            title: String,
        }
        impl hs_ui::tabs::TabItem for Item {
            fn id(&self) -> u64 {
                self.id
            }
            fn title(&self) -> String {
                self.title.clone()
            }
            fn icon(&self) -> &str {
                ph::GIT_BRANCH
            }
            fn pinned(&self) -> bool {
                false
            }
            fn dirty(&self) -> bool {
                false
            }
        }
        let items: Vec<Item> = self
            .tabs
            .iter()
            .map(|t| Item {
                id: t.uid,
                title: t.name.clone(),
            })
            .collect();
        let labels = hs_ui::tabs::TabStripLabels {
            new_tab: String::new(),
            close: s.t("git-close-repo"),
            close_others: s.t("editor-close-others"),
            close_left: s.t("tab-close-left"),
            close_right: s.t("tab-close-right"),
            pin: String::new(),
            unpin: String::new(),
            duplicate: String::new(),
            duplicate_blocked: String::new(),
            copy_path: String::new(),
        };
        let (min_width, sub_h) = {
            let st = s.settings.read();
            (
                st.appearance.min_file_tab_width,
                st.appearance.sub_tab_height(),
            )
        };
        let active = self.active_tab().map(|t| t.uid);
        let r = ui
            .push_id(("git-host-strip", &self.cred.id), |ui| {
                hs_ui::tabs::TabStrip {
                    theme,
                    min_width,
                    gap: 2.0,
                    active,
                    labels: &labels,
                    compact: true,
                    show_new_tab: false,
                    height: Some(sub_h),
                }
                .show(ui, &items)
            })
            .inner;
        if let Some(uid) = r.activated
            && let Some(i) = self.tabs.iter().position(|t| t.uid == uid)
        {
            self.active = i;
        }
        if let Some((from, to)) = r.reorder
            && from < self.tabs.len()
            && to <= self.tabs.len()
        {
            let t = self.tabs.remove(from);
            let to = to.min(self.tabs.len());
            self.tabs.insert(to, t);
            self.active = to;
        }
        if let Some(uid) = r.close
            && let Some(i) = self.tabs.iter().position(|t| t.uid == uid)
        {
            // Closing forgets the tab; the clone and the list entry stay.
            self.tabs.remove(i);
            self.active = self.active.min(self.tabs.len().saturating_sub(1));
        }
        let index = self.active.min(self.tabs.len().saturating_sub(1));
        let Some(tab) = self.tabs.get_mut(index) else {
            return;
        };
        if !tab.path.join(".git").exists() {
            ui.add_space(16.0);
            ui.horizontal(|ui| {
                ui.add_space(16.0);
                form::note(
                    ui,
                    theme,
                    ph::WARNING,
                    &s.i18n.t_args(
                        "git-host-missing-clone",
                        &[("path", &tab.path.display().to_string())],
                    ),
                );
            });
            return;
        }
        tab.pane.ui(ui, theme, cx);
    }
}

/// A folder-safe piece of a path: letters, digits, `.`, `_` and `-`;
/// anything else becomes `-`, and runs collapse.
pub fn slug(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut dash = false;
    for c in text.trim().chars() {
        if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
            out.push(c.to_ascii_lowercase());
            dash = false;
        } else if !dash {
            out.push('-');
            dash = true;
        }
    }
    out.trim_matches('-').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_clone_lands_under_the_picked_folder_by_its_own_name() {
        let d = HostView::clone_dest(Path::new("D:/src"), "owner/Hyper Stack");
        assert_eq!(d, Path::new("D:/src").join(slug("Hyper Stack")));
        assert_eq!(
            HostView::clone_dest(Path::new("/tmp"), "lock-screen"),
            Path::new("/tmp/lock-screen")
        );
    }

    #[test]
    fn a_clone_lands_under_host_owner_and_name() {
        // FEAT-0100: `<folder>/<host>/<owner>/<name>`, every piece a slug.
        assert_eq!(slug("Work GitLab"), "work-gitlab");
        assert_eq!(slug("https://git.zacx.net"), "https-git.zacx.net");
        assert_eq!(slug("  owner  "), "owner");
        let parts: Vec<String> = "owner/hyper-stack".split('/').map(slug).collect();
        assert_eq!(parts, vec!["owner", "hyper-stack"]);
    }
}
