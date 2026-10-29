//! The toolbar verbs and their inline panels (docs/tools/git-activity.md
//! §5.2, §5.4): Branch, Merge, Rebase (plain and interactive), Stash,
//! Tag, Git Flow, plus the Rename and Remote forms the tree's menus open.
//! A panel collects its options and hands back one [`Op`] on Apply; the
//! pane runs it on the blocking pool and asks first where a guard says
//! so. Nothing here touches git directly.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use hs_core::services::Services;
use hs_git::write::{RebaseAction, RebaseCommit};
use hs_ui::icons::ph;
use hs_ui::theme::Theme;
use parking_lot::Mutex;

type Slot<T> = Arc<Mutex<Option<T>>>;

/// The body of an operation: given the repository root, the summary
/// git printed or the error.
pub type Run = Box<dyn FnOnce(&Path) -> Result<String, String> + Send + 'static>;

/// One git operation, ready to run.
pub struct Op {
    pub label: String,
    /// Whether it deserves a toast when it ends.
    pub notify: bool,
    pub run: Run,
}

impl Op {
    pub fn new(
        label: impl Into<String>,
        run: impl FnOnce(&Path) -> Result<String, String> + Send + 'static,
    ) -> Self {
        Self {
            label: label.into(),
            notify: true,
            run: Box::new(run),
        }
    }
}

fn e(r: Result<impl Into<String>, hs_git::GitError>) -> Result<String, String> {
    r.map(Into::into).map_err(|e| e.to_string())
}

fn unit(r: Result<(), hs_git::GitError>) -> Result<String, String> {
    r.map(|()| String::new()).map_err(|e| e.to_string())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Panel {
    #[default]
    None,
    Branch,
    Merge,
    Rebase,
    Interactive,
    Stash,
    Tag,
    Flow,
    Rename,
    Remote,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlowKind {
    Feature,
    Release,
    Hotfix,
}

impl FlowKind {
    fn key(self) -> &'static str {
        match self {
            FlowKind::Feature => "git-flow-feature",
            FlowKind::Release => "git-flow-release",
            FlowKind::Hotfix => "git-flow-hotfix",
        }
    }
}

/// What the panels need to know about the repository.
pub struct Facts<'a> {
    pub branches: &'a [hs_git::Branch],
    pub remotes: &'a [hs_git::Remote],
    pub current: Option<&'a str>,
}

pub struct Verbs {
    pub panel: Panel,
    root: Option<PathBuf>,
    // Branch
    branch_name: String,
    branch_from: String,
    branch_checkout: bool,
    // Merge
    merge_source: String,
    merge_no_ff: bool,
    merge_squash: bool,
    merge_no_commit: bool,
    // Rebase
    rebase_onto: String,
    // Interactive rebase
    inter_base: String,
    inter_todo: Vec<(RebaseCommit, RebaseAction)>,
    inter_slot: Slot<(String, Result<Vec<RebaseCommit>, String>)>,
    inter_loaded_for: String,
    // Stash
    stash_message: String,
    stash_untracked: bool,
    // Tag
    tag_name: String,
    tag_message: String,
    tag_target: String,
    tag_push: bool,
    // Git Flow
    flow_kind: FlowKind,
    flow_name: String,
    flow_finish: bool,
    // Rename
    rename_old: String,
    rename_new: String,
    // Remote
    remote_old: Option<String>,
    remote_name: String,
    remote_url: String,
}

impl Default for Verbs {
    fn default() -> Self {
        Self {
            panel: Panel::None,
            root: None,
            branch_name: String::new(),
            branch_from: String::new(),
            branch_checkout: true,
            merge_source: String::new(),
            merge_no_ff: false,
            merge_squash: false,
            merge_no_commit: false,
            rebase_onto: String::new(),
            inter_base: String::new(),
            inter_todo: Vec::new(),
            inter_slot: Slot::default(),
            inter_loaded_for: String::new(),
            stash_message: String::new(),
            stash_untracked: true,
            tag_name: String::new(),
            tag_message: String::new(),
            tag_target: String::new(),
            tag_push: false,
            flow_kind: FlowKind::Feature,
            flow_name: String::new(),
            flow_finish: false,
            rename_old: String::new(),
            rename_new: String::new(),
            remote_old: None,
            remote_name: String::new(),
            remote_url: String::new(),
        }
    }
}

impl Verbs {
    pub fn set_root(&mut self, root: PathBuf, stash_untracked: bool) {
        *self = Self {
            root: Some(root),
            stash_untracked,
            ..Self::default()
        };
    }

    fn toggle(&mut self, p: Panel) {
        self.panel = if self.panel == p { Panel::None } else { p };
    }

    /// Open the Tag panel for `target` (a commit from History).
    pub fn tag_at(&mut self, target: &str) {
        self.tag_target = target.to_string();
        self.panel = Panel::Tag;
    }

    /// Open the Branch panel starting from `from`.
    pub fn branch_at(&mut self, from: &str) {
        self.branch_from = from.to_string();
        self.panel = Panel::Branch;
    }

    pub fn merge_from(&mut self, source: &str) {
        self.merge_source = source.to_string();
        self.panel = Panel::Merge;
    }

    pub fn rebase_on(&mut self, onto: &str) {
        self.rebase_onto = onto.to_string();
        self.panel = Panel::Rebase;
    }

    pub fn rename(&mut self, old: &str) {
        self.rename_old = old.to_string();
        self.rename_new = old.to_string();
        self.panel = Panel::Rename;
    }

    pub fn remote(&mut self, existing: Option<&hs_git::Remote>) {
        match existing {
            Some(r) => {
                self.remote_old = Some(r.name.clone());
                self.remote_name = r.name.clone();
                self.remote_url = r.url.clone();
            }
            None => {
                self.remote_old = None;
                self.remote_name = "origin".into();
                self.remote_url.clear();
            }
        }
        self.panel = Panel::Remote;
    }

    /// The toolbar's verb buttons, after Push.
    pub fn toolbar(&mut self, ui: &mut egui::Ui, s: &Services, busy: bool) {
        for (p, icon, key) in [
            (Panel::Branch, ph::GIT_BRANCH, "git-verb-branch"),
            (Panel::Merge, ph::GIT_MERGE, "git-verb-merge"),
            (Panel::Rebase, ph::GIT_PULL_REQUEST, "git-verb-rebase"),
            (Panel::Stash, ph::ARCHIVE, "git-verb-stash"),
            (Panel::Tag, ph::TAG, "git-verb-tag"),
            (Panel::Flow, ph::FLOW_ARROW, "git-verb-flow"),
        ] {
            let b = egui::Button::selectable(self.panel == p, format!("{icon} {}", s.t(key)));
            if ui.add_enabled(!busy, b).clicked() {
                self.toggle(p);
            }
        }
    }

    /// The open panel, drawn under the toolbar. Returns the operation to
    /// run when Apply was pressed.
    pub fn panel_ui(
        &mut self,
        ui: &mut egui::Ui,
        theme: &Theme,
        s: &Services,
        facts: &Facts<'_>,
        busy: bool,
    ) -> Option<Op> {
        if self.panel == Panel::None {
            return None;
        }
        let mut op = None;
        let mut close = false;
        egui::Frame::new()
            .fill(theme.surface)
            .inner_margin(egui::Margin::symmetric(12, 8))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    let title = match self.panel {
                        Panel::Branch => "git-verb-branch",
                        Panel::Merge => "git-verb-merge",
                        Panel::Rebase => "git-verb-rebase",
                        Panel::Interactive => "git-verb-interactive",
                        Panel::Stash => "git-verb-stash",
                        Panel::Tag => "git-verb-tag",
                        Panel::Flow => "git-verb-flow",
                        Panel::Rename => "git-verb-rename",
                        Panel::Remote => "git-verb-remote",
                        Panel::None => "",
                    };
                    ui.label(egui::RichText::new(s.t(title)).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button(ph::X).clicked() {
                            close = true;
                        }
                    });
                });
                ui.add_space(4.0);
                op = match self.panel {
                    Panel::Branch => self.branch_panel(ui, s, facts, busy),
                    Panel::Merge => self.merge_panel(ui, s, facts, busy),
                    Panel::Rebase => self.rebase_panel(ui, s, facts, busy),
                    Panel::Interactive => self.interactive_panel(ui, theme, s, facts, busy),
                    Panel::Stash => self.stash_panel(ui, s, busy),
                    Panel::Tag => self.tag_panel(ui, s, facts, busy),
                    Panel::Flow => self.flow_panel(ui, theme, s, facts, busy),
                    Panel::Rename => self.rename_panel(ui, s, busy),
                    Panel::Remote => self.remote_panel(ui, s, busy),
                    Panel::None => None,
                };
            });
        if close || op.is_some() {
            self.panel = Panel::None;
        }
        op
    }

    fn ref_picker(
        ui: &mut egui::Ui,
        salt: &str,
        value: &mut String,
        facts: &Facts<'_>,
        empty: &str,
    ) {
        let shown = if value.is_empty() {
            empty.to_string()
        } else {
            value.clone()
        };
        egui::ComboBox::from_id_salt(salt)
            .selected_text(shown)
            .width(220.0)
            .show_ui(ui, |ui| {
                for b in facts.branches {
                    if ui.selectable_label(*value == b.name, &b.name).clicked() {
                        *value = b.name.clone();
                    }
                }
            });
    }

    fn apply(ui: &mut egui::Ui, s: &Services, enabled: bool, key: &str) -> bool {
        ui.add_enabled(
            enabled,
            egui::Button::new(format!("{} {}", ph::CHECK, s.t(key))),
        )
        .clicked()
    }

    fn branch_panel(
        &mut self,
        ui: &mut egui::Ui,
        s: &Services,
        facts: &Facts<'_>,
        busy: bool,
    ) -> Option<Op> {
        let mut op = None;
        ui.horizontal(|ui| {
            ui.label(s.t("git-branch-name"));
            ui.add(
                egui::TextEdit::singleline(&mut self.branch_name)
                    .hint_text("feature/…")
                    .desired_width(220.0),
            );
            ui.label(s.t("git-branch-from"));
            Self::ref_picker(
                ui,
                "git-branch-from",
                &mut self.branch_from,
                facts,
                &s.t("git-head"),
            );
            ui.checkbox(&mut self.branch_checkout, s.t("git-branch-checkout"));
            let name = self.branch_name.trim().to_string();
            if Self::apply(ui, s, !busy && !name.is_empty(), "git-branch-create") {
                let from = (!self.branch_from.is_empty()).then(|| self.branch_from.clone());
                let checkout = self.branch_checkout;
                op = Some(Op::new(s.t("git-verb-branch"), move |r| {
                    unit(hs_git::write::create_branch(r, &name, from.as_deref()))?;
                    if checkout {
                        unit(hs_git::write::checkout(r, &name))?;
                    }
                    Ok(String::new())
                }));
                self.branch_name.clear();
            }
        });
        op
    }

    fn merge_panel(
        &mut self,
        ui: &mut egui::Ui,
        s: &Services,
        facts: &Facts<'_>,
        busy: bool,
    ) -> Option<Op> {
        let mut op = None;
        ui.horizontal(|ui| {
            ui.label(s.t("git-panel-merge-source"));
            Self::ref_picker(
                ui,
                "git-panel-merge-source",
                &mut self.merge_source,
                facts,
                &s.t("git-pick-branch"),
            );
            ui.label(egui::RichText::new(format!(
                "{} {}",
                ph::ARROW_RIGHT,
                facts.current.unwrap_or("HEAD")
            )));
            ui.checkbox(&mut self.merge_no_ff, s.t("git-merge-no-ff"));
            ui.checkbox(&mut self.merge_squash, s.t("git-panel-squash"));
            ui.checkbox(&mut self.merge_no_commit, s.t("git-panel-no-commit"));
            let source = self.merge_source.clone();
            if Self::apply(ui, s, !busy && !source.is_empty(), "git-verb-merge") {
                let (no_ff, squash, no_commit) =
                    (self.merge_no_ff, self.merge_squash, self.merge_no_commit);
                op = Some(Op::new(s.t("git-verb-merge"), move |r| {
                    e(hs_git::write::merge_with(
                        r, &source, no_ff, no_commit, squash,
                    ))
                }));
            }
        });
        op
    }

    fn rebase_panel(
        &mut self,
        ui: &mut egui::Ui,
        s: &Services,
        facts: &Facts<'_>,
        busy: bool,
    ) -> Option<Op> {
        let mut op = None;
        ui.horizontal(|ui| {
            ui.label(s.t("git-panel-rebase-onto"));
            Self::ref_picker(
                ui,
                "git-panel-rebase-onto",
                &mut self.rebase_onto,
                facts,
                &s.t("git-pick-branch"),
            );
            let onto = self.rebase_onto.clone();
            if Self::apply(ui, s, !busy && !onto.is_empty(), "git-verb-rebase") {
                let target = onto.clone();
                op = Some(Op::new(s.t("git-verb-rebase"), move |r| {
                    e(hs_git::write::rebase(r, &target))
                }));
            }
            ui.separator();
            if ui
                .add_enabled(
                    !busy && !onto.is_empty(),
                    egui::Button::new(s.t("git-verb-interactive")),
                )
                .clicked()
            {
                self.inter_base = onto;
                self.inter_loaded_for.clear();
                self.panel = Panel::Interactive;
            }
        });
        op
    }

    fn interactive_panel(
        &mut self,
        ui: &mut egui::Ui,
        theme: &Theme,
        s: &Services,
        _facts: &Facts<'_>,
        busy: bool,
    ) -> Option<Op> {
        // Read the commits once per base.
        if self.inter_loaded_for != self.inter_base
            && let Some(root) = self.root.clone()
        {
            self.inter_loaded_for = self.inter_base.clone();
            self.inter_todo.clear();
            let base = self.inter_base.clone();
            let slot = self.inter_slot.clone();
            s.runtime.spawn_blocking(move || {
                let r = hs_git::write::commits_since(&root, &base).map_err(|e| e.to_string());
                *slot.lock() = Some((base, r));
            });
        }
        let got = self.inter_slot.lock().take();
        if let Some((base, r)) = got
            && base == self.inter_base
        {
            match r {
                Ok(list) => {
                    self.inter_todo = list.into_iter().map(|c| (c, RebaseAction::Pick)).collect()
                }
                Err(err) => {
                    ui.label(egui::RichText::new(err).color(theme.error));
                }
            }
        }
        ui.label(
            egui::RichText::new(s.i18n.t_args(
                "git-interactive-hint",
                &[
                    ("base", &self.inter_base),
                    ("count", &self.inter_todo.len().to_string()),
                ],
            ))
            .color(theme.text_muted),
        );
        let n = self.inter_todo.len();
        let mut swap: Option<(usize, usize)> = None;
        egui::ScrollArea::vertical()
            .id_salt("git-interactive-list")
            .max_height(220.0)
            .show(ui, |ui| {
                for i in 0..n {
                    ui.horizontal(|ui| {
                        let (c, action) = &mut self.inter_todo[i];
                        egui::ComboBox::from_id_salt(("git-rebase-action", i))
                            .selected_text(action.as_str())
                            .width(90.0)
                            .show_ui(ui, |ui| {
                                for a in [
                                    RebaseAction::Pick,
                                    RebaseAction::Squash,
                                    RebaseAction::Fixup,
                                    RebaseAction::Drop,
                                ] {
                                    ui.selectable_value(action, a, a.as_str());
                                }
                            });
                        ui.label(
                            egui::RichText::new(&c.short)
                                .monospace()
                                .color(theme.text_muted),
                        );
                        ui.label(&c.subject);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .add_enabled(i + 1 < n, egui::Button::new(ph::ARROW_DOWN).small())
                                .clicked()
                            {
                                swap = Some((i, i + 1));
                            }
                            if ui
                                .add_enabled(i > 0, egui::Button::new(ph::ARROW_UP).small())
                                .clicked()
                            {
                                swap = Some((i, i - 1));
                            }
                        });
                    });
                }
            });
        if let Some((a, b)) = swap {
            self.inter_todo.swap(a, b);
        }
        let mut op = None;
        ui.horizontal(|ui| {
            if Self::apply(ui, s, !busy && n > 0, "git-verb-rebase") {
                let base = self.inter_base.clone();
                let todo: Vec<(String, RebaseAction)> = self
                    .inter_todo
                    .iter()
                    .map(|(c, a)| (c.sha.clone(), *a))
                    .collect();
                op = Some(Op::new(s.t("git-verb-interactive"), move |r| {
                    e(hs_git::write::rebase_interactive(r, &base, &todo))
                }));
            }
        });
        op
    }

    fn stash_panel(&mut self, ui: &mut egui::Ui, s: &Services, busy: bool) -> Option<Op> {
        let mut op = None;
        ui.horizontal(|ui| {
            ui.label(s.t("git-stash-message"));
            ui.add(
                egui::TextEdit::singleline(&mut self.stash_message)
                    .hint_text(s.t("git-stash-message-hint"))
                    .desired_width(300.0),
            );
            ui.checkbox(&mut self.stash_untracked, s.t("git-stash-untracked"));
            if Self::apply(ui, s, !busy, "git-panel-stash") {
                let message = self.stash_message.clone();
                let untracked = self.stash_untracked;
                op = Some(Op::new(s.t("git-verb-stash"), move |r| {
                    unit(hs_git::write::stash_push(r, &message, untracked))
                }));
                self.stash_message.clear();
            }
        });
        op
    }

    fn tag_panel(
        &mut self,
        ui: &mut egui::Ui,
        s: &Services,
        facts: &Facts<'_>,
        busy: bool,
    ) -> Option<Op> {
        let mut op = None;
        ui.horizontal(|ui| {
            ui.label(s.t("git-panel-tag-name"));
            ui.add(
                egui::TextEdit::singleline(&mut self.tag_name)
                    .hint_text("v1.2.0")
                    .desired_width(140.0),
            );
            ui.label(s.t("git-tag-message"));
            ui.add(
                egui::TextEdit::singleline(&mut self.tag_message)
                    .hint_text(s.t("git-panel-tag-hint"))
                    .desired_width(240.0),
            );
            ui.label(s.t("git-tag-target"));
            ui.add(
                egui::TextEdit::singleline(&mut self.tag_target)
                    .hint_text(facts.current.unwrap_or("HEAD"))
                    .desired_width(110.0),
            );
            ui.checkbox(&mut self.tag_push, s.t("git-panel-tag-push"));
            let name = self.tag_name.trim().to_string();
            if Self::apply(ui, s, !busy && !name.is_empty(), "git-tag-create") {
                let message = self.tag_message.clone();
                let target = (!self.tag_target.trim().is_empty())
                    .then(|| self.tag_target.trim().to_string());
                let push = self.tag_push;
                let remote = facts.remotes.first().map(|r| r.name.clone());
                op = Some(Op::new(s.t("git-verb-tag"), move |r| {
                    unit(hs_git::write::create_tag(
                        r,
                        &name,
                        target.as_deref(),
                        &message,
                    ))?;
                    if push && let Some(remote) = remote {
                        return e(hs_git::write::push_tag(r, &remote, &name));
                    }
                    Ok(String::new())
                }));
                self.tag_name.clear();
                self.tag_message.clear();
                self.tag_target.clear();
            }
        });
        op
    }

    fn flow_panel(
        &mut self,
        ui: &mut egui::Ui,
        theme: &Theme,
        s: &Services,
        facts: &Facts<'_>,
        busy: bool,
    ) -> Option<Op> {
        let flow = s.settings.read().git.flow.clone();
        let mut op = None;
        ui.horizontal(|ui| {
            for k in [FlowKind::Feature, FlowKind::Release, FlowKind::Hotfix] {
                if ui
                    .add(egui::Button::selectable(self.flow_kind == k, s.t(k.key())))
                    .clicked()
                {
                    self.flow_kind = k;
                }
            }
            ui.separator();
            if ui
                .add(egui::Button::selectable(
                    !self.flow_finish,
                    s.t("git-flow-start"),
                ))
                .clicked()
            {
                self.flow_finish = false;
            }
            if ui
                .add(egui::Button::selectable(
                    self.flow_finish,
                    s.t("git-flow-finish"),
                ))
                .clicked()
            {
                self.flow_finish = true;
            }
            ui.label(s.t("git-flow-name"));
            ui.add(
                egui::TextEdit::singleline(&mut self.flow_name)
                    .hint_text("login-page")
                    .desired_width(180.0),
            );
        });
        let prefix = match self.flow_kind {
            FlowKind::Feature => flow.feature.clone(),
            FlowKind::Release => flow.release.clone(),
            FlowKind::Hotfix => flow.hotfix.clone(),
        };
        let name = self.flow_name.trim().to_string();
        let branch = format!("{prefix}{name}");
        // Where it starts from and where it finishes into.
        let (base, into): (String, Vec<String>) = match self.flow_kind {
            FlowKind::Feature => (flow.develop.clone(), vec![flow.develop.clone()]),
            FlowKind::Release => (
                flow.develop.clone(),
                vec![flow.production.clone(), flow.develop.clone()],
            ),
            FlowKind::Hotfix => (
                flow.production.clone(),
                vec![flow.production.clone(), flow.develop.clone()],
            ),
        };
        let tag = match self.flow_kind {
            FlowKind::Feature => None,
            _ => Some(format!("{}{name}", flow.version_tag_prefix)),
        };
        // The exact commands, shown before Apply (docs/tools/git-activity.md §5.4).
        let mut lines: Vec<String> = Vec::new();
        if !self.flow_finish {
            lines.push(format!("git checkout -b {branch} {base}"));
        } else {
            for target in &into {
                lines.push(format!("git checkout {target}"));
                lines.push(format!("git merge --no-ff {branch}"));
                if let Some(t) = &tag
                    && target == &flow.production
                {
                    lines.push(format!("git tag -a {t} -m \"{t}\""));
                }
            }
            lines.push(format!("git branch -d {branch}"));
        }
        let have_base = facts.branches.iter().any(|b| !b.remote && b.name == base);
        let missing_base = !have_base && !self.flow_finish;
        egui::Frame::new()
            .fill(theme.panel_bg)
            .inner_margin(egui::Margin::symmetric(8, 6))
            .show(ui, |ui| {
                for l in &lines {
                    ui.label(egui::RichText::new(l).monospace().size(12.0));
                }
            });
        if missing_base {
            ui.label(
                egui::RichText::new(s.i18n.t_args("git-flow-missing-base", &[("branch", &base)]))
                    .color(theme.warning),
            );
        }
        ui.horizontal(|ui| {
            if Self::apply(
                ui,
                s,
                !busy && !name.is_empty() && !missing_base,
                "git-flow-apply",
            ) {
                let finish = self.flow_finish;
                let production = flow.production.clone();
                let label = s.t("git-verb-flow");
                op = Some(Op::new(label, move |r| {
                    if !finish {
                        unit(hs_git::write::create_branch(r, &branch, Some(&base)))?;
                        unit(hs_git::write::checkout(r, &branch))?;
                        return Ok(String::new());
                    }
                    let mut out = String::new();
                    for target in &into {
                        unit(hs_git::write::checkout(r, target))?;
                        out.push_str(&e(hs_git::write::merge_with(
                            r, &branch, true, false, false,
                        ))?);
                        if let Some(t) = &tag
                            && target == &production
                        {
                            unit(hs_git::write::create_tag(r, t, None, t))?;
                        }
                    }
                    unit(hs_git::write::delete_branch(r, &branch, false))?;
                    Ok(out)
                }));
                self.flow_name.clear();
            }
        });
        op
    }

    fn rename_panel(&mut self, ui: &mut egui::Ui, s: &Services, busy: bool) -> Option<Op> {
        let mut op = None;
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(&self.rename_old).monospace());
            ui.label(ph::ARROW_RIGHT);
            ui.add(egui::TextEdit::singleline(&mut self.rename_new).desired_width(220.0));
            let new = self.rename_new.trim().to_string();
            let old = self.rename_old.clone();
            if Self::apply(
                ui,
                s,
                !busy && !new.is_empty() && new != old,
                "git-verb-rename",
            ) {
                op = Some(Op::new(s.t("git-verb-rename"), move |r| {
                    unit(hs_git::write::rename_branch(r, &old, &new))
                }));
            }
        });
        op
    }

    fn remote_panel(&mut self, ui: &mut egui::Ui, s: &Services, busy: bool) -> Option<Op> {
        let mut op = None;
        ui.horizontal(|ui| {
            ui.label(s.t("git-panel-remote-name"));
            ui.add_enabled(
                self.remote_old.is_none(),
                egui::TextEdit::singleline(&mut self.remote_name).desired_width(120.0),
            );
            ui.label(s.t("git-panel-remote-url"));
            ui.add(
                egui::TextEdit::singleline(&mut self.remote_url)
                    .hint_text("https://host/owner/name.git")
                    .desired_width(360.0),
            );
            let name = self.remote_name.trim().to_string();
            let url = self.remote_url.trim().to_string();
            let key = if self.remote_old.is_some() {
                "git-remote-save"
            } else {
                "git-remote-add"
            };
            if Self::apply(ui, s, !busy && !name.is_empty() && !url.is_empty(), key) {
                let editing = self.remote_old.is_some();
                op = Some(Op::new(s.t("git-verb-remote"), move |r| {
                    if editing {
                        unit(hs_git::write::set_remote_url(r, &name, &url))
                    } else {
                        unit(hs_git::write::add_remote(r, &name, &url))
                    }
                }));
            }
        });
        op
    }
}
