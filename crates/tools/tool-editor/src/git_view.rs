//! The editor's Git view (docs/tools/git.md §3a): the branch of every
//! project, change letters in the project tree, a Source Control list in
//! the sidebar, a diff viewer and the repository history in the main area,
//! and change markers in the gutter. Read-only: it shows what `git` sees;
//! staging, commits and push come with the Git tool.
//!
//! Every `git` call runs on the blocking pool (`hs_git` blocks); results
//! come back through channels drained once a frame (ADR-013).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use egui::{Align2, Color32, CornerRadius, Sense, Vec2};
use hs_core::Services;
use hs_editor::{Buffer, LineMark};
use hs_git::{ChangedFile, Commit, CommitDetails, FileDiff, FileStatus, Kind, LineKind, Side};
use hs_ui::fileicons;
use hs_ui::icons::ph;
use hs_ui::theme::Theme;
use hs_ui::tip::TipExt;
use hs_ui::tree::TreeNode;

/// Commits fetched per page of the history list.
const PAGE: usize = 200;
/// Quiet time after a file-system event before `git status` runs again.
const DEBOUNCE: Duration = Duration::from_millis(400);
/// Quiet time after typing before the gutter markers are recomputed.
const MARKS_DEBOUNCE: Duration = Duration::from_millis(300);
/// Larger buffers get no gutter markers.
const MARKS_MAX_CHARS: usize = 2_000_000;
const ROW: f32 = 22.0;

/// One answer from the blocking pool, picked up when it is ready.
struct Pending<T>(Option<mpsc::Receiver<T>>);

impl<T: Send + 'static> Pending<T> {
    fn none() -> Self {
        Self(None)
    }

    fn start(s: &Services, f: impl FnOnce() -> T + Send + 'static) -> Self {
        let (tx, rx) = mpsc::channel();
        s.runtime.spawn_blocking(move || {
            let _ = tx.send(f());
        });
        Self(Some(rx))
    }

    fn busy(&self) -> bool {
        self.0.is_some()
    }

    fn take(&mut self) -> Option<T> {
        let r = self.0.as_ref()?.try_recv();
        match r {
            Ok(v) => {
                self.0 = None;
                Some(v)
            }
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                self.0 = None;
                None
            }
        }
    }
}

type GitResult<T> = Result<T, String>;

fn err(e: hs_git::GitError) -> String {
    e.to_string()
}

/// `root` joined with a repository-relative `/` path, in this OS's form.
fn abs(root: &Path, rel: &str) -> PathBuf {
    root.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR))
}

fn rel(root: &Path, path: &Path) -> Option<String> {
    let r = path.strip_prefix(root).ok()?;
    Some(
        r.components()
            .map(|c| c.as_os_str().to_string_lossy().to_string())
            .collect::<Vec<_>>()
            .join("/"),
    )
}

fn name_of(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string_lossy().to_string())
}

/// What a change letter means, for its colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Tone {
    Renamed,
    Added,
    Modified,
    Deleted,
    Conflict,
}

impl Tone {
    fn of(letter: char) -> Self {
        match letter {
            '!' => Tone::Conflict,
            'U' | '?' | 'A' => Tone::Added,
            'D' => Tone::Deleted,
            'R' | 'C' => Tone::Renamed,
            _ => Tone::Modified,
        }
    }

    fn colour(self, theme: &Theme) -> Color32 {
        match self {
            Tone::Added => theme.success,
            Tone::Modified => theme.warning,
            Tone::Deleted | Tone::Conflict => theme.error,
            Tone::Renamed => theme.accent,
        }
    }
}

// ---- repositories -------------------------------------------------------------

struct Repo {
    root: PathBuf,
    status: Option<hs_git::Status>,
    error: Option<String>,
    pending: Pending<hs_git::Result<hs_git::Status>>,
    /// A refresh is due at this time.
    due: Option<Instant>,
    /// Events arrived while a refresh ran: run it again when it ends.
    again: bool,
    /// Bumped on every new status.
    generation: u64,
    /// The write side: commit box, stashes, branches, operations.
    ops: crate::git_ops::Ops,
}

impl Repo {
    fn new(root: PathBuf) -> Self {
        Self {
            root,
            status: None,
            error: None,
            pending: Pending::none(),
            due: Some(Instant::now()),
            again: false,
            generation: 0,
            ops: crate::git_ops::Ops::default(),
        }
    }

    fn schedule(&mut self, delay: Duration) {
        if self.pending.busy() {
            self.again = true;
            return;
        }
        let t = Instant::now() + delay;
        self.due = Some(self.due.map_or(t, |d| d.min(t)));
    }

    fn name(&self) -> String {
        name_of(&self.root)
    }
}

/// Where a change shown in the diff view lives, so the arrow keys can step
/// through the Source Control list.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ChangeRef {
    repo: PathBuf,
    path: String,
    side: Side,
    /// Unmerged: the file opens in the conflict editor, not the diff.
    conflict: bool,
}

/// What the sidebar or the main view asks the editor to do.
pub(crate) enum GitAction {
    OpenFile(PathBuf),
    CopyPath(String),
    /// What an operation said, for the toast.
    Toast(hs_core::tool::ToastLevel, String),
}

// ---- diff rows ------------------------------------------------------------------

#[derive(Clone)]
enum Row {
    Note(String),
    /// A hunk header, with the hunk's index in the diff.
    Hunk(usize, String),
    Line {
        kind: LineKind,
        old: Option<u32>,
        new: Option<u32>,
        text: String,
    },
}

#[derive(Clone)]
struct Half {
    no: Option<u32>,
    kind: LineKind,
    text: String,
}

#[derive(Clone)]
enum SplitRow {
    Note(String),
    Hunk(usize, String),
    Pair(Option<Half>, Option<Half>),
}

/// A diff laid out for drawing, both ways.
struct DiffRows {
    inline: Vec<Row>,
    split: Vec<SplitRow>,
    /// Longest line, in characters.
    width: usize,
    /// Digits of the largest line number.
    digits: usize,
    added: usize,
    removed: usize,
    binary: bool,
}

fn display(text: &str) -> String {
    text.replace('\t', "    ")
}

impl DiffRows {
    fn new(d: &FileDiff) -> Self {
        let mut inline = Vec::new();
        let mut split = Vec::new();
        let mut width = 0;
        let mut max_no = 1;
        for n in &d.notes {
            inline.push(Row::Note(n.clone()));
            split.push(SplitRow::Note(n.clone()));
        }
        for (hi, h) in d.hunks.iter().enumerate() {
            inline.push(Row::Hunk(hi, h.header.clone()));
            split.push(SplitRow::Hunk(hi, h.header.clone()));
            let mut removed: Vec<Half> = Vec::new();
            let mut added: Vec<Half> = Vec::new();
            let flush =
                |removed: &mut Vec<Half>, added: &mut Vec<Half>, split: &mut Vec<SplitRow>| {
                    let n = removed.len().max(added.len());
                    let mut r = removed.drain(..);
                    let mut a = added.drain(..);
                    for _ in 0..n {
                        split.push(SplitRow::Pair(r.next(), a.next()));
                    }
                };
            for l in &h.lines {
                let text = display(&l.text);
                width = width.max(text.chars().count());
                max_no = max_no.max(l.old.unwrap_or(0)).max(l.new.unwrap_or(0));
                inline.push(Row::Line {
                    kind: l.kind,
                    old: l.old,
                    new: l.new,
                    text: text.clone(),
                });
                match l.kind {
                    LineKind::Removed => removed.push(Half {
                        no: l.old,
                        kind: l.kind,
                        text,
                    }),
                    LineKind::Added => added.push(Half {
                        no: l.new,
                        kind: l.kind,
                        text,
                    }),
                    LineKind::Context => {
                        flush(&mut removed, &mut added, &mut split);
                        split.push(SplitRow::Pair(
                            Some(Half {
                                no: l.old,
                                kind: l.kind,
                                text: text.clone(),
                            }),
                            Some(Half {
                                no: l.new,
                                kind: l.kind,
                                text,
                            }),
                        ));
                    }
                    LineKind::NoNewline => {}
                }
            }
            flush(&mut removed, &mut added, &mut split);
        }
        Self {
            inline,
            split,
            width,
            digits: max_no.to_string().len(),
            added: d.added(),
            removed: d.removed(),
            binary: d.binary,
        }
    }
}

/// A diff being loaded or shown.
struct DiffSlot {
    pending: Pending<GitResult<FileDiff>>,
    rows: Option<GitResult<DiffRows>>,
    /// The diff itself, for per-hunk operations.
    diff: Option<FileDiff>,
}

impl DiffSlot {
    fn load(s: &Services, f: impl FnOnce() -> GitResult<FileDiff> + Send + 'static) -> Self {
        Self {
            pending: Pending::start(s, f),
            rows: None,
            diff: None,
        }
    }

    fn poll(&mut self) {
        if let Some(r) = self.pending.take() {
            self.diff = r.as_ref().ok().cloned();
            self.rows = Some(r.map(|d| DiffRows::new(&d)));
        }
    }
}

// ---- main-area views ------------------------------------------------------------

struct DiffPane {
    change: ChangeRef,
    slot: DiffSlot,
    /// Status generation of the repository when the diff was asked for;
    /// a newer status reloads it.
    generation: u64,
}

/// The block-by-block editor for one conflicted file.
struct ConflictPane {
    change: ChangeRef,
    /// The file being read and parsed.
    load: Pending<GitResult<hs_git::conflict::Parsed>>,
    parsed: Option<hs_git::conflict::Parsed>,
    /// One per conflict block, in file order.
    choices: Vec<hs_git::conflict::Choice>,
    /// The write-and-stage in flight.
    save: Pending<GitResult<()>>,
    error: Option<String>,
}

struct HistoryPane {
    repo: PathBuf,
    commits: Vec<Commit>,
    pending: Pending<GitResult<Vec<Commit>>>,
    done: bool,
    error: Option<String>,
    selected: Option<usize>,
    details: Option<GitResult<CommitDetails>>,
    details_pending: Pending<(String, GitResult<CommitDetails>)>,
    file: Option<usize>,
    diff: Option<DiffSlot>,
    scroll_to_selected: bool,
    /// Scroll offset and height of the commit list in the last frame.
    list_offset: f32,
    list_height: f32,
}

impl HistoryPane {
    fn new(repo: PathBuf, s: &Services) -> Self {
        let mut h = Self {
            repo,
            commits: Vec::new(),
            pending: Pending::none(),
            done: false,
            error: None,
            selected: None,
            details: None,
            details_pending: Pending::none(),
            file: None,
            diff: None,
            scroll_to_selected: false,
            list_offset: 0.0,
            list_height: 0.0,
        };
        h.load_more(s);
        h
    }

    fn load_more(&mut self, s: &Services) {
        if self.pending.busy() || self.done {
            return;
        }
        let root = self.repo.clone();
        let skip = self.commits.len();
        self.pending = Pending::start(s, move || hs_git::log(&root, skip, PAGE).map_err(err));
    }

    fn select(&mut self, i: usize, s: &Services) {
        if i >= self.commits.len() || self.selected == Some(i) {
            return;
        }
        self.selected = Some(i);
        self.details = None;
        self.file = None;
        self.diff = None;
        let c = &self.commits[i];
        let (root, id, parent) = (self.repo.clone(), c.id.clone(), c.parents.first().cloned());
        self.details_pending = Pending::start(s, move || {
            let r = hs_git::commit_details(&root, &id, parent.as_deref()).map_err(err);
            (id, r)
        });
    }

    fn select_file(&mut self, fi: usize, s: &Services) {
        let (Some(ci), Some(Ok(det))) = (self.selected, &self.details) else {
            return;
        };
        let Some(f) = det.files.get(fi).cloned() else {
            return;
        };
        self.file = Some(fi);
        let c = &self.commits[ci];
        let (root, id, parent) = (self.repo.clone(), c.id.clone(), c.parents.first().cloned());
        self.diff = Some(DiffSlot::load(s, move || {
            hs_git::commit_file_diff(&root, &id, parent.as_deref(), &f).map_err(err)
        }));
    }

    fn poll(&mut self, s: &Services) {
        if let Some(r) = self.pending.take() {
            match r {
                Ok(page) => {
                    self.done = page.len() < PAGE;
                    self.commits.extend(page);
                    if self.selected.is_none() && !self.commits.is_empty() {
                        self.select(0, s);
                    }
                }
                Err(e) => {
                    self.error = Some(e);
                    self.done = true;
                }
            }
        }
        if let Some((id, r)) = self.details_pending.take()
            && self
                .selected
                .and_then(|i| self.commits.get(i))
                .is_some_and(|c| c.id == id)
        {
            let first = r.as_ref().is_ok_and(|d| !d.files.is_empty());
            self.details = Some(r);
            if first {
                self.select_file(0, s);
            }
        }
        if let Some(d) = &mut self.diff {
            d.poll();
        }
    }
}

enum MainView {
    Diff(DiffPane),
    Conflict(ConflictPane),
    History(HistoryPane),
}

// ---- per-file gutter state ------------------------------------------------------

/// The committed text a buffer is compared with.
#[derive(Clone)]
enum Base {
    /// Not a tracked file (untracked, ignored, outside a repository).
    None,
    /// Staged as new: every line is added.
    NewFile,
    Text(Arc<String>),
}

/// Gutter-marker state of one open file.
#[derive(Default)]
pub(crate) struct DocGit {
    /// (repository, status generation) the base was fetched for.
    key: Option<(PathBuf, u64)>,
    base: Option<Base>,
    base_pending: Option<Pending<GitResult<Option<String>>>>,
    marks_pending: Option<Pending<Vec<Option<hs_git::LineChange>>>>,
    /// Buffer version the current markers belong to.
    marks_for: Option<u64>,
    changed_at: Option<Instant>,
    seen_version: u64,
    has_marks: bool,
}

fn to_marks(v: Vec<Option<hs_git::LineChange>>) -> Vec<Option<LineMark>> {
    v.into_iter()
        .map(|m| {
            m.map(|m| match m {
                hs_git::LineChange::Added => LineMark::Added,
                hs_git::LineChange::Modified => LineMark::Modified,
                hs_git::LineChange::Deleted => LineMark::Deleted,
            })
        })
        .collect()
}

// ---- the whole view -------------------------------------------------------------

pub(crate) struct Git {
    repos: Vec<Repo>,
    /// The sidebar shows Source Control instead of the project tree.
    pub sidebar: bool,
    main: Option<MainView>,
    pub side_by_side: bool,
    /// Tree decorations, rebuilt when a status changes.
    decorated: Option<Vec<u64>>,
    files: HashMap<String, (char, Tone)>,
    dirs: HashMap<String, Tone>,
    branches: HashMap<String, String>,
    /// Project roots (tree root ids) and the repository each lies in.
    project_repos: Vec<(PathBuf, Option<PathBuf>)>,
    /// Results of operations, handed to the editor as toasts on the next
    /// sidebar pass.
    toasts: Vec<(hs_core::tool::ToastLevel, String)>,
    /// A push waiting for the protected-remote confirmation.
    pending_push: Option<crate::git_ops::Request>,
}

impl Git {
    pub fn new(sidebar: bool, side_by_side: bool) -> Self {
        Self {
            repos: Vec::new(),
            sidebar,
            main: None,
            side_by_side,
            decorated: None,
            files: HashMap::new(),
            dirs: HashMap::new(),
            branches: HashMap::new(),
            project_repos: Vec::new(),
            toasts: Vec::new(),
            pending_push: None,
        }
    }

    /// Follow the project list: find the repository of every project root.
    pub fn sync(&mut self, roots: &[PathBuf]) {
        self.project_repos = roots
            .iter()
            .map(|r| (r.clone(), hs_git::discover(r)))
            .collect();
        let wanted: HashSet<PathBuf> = self
            .project_repos
            .iter()
            .filter_map(|(_, r)| r.clone())
            .collect();
        self.repos.retain(|r| wanted.contains(&r.root));
        for w in wanted {
            if !self.repos.iter().any(|r| r.root == w) {
                self.repos.push(Repo::new(w));
            }
        }
        self.repos.sort_by(|a, b| a.root.cmp(&b.root));
        self.decorated = None;
        let roots: Vec<PathBuf> = self.repos.iter().map(|r| r.root.clone()).collect();
        let keep = |p: &Path| roots.iter().any(|r| p.starts_with(r));
        match &self.main {
            Some(MainView::Diff(d)) if !keep(&d.change.repo) => self.main = None,
            Some(MainView::Conflict(c)) if !keep(&c.change.repo) => self.main = None,
            Some(MainView::History(h)) if !keep(&h.repo) => self.main = None,
            _ => {}
        }
    }

    /// `.git` folders of repositories whose root is above every project,
    /// so commits and checkouts there are noticed too.
    pub fn extra_watch_dirs(&self) -> Vec<PathBuf> {
        self.repos
            .iter()
            .filter(|r| !self.project_repos.iter().any(|(p, _)| *p == r.root))
            .map(|r| r.root.join(".git"))
            .filter(|g| g.is_dir())
            .collect()
    }

    fn repo_for(&self, path: &Path) -> Option<usize> {
        self.repos
            .iter()
            .enumerate()
            .filter(|(_, r)| path.starts_with(&r.root))
            .max_by_key(|(_, r)| r.root.as_os_str().len())
            .map(|(i, _)| i)
    }

    /// A watcher event: refresh the status of the repository it touched.
    pub fn on_fs_event(&mut self, path: &Path) {
        for r in &mut self.repos {
            if hs_git::event_matters(&r.root, path) {
                r.schedule(DEBOUNCE);
            }
        }
    }

    pub fn refresh_all(&mut self) {
        for r in &mut self.repos {
            r.schedule(Duration::ZERO);
        }
    }

    pub fn has_main(&self) -> bool {
        self.main.is_some()
    }

    pub fn close_main(&mut self) {
        self.main = None;
    }

    pub fn change_count(&self) -> usize {
        self.repos
            .iter()
            .filter_map(|r| r.status.as_ref())
            .map(|s| s.files.len())
            .sum()
    }

    /// Run due refreshes and collect finished results. Call once a frame.
    pub fn poll(&mut self, s: &Services, ctx: &egui::Context) {
        let now = Instant::now();
        for r in &mut self.repos {
            r.ops.poll_lists();
            if let Some((_label, res)) = r.ops.take_result() {
                self.toasts.push((
                    crate::git_ops::level(&res),
                    match res {
                        Ok(t) | Err(t) => t,
                    },
                ));
                // Whatever it did, the status and the lists are stale.
                r.schedule(Duration::ZERO);
                r.ops.lists_due = true;
            }
            if r.ops.lists_due && r.ops.busy.is_none() {
                r.ops.refresh_lists(s, &r.root.clone());
            }
            if let Some(res) = r.pending.take() {
                match res {
                    Ok(st) => {
                        r.status = Some(st);
                        r.error = None;
                    }
                    Err(e) => r.error = Some(e.to_string()),
                }
                r.generation += 1;
                // A merge, rebase or cherry-pick may have ended outside the app.
                r.ops.operation_check_due = true;
                if std::mem::take(&mut r.again) {
                    r.due = Some(now + DEBOUNCE);
                }
            }
            if let Some(due) = r.due
                && !r.pending.busy()
            {
                if due <= now {
                    r.due = None;
                    let root = r.root.clone();
                    r.pending = Pending::start(s, move || hs_git::status(&root));
                } else {
                    ctx.request_repaint_after(due - now);
                }
            }
        }
        let mut resolved = false;
        match &mut self.main {
            Some(MainView::Diff(d)) => d.slot.poll(),
            Some(MainView::Conflict(c)) => {
                if let Some(result) = c.load.take() {
                    match result {
                        Ok(parsed) => {
                            c.choices =
                                vec![hs_git::conflict::Choice::Unresolved; parsed.conflicts()];
                            c.parsed = Some(parsed);
                        }
                        Err(e) => c.error = Some(e),
                    }
                }
                if let Some(result) = c.save.take() {
                    match result {
                        Ok(()) => resolved = true,
                        Err(e) => c.error = Some(e),
                    }
                }
            }
            Some(MainView::History(h)) => h.poll(s),
            None => {}
        }
        if resolved {
            // The file is no longer unmerged: the row moved to Staged.
            self.main = None;
            self.toasts.push((
                hs_core::tool::ToastLevel::Success,
                s.t("git-conflict-resolved"),
            ));
            self.refresh_all();
        }
        // A newer status may have made the shown diff stale.
        if let Some(MainView::Diff(d)) = &self.main
            && let Some(r) = self.repos.iter().find(|r| r.root == d.change.repo)
            && r.generation != d.generation
            && !d.slot.pending.busy()
        {
            let c = d.change.clone();
            self.show_change(c, s);
        }
    }

    // ---- tree --------------------------------------------------------------------

    fn rebuild_decorations(&mut self) {
        self.files.clear();
        self.dirs.clear();
        self.branches.clear();
        for r in &self.repos {
            let Some(st) = &r.status else { continue };
            for f in &st.files {
                let letter = f.letter();
                let tone = Tone::of(letter);
                let p = abs(&r.root, &f.path);
                self.files
                    .insert(p.to_string_lossy().to_string(), (letter, tone));
                let mut dir = p.parent();
                while let Some(d) = dir {
                    if !d.starts_with(&r.root) || d == r.root {
                        break;
                    }
                    let e = self
                        .dirs
                        .entry(d.to_string_lossy().to_string())
                        .or_insert(tone);
                    *e = (*e).max(tone);
                    dir = d.parent();
                }
            }
        }
        for (project, repo) in &self.project_repos {
            if let Some(r) = repo
                .as_ref()
                .and_then(|rp| self.repos.iter().find(|r| &r.root == rp))
                && let Some(st) = &r.status
            {
                self.branches.insert(
                    project.to_string_lossy().to_string(),
                    format!("{} {}", ph::GIT_BRANCH, st.branch_label()),
                );
            }
        }
    }

    /// Put change letters, colours and branch names on the loaded tree.
    pub fn decorate(&mut self, nodes: &mut [TreeNode], theme: &Theme) {
        let gens: Vec<u64> = self.repos.iter().map(|r| r.generation).collect();
        if self.decorated.as_ref() != Some(&gens) {
            self.rebuild_decorations();
            self.decorated = Some(gens);
        }
        self.decorate_nodes(nodes, theme, true);
    }

    fn decorate_nodes(&self, nodes: &mut [TreeNode], theme: &Theme, top: bool) {
        for n in nodes {
            let (badge, tint) = if top {
                (self.branches.get(&n.id).cloned(), None)
            } else if let Some((letter, tone)) = self.files.get(&n.id) {
                (Some(letter.to_string()), Some(tone.colour(theme)))
            } else if let Some(tone) = self.dirs.get(&n.id) {
                (Some("\u{2022}".to_string()), Some(tone.colour(theme)))
            } else {
                (None, None)
            };
            if n.badge != badge {
                n.badge = badge;
            }
            n.tint = tint;
            if let Some(ch) = &mut n.children {
                self.decorate_nodes(ch, theme, false);
            }
        }
    }

    // ---- the Source Control list ------------------------------------------------

    /// The changes of one repository in list order, with the side to diff.
    fn ordered(st: &hs_git::Status) -> [(&'static str, Vec<(&FileStatus, Side)>); 4] {
        let mut conflicts = Vec::new();
        let mut staged = Vec::new();
        let mut changes = Vec::new();
        let mut untracked = Vec::new();
        for f in &st.files {
            match f.kind {
                Kind::Conflict => conflicts.push((f, Side::Unstaged)),
                Kind::Untracked => untracked.push((f, Side::Untracked)),
                Kind::Changed => {
                    if f.staged() {
                        staged.push((f, Side::Staged));
                    }
                    if f.unstaged() {
                        changes.push((f, Side::Unstaged));
                    }
                }
            }
        }
        [
            ("git-conflicts", conflicts),
            ("git-staged", staged),
            ("git-changes", changes),
            ("git-untracked", untracked),
        ]
    }

    /// Every change of every repository in the order the list shows them.
    fn all_changes(&self) -> Vec<ChangeRef> {
        let mut out = Vec::new();
        for r in &self.repos {
            let Some(st) = &r.status else { continue };
            for (_, items) in Self::ordered(st) {
                for (f, side) in items {
                    out.push(ChangeRef {
                        repo: r.root.clone(),
                        path: f.path.clone(),
                        side,
                        conflict: f.kind == Kind::Conflict,
                    });
                }
            }
        }
        out
    }

    fn show_change(&mut self, change: ChangeRef, s: &Services) {
        // An unmerged file opens in the block editor: a diff of a file full
        // of markers says nothing about which side to keep.
        if change.conflict {
            let full = abs(&change.repo, &change.path);
            let load = Pending::start(s, move || {
                let text = std::fs::read_to_string(&full).map_err(|e| e.to_string())?;
                Ok(hs_git::conflict::parse(&text))
            });
            self.main = Some(MainView::Conflict(ConflictPane {
                change,
                load,
                parsed: None,
                choices: Vec::new(),
                save: Pending::none(),
                error: None,
            }));
            return;
        }
        let (root, path, side) = (change.repo.clone(), change.path.clone(), change.side);
        let slot = DiffSlot::load(s, move || {
            hs_git::file_diff(&root, &path, side).map_err(err)
        });
        // Keep the old rows until the new ones arrive (no flicker on refresh).
        let rows = match &mut self.main {
            Some(MainView::Diff(d)) if d.change == change => d.slot.rows.take(),
            _ => None,
        };
        let generation = self
            .repos
            .iter()
            .find(|r| r.root == change.repo)
            .map_or(0, |r| r.generation);
        self.main = Some(MainView::Diff(DiffPane {
            change,
            slot: DiffSlot { rows, ..slot },
            generation,
        }));
    }

    fn selected_change(&self) -> Option<&ChangeRef> {
        match &self.main {
            Some(MainView::Diff(d)) => Some(&d.change),
            _ => None,
        }
    }

    /// Step to the next or previous change in the list.
    fn step_change(&mut self, forward: bool, s: &Services) {
        let all = self.all_changes();
        if all.is_empty() {
            return;
        }
        let at = self
            .selected_change()
            .and_then(|c| all.iter().position(|x| x == c));
        let next = match (at, forward) {
            (None, _) => 0,
            (Some(i), true) => (i + 1).min(all.len() - 1),
            (Some(i), false) => i.saturating_sub(1),
        };
        self.show_change(all[next].clone(), s);
    }

    pub fn open_history_for(&mut self, path: Option<&Path>, s: &Services) -> bool {
        let ri = path
            .and_then(|p| self.repo_for(p))
            .or(if self.repos.is_empty() { None } else { Some(0) });
        let Some(ri) = ri else { return false };
        let root = self.repos[ri].root.clone();
        self.main = Some(MainView::History(HistoryPane::new(root, s)));
        true
    }

    pub fn sidebar_ui(
        &mut self,
        ui: &mut egui::Ui,
        s: &Services,
        theme: &Theme,
        projects: &[(PathBuf, String)],
    ) -> Vec<GitAction> {
        let mut actions = Vec::new();
        let mut show: Option<ChangeRef> = None;
        let mut history: Option<PathBuf> = None;
        let mut refresh = false;
        let mut requests: Vec<(PathBuf, crate::git_ops::Request, bool)> = Vec::new();
        let selected = self.selected_change().cloned();
        for (level, text) in self.toasts.drain(..) {
            actions.push(GitAction::Toast(level, text));
        }
        egui::ScrollArea::vertical()
            .id_salt("git-sidebar")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if projects.is_empty() {
                    ui.label(egui::RichText::new(s.t("editor-no-folder")).weak());
                }
                for r in &mut self.repos {
                    let salt = r.root.clone();
                    ui.push_id(&salt, |ui| {
                        repo_header(ui, s, theme, r, &mut history, &mut refresh);
                        r.ops.ensure_lists(s, &r.root);
                        r.ops.ensure_operation(s, &r.root);
                        if let Some(st) = &r.status {
                            let protected = {
                                let settings = s.settings.read();
                                r.ops.protected(&settings.git, st.branch.as_deref())
                            };
                            for req in crate::git_ops::box_ui(
                                &mut r.ops, ui, s, theme, &r.root, st, protected,
                            ) {
                                requests.push((r.root.clone(), req, protected));
                            }
                        }
                        if let Some(e) = &r.error {
                            let text = if e.contains("not installed") {
                                s.t("git-not-installed")
                            } else {
                                e.clone()
                            };
                            ui.label(egui::RichText::new(text).color(theme.error).small());
                        }
                        let Some(st) = &r.status else {
                            if r.error.is_none() {
                                ui.label(egui::RichText::new(s.t("git-loading")).weak());
                            }
                            return;
                        };
                        if st.files.is_empty() {
                            ui.label(egui::RichText::new(s.t("git-no-changes")).weak());
                        }
                        for (key, items) in Self::ordered(st) {
                            if items.is_empty() {
                                continue;
                            }
                            let group = egui::CollapsingHeader::new(
                                egui::RichText::new(format!("{} ({})", s.t(key), items.len()))
                                    .strong(),
                            )
                            .id_salt(key)
                            .default_open(true)
                            .show(ui, |ui| {
                                for (f, side) in items {
                                    let change = ChangeRef {
                                        repo: r.root.clone(),
                                        path: f.path.clone(),
                                        side,
                                        conflict: f.kind == Kind::Conflict,
                                    };
                                    let letter = match side {
                                        Side::Staged => f.index,
                                        Side::Unstaged if f.kind == Kind::Changed => f.worktree,
                                        _ => f.letter(),
                                    };
                                    let full = abs(&r.root, &f.path);
                                    let resp = change_row(
                                        ui,
                                        theme,
                                        f,
                                        letter,
                                        selected.as_ref() == Some(&change),
                                    );
                                    if resp.clicked() {
                                        show = Some(change.clone());
                                    }
                                    if resp.double_clicked() && full.is_file() {
                                        actions.push(GitAction::OpenFile(full.clone()));
                                    }
                                    let path = f.path.clone();
                                    let root = r.root.clone();
                                    let untracked = f.kind == Kind::Untracked;
                                    resp.context_menu(|ui| {
                                        // The write side first: that is what a
                                        // right-click on a change is for.
                                        let conflicted = f.kind == Kind::Conflict;
                                        if conflicted {
                                            // A conflict cannot be staged as-is:
                                            // pick a side or say it is resolved.
                                            ui.label(
                                                egui::RichText::new(s.t("git-conflict-heading"))
                                                    .small()
                                                    .color(theme.warning),
                                            );
                                            if ui.button(s.t("git-keep-ours")).clicked() {
                                                requests.push((
                                                    root.clone(),
                                                    crate::git_ops::Request::ResolveConflict {
                                                        path: path.clone(),
                                                        side: hs_git::write::ConflictSide::Ours,
                                                    },
                                                    false,
                                                ));
                                                ui.close();
                                            }
                                            if ui.button(s.t("git-keep-theirs")).clicked() {
                                                requests.push((
                                                    root.clone(),
                                                    crate::git_ops::Request::ResolveConflict {
                                                        path: path.clone(),
                                                        side: hs_git::write::ConflictSide::Theirs,
                                                    },
                                                    false,
                                                ));
                                                ui.close();
                                            }
                                            if ui.button(s.t("git-mark-resolved")).clicked() {
                                                requests.push((
                                                    root.clone(),
                                                    crate::git_ops::Request::Stage(vec![
                                                        path.clone(),
                                                    ]),
                                                    false,
                                                ));
                                                ui.close();
                                            }
                                            ui.separator();
                                        } else if side == Side::Staged {
                                            if ui.button(s.t("git-unstage")).clicked() {
                                                requests.push((
                                                    root.clone(),
                                                    crate::git_ops::Request::Unstage(vec![
                                                        path.clone(),
                                                    ]),
                                                    false,
                                                ));
                                                ui.close();
                                            }
                                        } else {
                                            if ui.button(s.t("git-stage")).clicked() {
                                                requests.push((
                                                    root.clone(),
                                                    crate::git_ops::Request::Stage(vec![
                                                        path.clone(),
                                                    ]),
                                                    false,
                                                ));
                                                ui.close();
                                            }
                                            if untracked {
                                                if ui.button(s.t("git-delete-untracked")).clicked()
                                                {
                                                    r.ops.confirm =
                                                        Some(crate::git_ops::Confirm::Delete(
                                                            vec![path.clone()],
                                                        ));
                                                    ui.close();
                                                }
                                            } else if ui.button(s.t("git-discard")).clicked() {
                                                r.ops.confirm =
                                                    Some(crate::git_ops::Confirm::Discard(vec![
                                                        path.clone(),
                                                    ]));
                                                ui.close();
                                            }
                                        }
                                        ui.separator();
                                        if ui.button(s.t("git-open-changes")).clicked() {
                                            show = Some(change.clone());
                                            ui.close();
                                        }
                                        if full.is_file()
                                            && ui.button(s.t("git-open-file")).clicked()
                                        {
                                            actions.push(GitAction::OpenFile(full.clone()));
                                            ui.close();
                                        }
                                        if ui.button(s.t("git-copy-path")).clicked() {
                                            actions.push(GitAction::CopyPath(
                                                full.to_string_lossy().to_string(),
                                            ));
                                            ui.close();
                                        }
                                    });
                                }
                            });
                            // Stage All / Unstage All on the group's header.
                            let all_key = match key {
                                "git-staged" => Some(("git-unstage-all", true)),
                                "git-changes" | "git-untracked" => Some(("git-stage-all", false)),
                                _ => None,
                            };
                            if let Some((label, unstage)) = all_key {
                                group.header_response.context_menu(|ui| {
                                    if ui.button(s.t(label)).clicked() {
                                        requests.push((
                                            r.root.clone(),
                                            if unstage {
                                                crate::git_ops::Request::UnstageAll
                                            } else {
                                                crate::git_ops::Request::StageAll
                                            },
                                            false,
                                        ));
                                        ui.close();
                                    }
                                });
                            }
                        }
                    });
                    ui.add_space(8.0);
                }
                let outside: Vec<&String> = projects
                    .iter()
                    .filter(|(root, _)| {
                        self.project_repos
                            .iter()
                            .any(|(p, r)| p == root && r.is_none())
                    })
                    .map(|(_, name)| name)
                    .collect();
                for name in outside {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(ph::FOLDER_SIMPLE).color(theme.text_muted));
                        ui.label(egui::RichText::new(name).strong());
                    });
                    ui.label(egui::RichText::new(s.t("git-not-repo")).weak());
                    ui.add_space(8.0);
                }
            });
        // A question first, where one is due; then whatever was asked.
        let mut pending_push = self.pending_push.take();
        for r in &mut self.repos {
            if let Some(req) =
                crate::git_ops::confirm_ui(&mut r.ops, ui, s, theme, &mut pending_push)
            {
                requests.push((r.root.clone(), req, false));
            }
        }
        self.pending_push = pending_push;
        for (root, req, protected) in requests {
            let Some(r) = self.repos.iter_mut().find(|r| r.root == root) else {
                continue;
            };
            let Some(st) = r.status.clone() else { continue };
            // A push to a protected remote asks once more; a plain
            // --force is never issued, so that is all the guard needs.
            if protected && matches!(req, crate::git_ops::Request::Push) {
                self.pending_push = Some(req);
                r.ops.confirm = Some(crate::git_ops::Confirm::ProtectedPush);
                continue;
            }
            crate::git_ops::run(&mut r.ops, s, &root, &st, req);
        }
        if refresh {
            self.refresh_all();
        }
        if let Some(root) = history {
            self.main = Some(MainView::History(HistoryPane::new(root, s)));
        }
        if let Some(c) = show {
            self.show_change(c, s);
        }
        actions
    }

    // ---- main area -----------------------------------------------------------

    /// Draw the diff or history in the main area.
    pub fn main_ui(
        &mut self,
        ui: &mut egui::Ui,
        s: &Services,
        theme: &Theme,
        font_size: f32,
    ) -> Vec<GitAction> {
        let mut actions = Vec::new();
        let keys_free = !ui.ctx().egui_wants_keyboard_input();
        if keys_free && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.main = None;
            return actions;
        }
        let mut back = false;
        let mut reload_after: Option<ChangeRef> = None;
        let mut side_by_side = self.side_by_side;
        // A hunk operation asked for in the diff view, run after the match
        // so the repository can be borrowed mutably.
        let mut hunk_request: Option<(PathBuf, String, hs_git::Hunk, hs_git::write::HunkOp)> = None;
        match &mut self.main {
            Some(MainView::Diff(d)) => {
                let full = abs(&d.change.repo, &d.change.path);
                let side_key = match d.change.side {
                    Side::Staged => "git-staged",
                    Side::Unstaged => "git-changes",
                    Side::Untracked => "git-untracked",
                };
                let counts = match &d.slot.rows {
                    Some(Ok(r)) => Some((r.added, r.removed)),
                    _ => None,
                };
                let mut open = false;
                header(
                    ui,
                    s,
                    theme,
                    &mut back,
                    |ui| {
                        ui.label(egui::RichText::new(&d.change.path).strong());
                        ui.label(egui::RichText::new(s.t(side_key)).color(theme.text_muted));
                        counts_label(ui, theme, counts);
                    },
                    |ui| {
                        if full.is_file() && ui.button(s.t("git-open-file")).clicked() {
                            open = true;
                        }
                        mode_toggle(ui, s, &mut side_by_side);
                    },
                );
                if open {
                    actions.push(GitAction::OpenFile(full.clone()));
                }
                let mut hunk_op: Option<(usize, hs_git::write::HunkOp)> = None;
                diff_body(
                    ui,
                    s,
                    theme,
                    &d.slot,
                    side_by_side,
                    font_size,
                    "git-diff",
                    Some(d.change.side),
                    &mut hunk_op,
                );
                if keys_free && ui.input(|i| i.key_pressed(egui::Key::Enter)) && full.is_file() {
                    actions.push(GitAction::OpenFile(full));
                }
                if let Some((hi, op)) = hunk_op
                    && let Some(diff) = &d.slot.diff
                    && let Some(hunk) = diff.hunks.get(hi).cloned()
                {
                    hunk_request = Some((d.change.repo.clone(), d.change.path.clone(), hunk, op));
                }
            }
            Some(MainView::Conflict(c)) => {
                let full = abs(&c.change.repo, &c.change.path);
                let mut open = false;
                let mut reload = false;
                let mut save_now = false;
                let total = c.parsed.as_ref().map_or(0, |p| p.conflicts());
                let chosen = c
                    .choices
                    .iter()
                    .filter(|x| **x != hs_git::conflict::Choice::Unresolved)
                    .count();
                header(
                    ui,
                    s,
                    theme,
                    &mut back,
                    |ui| {
                        ui.label(egui::RichText::new(&c.change.path).strong());
                        ui.label(
                            egui::RichText::new(s.t("git-conflict-heading")).color(theme.warning),
                        );
                        if total > 0 {
                            ui.label(
                                egui::RichText::new(format!("{chosen}/{total}"))
                                    .small()
                                    .color(theme.text_muted),
                            );
                        }
                    },
                    |ui| {
                        if full.is_file() && ui.button(s.t("git-open-file")).clicked() {
                            open = true;
                        }
                        if ui.button(s.t("git-refresh")).clicked() {
                            reload = true;
                        }
                    },
                );
                if open {
                    actions.push(GitAction::OpenFile(full.clone()));
                }
                if reload {
                    reload_after = Some(c.change.clone());
                }
                if let Some(e) = &c.error {
                    ui.colored_label(theme.error, e);
                }
                if c.parsed.is_none() {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(s.t("git-conflict-loading"));
                    });
                } else {
                    let ready = total > 0 && chosen == total;
                    ui.horizontal(|ui| {
                        if ui.button(s.t("git-conflict-all-ours")).clicked() {
                            c.choices.fill(hs_git::conflict::Choice::Ours);
                        }
                        if ui.button(s.t("git-conflict-all-theirs")).clicked() {
                            c.choices.fill(hs_git::conflict::Choice::Theirs);
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .add_enabled(
                                    ready && !c.save.busy(),
                                    egui::Button::new(s.t("git-conflict-save")),
                                )
                                .clicked()
                            {
                                save_now = true;
                            }
                        });
                    });
                    ui.separator();
                    egui::ScrollArea::vertical()
                        .id_salt(("git-conflict", &c.change.path))
                        .show(ui, |ui| {
                            if let Some(parsed) = &c.parsed {
                                let mut index = 0;
                                for segment in &parsed.segments {
                                    match segment {
                                        hs_git::conflict::Segment::Context(text) => {
                                            conflict_context(ui, theme, text);
                                        }
                                        hs_git::conflict::Segment::Conflict(block) => {
                                            if let Some(choice) = c.choices.get_mut(index) {
                                                conflict_block(ui, s, theme, block, choice);
                                            }
                                            index += 1;
                                        }
                                    }
                                }
                            }
                        });
                }
                if save_now && let Some(parsed) = &c.parsed {
                    match hs_git::conflict::resolve(parsed, &c.choices) {
                        Ok(text) => {
                            let root = c.change.repo.clone();
                            let path = c.change.path.clone();
                            let full = abs(&root, &path);
                            c.save = Pending::start(s, move || {
                                std::fs::write(&full, text).map_err(|e| e.to_string())?;
                                hs_git::write::stage(&root, &[path.as_str()]).map_err(err)
                            });
                            c.error = None;
                        }
                        Err(hs_git::conflict::ResolveError::Unresolved(index)) => {
                            c.error = Some(s.i18n.t_args(
                                "git-conflict-unresolved-index",
                                &[("index", &(index + 1).to_string())],
                            ));
                        }
                    }
                }
            }
            Some(MainView::History(h)) => {
                let name = name_of(&h.repo);
                let branch = self
                    .repos
                    .iter()
                    .find(|r| r.root == h.repo)
                    .and_then(|r| r.status.as_ref())
                    .map(|st| st.branch_label());
                let mut reload = false;
                header(
                    ui,
                    s,
                    theme,
                    &mut back,
                    |ui| {
                        ui.label(
                            egui::RichText::new(format!(
                                "{} {}",
                                ph::CLOCK_COUNTER_CLOCKWISE,
                                s.i18n.t_args("git-history-of", &[("name", &name)])
                            ))
                            .strong(),
                        );
                        if let Some(b) = &branch {
                            ui.label(
                                egui::RichText::new(format!("{} {b}", ph::GIT_BRANCH))
                                    .color(theme.text_muted),
                            );
                        }
                    },
                    |ui| {
                        if ui
                            .add(|ui: &mut egui::Ui| {
                                hs_ui::form::icon_button(
                                    ui,
                                    theme,
                                    ph::ARROW_CLOCKWISE,
                                    16.0,
                                    false,
                                )
                            })
                            .tip(s.t("git-refresh"))
                            .clicked()
                        {
                            reload = true;
                        }
                        mode_toggle(ui, s, &mut side_by_side);
                    },
                );
                if reload {
                    *h = HistoryPane::new(h.repo.clone(), s);
                }
                history_body(
                    ui,
                    s,
                    theme,
                    h,
                    side_by_side,
                    font_size,
                    keys_free,
                    &mut actions,
                );
            }
            None => {}
        }
        self.side_by_side = side_by_side;
        if let Some((repo, path, hunk, op)) = hunk_request
            && let Some(r) = self.repos.iter_mut().find(|r| r.root == repo)
        {
            if op == hs_git::write::HunkOp::Discard {
                r.ops.confirm = Some(crate::git_ops::Confirm::DiscardHunk { path, hunk });
            } else if let Some(st) = r.status.clone() {
                crate::git_ops::run(
                    &mut r.ops,
                    s,
                    &repo,
                    &st,
                    crate::git_ops::Request::Hunk { path, hunk, op },
                );
            }
        }
        if let Some(change) = reload_after {
            self.show_change(change, s);
        }
        if back {
            self.main = None;
        } else if keys_free && matches!(self.main, Some(MainView::Diff(_))) {
            let (up, down) = ui.input(|i| {
                (
                    i.key_pressed(egui::Key::ArrowUp),
                    i.key_pressed(egui::Key::ArrowDown),
                )
            });
            if up || down {
                self.step_change(down, s);
            }
        }
        actions
    }

    // ---- gutter markers -----------------------------------------------------

    /// Keep the gutter markers of the active file current. Returns new
    /// markers when they changed.
    pub fn update_marks(
        &mut self,
        g: &mut DocGit,
        path: Option<&Path>,
        buf: &Buffer,
        s: &Services,
        ctx: &egui::Context,
    ) -> Option<Vec<Option<LineMark>>> {
        let mut out = None;
        if let Some(p) = &mut g.marks_pending
            && let Some(m) = p.take()
        {
            g.marks_pending = None;
            g.has_marks = m.iter().any(Option::is_some);
            out = Some(to_marks(m));
        }
        if let Some(p) = &mut g.base_pending
            && let Some(r) = p.take()
        {
            g.base_pending = None;
            g.base = Some(match r {
                Ok(Some(t)) => Base::Text(Arc::new(t)),
                Ok(None) => g.base.clone().unwrap_or(Base::None),
                Err(_) => Base::None,
            });
            g.marks_for = None;
        }
        let clear = |g: &mut DocGit| {
            let had = g.has_marks;
            *g = DocGit::default();
            had.then(Vec::new)
        };
        let Some(path) = path else {
            return clear(g).or(out);
        };
        let Some(ri) = self.repo_for(path) else {
            return clear(g).or(out);
        };
        let repo = &self.repos[ri];
        let Some(st) = &repo.status else { return out };
        let key = (repo.root.clone(), repo.generation);
        if g.key.as_ref() != Some(&key) {
            g.key = Some(key);
            let rp = rel(&repo.root, path).unwrap_or_default();
            match st.files.iter().find(|f| f.path == rp) {
                Some(f) if f.kind == Kind::Untracked => {
                    g.base = Some(Base::None);
                    g.marks_for = None;
                }
                listed => {
                    // A staged new file is "all added" if HEAD lacks it;
                    // an unlisted one is clean or ignored.
                    let fallback = match listed {
                        Some(f) if f.index == 'A' => Base::NewFile,
                        _ => Base::None,
                    };
                    g.base = Some(fallback);
                    let root = repo.root.clone();
                    g.base_pending = Some(Pending::start(s, move || {
                        hs_git::head_text(&root, &rp).map_err(err)
                    }));
                }
            }
        }
        if buf.version() != g.seen_version {
            g.seen_version = buf.version();
            g.changed_at = Some(Instant::now());
        }
        let base = match &g.base {
            Some(Base::None) => {
                if g.has_marks {
                    g.has_marks = false;
                    return Some(Vec::new());
                }
                return out;
            }
            Some(b) if g.base_pending.is_none() => b.clone(),
            _ => return out,
        };
        if g.marks_for == Some(buf.version()) || g.marks_pending.is_some() {
            return out;
        }
        if buf.len_chars() > MARKS_MAX_CHARS {
            return out;
        }
        if let Some(t) = g.changed_at
            && g.marks_for.is_some()
            && t.elapsed() < MARKS_DEBOUNCE
        {
            ctx.request_repaint_after(MARKS_DEBOUNCE - t.elapsed());
            return out;
        }
        g.marks_for = Some(buf.version());
        let text = buf.text();
        g.marks_pending = Some(Pending::start(s, move || match base {
            Base::Text(old) => hs_git::line_changes(Some(&old), &text),
            _ => hs_git::line_changes(None, &text),
        }));
        out
    }
}

// ---- drawing helpers ------------------------------------------------------------

fn repo_header(
    ui: &mut egui::Ui,
    s: &Services,
    theme: &Theme,
    r: &Repo,
    history: &mut Option<PathBuf>,
    refresh: &mut bool,
) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(ph::GIT_BRANCH).color(theme.accent));
        ui.label(egui::RichText::new(r.name()).strong())
            .tip(r.root.display().to_string());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add(|ui: &mut egui::Ui| {
                    hs_ui::form::icon_button(ui, theme, ph::ARROW_CLOCKWISE, 15.0, false)
                })
                .tip(s.t("git-refresh"))
                .clicked()
            {
                *refresh = true;
            }
            if ui
                .add(|ui: &mut egui::Ui| {
                    hs_ui::form::icon_button(ui, theme, ph::CLOCK_COUNTER_CLOCKWISE, 15.0, false)
                })
                .tip(s.t("git-history-tip"))
                .clicked()
            {
                *history = Some(r.root.clone());
            }
            if let Some(st) = &r.status {
                let tip = st.upstream.as_ref().map(|u| {
                    s.i18n.t_args(
                        "git-upstream-tip",
                        &[
                            ("upstream", u),
                            ("ahead", &st.ahead.to_string()),
                            ("behind", &st.behind.to_string()),
                        ],
                    )
                });
                let l = ui.add(
                    egui::Label::new(
                        egui::RichText::new(st.branch_label()).color(theme.text_muted),
                    )
                    .truncate()
                    .selectable(false),
                );
                if let Some(t) = tip {
                    l.tip(t);
                }
            }
        });
    });
}

/// Shared lines around a conflict, capped so a huge file stays responsive.
fn conflict_context(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    let lines: Vec<&str> = text.lines().collect();
    let head = 10;
    let tail = 5;
    if lines.len() > head + tail + 1 {
        for line in &lines[..head] {
            ui.monospace(*line);
        }
        ui.label(
            egui::RichText::new(format!("… {} lines …", lines.len() - head - tail))
                .small()
                .color(theme.text_muted),
        );
        for line in &lines[lines.len() - tail..] {
            ui.monospace(*line);
        }
    } else {
        for line in lines {
            ui.monospace(line);
        }
    }
}

/// One side of a conflict, with its first lines and a count for the rest.
fn conflict_side(ui: &mut egui::Ui, theme: &Theme, text: &str, empty: &str) {
    if text.is_empty() {
        ui.label(egui::RichText::new(empty).italics().color(theme.text_muted));
        return;
    }
    let lines: Vec<&str> = text.lines().collect();
    let cap = 24;
    for line in lines.iter().take(cap) {
        ui.monospace(*line);
    }
    if lines.len() > cap {
        ui.label(
            egui::RichText::new(format!("… {} more …", lines.len() - cap))
                .small()
                .color(theme.text_muted),
        );
    }
}

/// One conflict block: both sides under their marker labels, a choice row,
/// and a text field when the result is typed.
fn conflict_block(
    ui: &mut egui::Ui,
    s: &Services,
    theme: &Theme,
    block: &hs_git::conflict::Conflict,
    choice: &mut hs_git::conflict::Choice,
) {
    use hs_git::conflict::Choice;
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.label(
            egui::RichText::new(format!(
                "{} · {}",
                s.t("git-conflict-ours"),
                block.ours_label
            ))
            .strong()
            .color(theme.warning),
        );
        conflict_side(ui, theme, &block.ours, &s.t("git-conflict-empty"));
        ui.add_space(6.0);
        ui.label(
            egui::RichText::new(format!(
                "{} · {}",
                s.t("git-conflict-theirs"),
                block.theirs_label
            ))
            .strong()
            .color(theme.accent),
        );
        conflict_side(ui, theme, &block.theirs, &s.t("git-conflict-empty"));
        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            let mut pick = |ui: &mut egui::Ui, value: Choice, key: &str| {
                if ui.selectable_label(*choice == value, s.t(key)).clicked() {
                    *choice = value.clone();
                }
            };
            pick(ui, Choice::Ours, "git-conflict-ours");
            pick(ui, Choice::Theirs, "git-conflict-theirs");
            pick(ui, Choice::Both, "git-conflict-both");
            if ui
                .selectable_label(
                    matches!(choice, Choice::Custom(_)),
                    s.t("git-conflict-custom"),
                )
                .clicked()
            {
                *choice = Choice::Custom(block.ours.clone());
            }
            if matches!(choice, Choice::Unresolved) {
                ui.label(
                    egui::RichText::new(s.t("git-conflict-unresolved"))
                        .small()
                        .color(theme.warning),
                );
            }
        });
        if let Choice::Custom(text) = choice {
            ui.add(
                egui::TextEdit::multiline(text)
                    .code_editor()
                    .desired_rows(4)
                    .desired_width(f32::INFINITY),
            );
        }
    });
}

/// One file in the Source Control list: icon, name, folder, letter.
fn change_row(
    ui: &mut egui::Ui,
    theme: &Theme,
    f: &FileStatus,
    letter: char,
    selected: bool,
) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW), Sense::click());
    let (dir, name) = match f.path.rsplit_once('/') {
        Some((d, n)) => (d.to_string(), n.to_string()),
        None => (String::new(), f.path.clone()),
    };
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &f.path));
    if selected {
        ui.painter().rect_filled(
            rect,
            CornerRadius::same(3),
            theme.accent.linear_multiply(0.3),
        );
    } else if resp.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(3), theme.surface_hover);
    }
    let tone = Tone::of(letter);
    let p = ui.painter();
    let icon_rect = egui::Rect::from_center_size(
        egui::pos2(rect.left() + 14.0, rect.center().y),
        Vec2::splat(16.0),
    );
    egui::Image::new(fileicons::for_file(&name))
        .fit_to_exact_size(Vec2::splat(16.0))
        .paint_at(ui, icon_rect);
    let letter_w = 18.0;
    let clip = egui::Rect::from_min_max(rect.min, egui::pos2(rect.right() - letter_w, rect.max.y));
    let p = p.with_clip_rect(clip);
    let name_colour = if tone == Tone::Deleted {
        theme.text_muted
    } else {
        theme.text
    };
    let name_rect = p.text(
        egui::pos2(rect.left() + 28.0, rect.center().y),
        Align2::LEFT_CENTER,
        &name,
        egui::FontId::proportional(13.0),
        name_colour,
    );
    if tone == Tone::Deleted {
        p.hline(
            name_rect.x_range(),
            name_rect.center().y,
            egui::Stroke::new(1.0, theme.text_muted),
        );
    }
    if !dir.is_empty() {
        p.text(
            egui::pos2(name_rect.right() + 6.0, rect.center().y),
            Align2::LEFT_CENTER,
            &dir,
            egui::FontId::proportional(11.0),
            theme.text_muted,
        );
    }
    ui.painter().text(
        egui::pos2(rect.right() - 6.0, rect.center().y),
        Align2::RIGHT_CENTER,
        letter.to_string(),
        egui::FontId::monospace(12.0),
        tone.colour(theme),
    );
    let tip = match &f.orig_path {
        Some(o) => format!("{}\n\u{2190} {o}", f.path),
        None => f.path.clone(),
    };
    resp.tip(tip)
}

fn header(
    ui: &mut egui::Ui,
    s: &Services,
    theme: &Theme,
    back: &mut bool,
    left: impl FnOnce(&mut egui::Ui),
    right: impl FnOnce(&mut egui::Ui),
) {
    hs_ui::form::header_bar(ui, theme, |ui| {
        if ui
            .button(format!("{} {}", ph::ARROW_LEFT, s.t("git-back")))
            .tip(s.t("git-back-tip"))
            .clicked()
        {
            *back = true;
        }
        ui.separator();
        left(ui);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), right);
    });
}

fn mode_toggle(ui: &mut egui::Ui, s: &Services, side_by_side: &mut bool) {
    ui.selectable_value(
        side_by_side,
        true,
        format!("{} {}", ph::COLUMNS, s.t("git-side-by-side")),
    );
    ui.selectable_value(
        side_by_side,
        false,
        format!("{} {}", ph::ROWS, s.t("git-inline")),
    );
}

fn counts_label(ui: &mut egui::Ui, theme: &Theme, counts: Option<(usize, usize)>) {
    if let Some((a, r)) = counts {
        ui.label(
            egui::RichText::new(format!("+{a}"))
                .color(theme.success)
                .monospace(),
        );
        ui.label(
            egui::RichText::new(format!("\u{2212}{r}"))
                .color(theme.error)
                .monospace(),
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn diff_body(
    ui: &mut egui::Ui,
    s: &Services,
    theme: &Theme,
    slot: &DiffSlot,
    side_by_side: bool,
    font_size: f32,
    salt: &str,
    // Where the file sits, for the per-hunk buttons; `None` for a diff
    // nothing can be done to (a commit's).
    side: Option<Side>,
    hunk_op: &mut Option<(usize, hs_git::write::HunkOp)>,
) {
    match &slot.rows {
        None => {
            ui.add_space(8.0);
            ui.label(egui::RichText::new(s.t("git-loading")).weak());
        }
        Some(Err(e)) => {
            ui.add_space(8.0);
            ui.label(egui::RichText::new(e).color(theme.error));
        }
        Some(Ok(rows)) if rows.binary => {
            ui.add_space(8.0);
            ui.label(egui::RichText::new(s.t("git-binary")).weak());
        }
        Some(Ok(rows)) if rows.inline.is_empty() => {
            ui.add_space(8.0);
            ui.label(egui::RichText::new(s.t("git-no-diff")).weak());
        }
        Some(Ok(rows)) => {
            // What one hunk can be asked to do, by where the file sits.
            let ops: &[(&str, hs_git::write::HunkOp)] = match side {
                Some(Side::Staged) => &[("git-unstage-hunk", hs_git::write::HunkOp::Unstage)],
                Some(Side::Unstaged) => &[
                    ("git-stage-hunk", hs_git::write::HunkOp::Stage),
                    ("git-discard-hunk", hs_git::write::HunkOp::Discard),
                ],
                Some(Side::Untracked) | None => &[],
            };
            let labels: Vec<(String, hs_git::write::HunkOp)> =
                ops.iter().map(|(k, op)| (s.t(k), *op)).collect();
            draw_diff(
                ui,
                theme,
                rows,
                side_by_side,
                font_size,
                salt,
                &labels,
                hunk_op,
            )
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_diff(
    ui: &mut egui::Ui,
    theme: &Theme,
    rows: &DiffRows,
    side_by_side: bool,
    font_size: f32,
    salt: &str,
    hunk_buttons: &[(String, hs_git::write::HunkOp)],
    hunk_op: &mut Option<(usize, hs_git::write::HunkOp)>,
) {
    let font = egui::FontId::new(font_size, hs_ui::fonts::editor_family());
    let cw = ui.fonts_mut(|f| f.glyph_width(&font, '0')).max(1.0);
    let row_h = (font_size * 1.45).round().max(16.0);
    let num_w = (rows.digits as f32 + 1.0) * cw;
    let text_w = (rows.width as f32 + 2.0) * cw;
    let bg = |k: LineKind| match k {
        LineKind::Added => Some(theme.success.linear_multiply(0.16)),
        LineKind::Removed => Some(theme.error.linear_multiply(0.16)),
        _ => None,
    };
    let avail = ui.available_width();
    let n = if side_by_side {
        rows.split.len()
    } else {
        rows.inline.len()
    };
    let half_w = (avail / 2.0).max(num_w + 8.0 + text_w);
    let total_w = if side_by_side {
        half_w * 2.0
    } else {
        (2.0 * num_w + 2.0 * cw + 12.0 + text_w).max(avail)
    };
    let surface = theme.surface;
    egui::ScrollArea::both()
        .id_salt(salt)
        .auto_shrink([false, false])
        .show_rows(ui, row_h, n, |ui, range| {
            ui.set_min_width(total_w);
            for i in range {
                let (rect, _) = ui.allocate_exact_size(Vec2::new(total_w, row_h), Sense::hover());
                let p = ui.painter();
                let y = rect.center().y;
                let banner = |text: &str, colour: Color32| {
                    p.rect_filled(rect, CornerRadius::ZERO, surface);
                    p.text(
                        egui::pos2(rect.left() + 8.0, y),
                        Align2::LEFT_CENTER,
                        text,
                        font.clone(),
                        colour,
                    );
                };
                // The hunk's buttons sit at the right end of its banner, as
                // real widgets placed over the painted row.
                let hunk_row = if side_by_side {
                    match &rows.split[i] {
                        SplitRow::Hunk(hi, _) => Some(*hi),
                        _ => None,
                    }
                } else {
                    match &rows.inline[i] {
                        Row::Hunk(hi, _) => Some(*hi),
                        _ => None,
                    }
                };
                if side_by_side {
                    match &rows.split[i] {
                        SplitRow::Note(t) => banner(t, theme.text_muted),
                        SplitRow::Hunk(_, t) => banner(t, theme.accent),
                        SplitRow::Pair(l, r) => {
                            for (k, half) in [(0.0, l), (1.0, r)] {
                                let x0 = rect.left() + k * half_w;
                                let hr = egui::Rect::from_min_size(
                                    egui::pos2(x0, rect.top()),
                                    Vec2::new(half_w, row_h),
                                );
                                let Some(h) = half else {
                                    p.rect_filled(
                                        hr,
                                        CornerRadius::ZERO,
                                        surface.linear_multiply(0.5),
                                    );
                                    continue;
                                };
                                if let Some(c) = bg(h.kind) {
                                    p.rect_filled(hr, CornerRadius::ZERO, c);
                                }
                                let p = p.with_clip_rect(hr.intersect(p.clip_rect()));
                                if let Some(no) = h.no {
                                    p.text(
                                        egui::pos2(x0 + num_w, y),
                                        Align2::RIGHT_CENTER,
                                        no.to_string(),
                                        font.clone(),
                                        theme.text_muted,
                                    );
                                }
                                p.text(
                                    egui::pos2(x0 + num_w + 8.0, y),
                                    Align2::LEFT_CENTER,
                                    &h.text,
                                    font.clone(),
                                    theme.text,
                                );
                            }
                            p.vline(
                                rect.left() + half_w,
                                rect.y_range(),
                                egui::Stroke::new(1.0, theme.border),
                            );
                        }
                    }
                } else {
                    match &rows.inline[i] {
                        Row::Note(t) => banner(t, theme.text_muted),
                        Row::Hunk(_, t) => banner(t, theme.accent),
                        Row::Line {
                            kind,
                            old,
                            new,
                            text,
                        } => {
                            if let Some(c) = bg(*kind) {
                                p.rect_filled(rect, CornerRadius::ZERO, c);
                            }
                            let x = rect.left();
                            for (no, right) in [(old, x + num_w), (new, x + 2.0 * num_w + 4.0)] {
                                if let Some(no) = no {
                                    p.text(
                                        egui::pos2(right, y),
                                        Align2::RIGHT_CENTER,
                                        no.to_string(),
                                        font.clone(),
                                        theme.text_muted,
                                    );
                                }
                            }
                            let sign = match kind {
                                LineKind::Added => "+",
                                LineKind::Removed => "-",
                                _ => " ",
                            };
                            let tx = x + 2.0 * num_w + 12.0;
                            p.text(
                                egui::pos2(tx, y),
                                Align2::LEFT_CENTER,
                                sign,
                                font.clone(),
                                theme.text_muted,
                            );
                            let colour = if *kind == LineKind::NoNewline {
                                theme.text_muted
                            } else {
                                theme.text
                            };
                            p.text(
                                egui::pos2(tx + 2.0 * cw, y),
                                Align2::LEFT_CENTER,
                                text,
                                font.clone(),
                                colour,
                            );
                        }
                    }
                }
                if let Some(hi) = hunk_row
                    && !hunk_buttons.is_empty()
                {
                    // Right-aligned in the visible part of the banner, so
                    // they stay in view however wide the diff is.
                    let visible_right = ui.clip_rect().right().min(rect.right()) - 6.0;
                    let mut x = visible_right;
                    for (label, op) in hunk_buttons.iter().rev() {
                        let w = (label.chars().count() as f32 * font_size * 0.6 + 16.0).max(60.0);
                        let brect = egui::Rect::from_min_max(
                            egui::pos2(x - w, rect.top() + 1.0),
                            egui::pos2(x, rect.bottom() - 1.0),
                        );
                        let r = ui.put(
                            brect,
                            egui::Button::new(egui::RichText::new(label).size(font_size * 0.9)),
                        );
                        if r.clicked() {
                            *hunk_op = Some((hi, *op));
                        }
                        x -= w + 4.0;
                    }
                }
            }
        });
}

fn local_time(secs: i64) -> String {
    use chrono::TimeZone;
    match chrono::Local.timestamp_opt(secs, 0) {
        chrono::LocalResult::Single(t) | chrono::LocalResult::Ambiguous(t, _) => {
            t.format("%Y-%m-%d %H:%M").to_string()
        }
        chrono::LocalResult::None => String::new(),
    }
}

#[allow(clippy::too_many_arguments)] // one call site; splitting it only moves the list
fn history_body(
    ui: &mut egui::Ui,
    s: &Services,
    theme: &Theme,
    h: &mut HistoryPane,
    side_by_side: bool,
    font_size: f32,
    keys_free: bool,
    actions: &mut Vec<GitAction>,
) {
    if let Some(e) = &h.error {
        ui.label(egui::RichText::new(e).color(theme.error));
    }
    if h.commits.is_empty() {
        let key = if h.pending.busy() {
            "git-loading"
        } else {
            "git-no-commits"
        };
        ui.label(egui::RichText::new(s.t(key)).weak());
        return;
    }
    if keys_free {
        let (up, down) = ui.input(|i| {
            (
                i.key_pressed(egui::Key::ArrowUp),
                i.key_pressed(egui::Key::ArrowDown),
            )
        });
        if let Some(cur) = h.selected
            && (up || down)
        {
            let next = if down {
                (cur + 1).min(h.commits.len() - 1)
            } else {
                cur.saturating_sub(1)
            };
            h.select(next, s);
            h.scroll_to_selected = true;
        }
    }
    let height = ui.available_height();
    egui::Panel::top(egui::Id::new(("git-history-list", &h.repo)))
        .default_size((height * 0.42).max(120.0))
        .size_range(80.0..=(height - 120.0).max(80.0))
        .resizable(true)
        .show(ui, |ui| commit_list(ui, s, theme, h));
    let Some(ci) = h.selected else { return };
    let Some(c) = h.commits.get(ci).cloned() else {
        return;
    };
    egui::Panel::left(egui::Id::new(("git-history-files", &h.repo)))
        .default_size(300.0)
        .size_range(200.0..=(ui.available_width() - 200.0).max(200.0))
        .resizable(true)
        .show(ui, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("git-commit-details")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    match &h.details {
                        None => {
                            ui.label(egui::RichText::new(s.t("git-loading")).weak());
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
                                    local_time(c.time)
                                ))
                                .color(theme.text_muted)
                                .small(),
                            );
                            if d.committer != c.author {
                                ui.label(
                                    egui::RichText::new(
                                        s.i18n
                                            .t_args("git-committed-by", &[("name", &d.committer)]),
                                    )
                                    .color(theme.text_muted)
                                    .small(),
                                );
                            }
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new(&c.id)
                                        .monospace()
                                        .small()
                                        .color(theme.text_muted),
                                );
                                if ui.small_button(ph::COPY).tip(s.t("git-copy-id")).clicked() {
                                    actions.push(GitAction::CopyPath(c.id.clone()));
                                }
                            });
                            ui.separator();
                            ui.label(
                                egui::RichText::new(s.i18n.t_args(
                                    "git-files-changed",
                                    &[("n", &d.files.len().to_string())],
                                ))
                                .strong(),
                            );
                            let mut pick = None;
                            for (fi, f) in d.files.iter().enumerate() {
                                let fs = FileStatus {
                                    path: f.path.clone(),
                                    orig_path: f.orig_path.clone(),
                                    index: f.status,
                                    worktree: f.status,
                                    kind: Kind::Changed,
                                };
                                let r = change_row(ui, theme, &fs, f.status, h.file == Some(fi));
                                if r.clicked() {
                                    pick = Some(fi);
                                }
                                let full = abs(&h.repo, &f.path);
                                if r.double_clicked() && full.is_file() {
                                    actions.push(GitAction::OpenFile(full));
                                }
                            }
                            if let Some(fi) = pick {
                                h.select_file(fi, s);
                            }
                        }
                    }
                });
        });
    // The selected file's diff fills the rest.
    let file: Option<ChangedFile> = match (&h.details, h.file) {
        (Some(Ok(d)), Some(fi)) => d.files.get(fi).cloned(),
        _ => None,
    };
    if let (Some(f), Some(slot)) = (file, &h.diff) {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(&f.path).strong());
            if let Some(o) = &f.orig_path {
                ui.label(
                    egui::RichText::new(s.i18n.t_args("git-renamed-from", &[("path", o)]))
                        .color(theme.text_muted),
                );
            }
            let counts = match &slot.rows {
                Some(Ok(r)) => Some((r.added, r.removed)),
                _ => None,
            };
            counts_label(ui, theme, counts);
        });
        diff_body(
            ui,
            s,
            theme,
            slot,
            side_by_side,
            font_size,
            "git-history-diff",
            None,
            &mut None,
        );
    } else {
        ui.label(egui::RichText::new(s.t("git-select-file")).weak());
    }
}

fn commit_list(ui: &mut egui::Ui, s: &Services, theme: &Theme, h: &mut HistoryPane) {
    let n = h.commits.len() + usize::from(!h.done);
    let mut pick = None;
    let mut need_more = false;
    let mut area = egui::ScrollArea::vertical()
        .id_salt("git-commit-list")
        .auto_shrink([false, false]);
    if std::mem::take(&mut h.scroll_to_selected)
        && let Some(sel) = h.selected
    {
        // Keep the keyboard selection in view.
        let row_h = ROW + ui.spacing().item_spacing.y;
        let top = sel as f32 * row_h;
        if top < h.list_offset {
            area = area.vertical_scroll_offset(top);
        } else if top + row_h > h.list_offset + h.list_height {
            area = area.vertical_scroll_offset(top + row_h - h.list_height);
        }
    }
    let out = area.show_rows(ui, ROW, n, |ui, range| {
        for i in range {
            let Some(c) = h.commits.get(i) else {
                need_more = true;
                ui.label(egui::RichText::new(s.t("git-loading")).weak());
                continue;
            };
            if i + 50 >= h.commits.len() {
                need_more = true;
            }
            let (rect, resp) =
                ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW), Sense::click());
            resp.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &c.subject)
            });
            if h.selected == Some(i) {
                ui.painter().rect_filled(
                    rect,
                    CornerRadius::same(3),
                    theme.accent.linear_multiply(0.3),
                );
            } else if resp.hovered() {
                ui.painter()
                    .rect_filled(rect, CornerRadius::same(3), theme.surface_hover);
            }
            let p = ui.painter();
            let y = rect.center().y;
            let merge = c.parents.len() > 1;
            p.circle(
                egui::pos2(rect.left() + 12.0, y),
                if merge { 3.0 } else { 4.0 },
                if merge { theme.surface } else { theme.accent },
                egui::Stroke::new(1.5, theme.accent),
            );
            // Right side first, so the subject knows where to stop.
            let date = local_time(c.time);
            let date_r = p.text(
                egui::pos2(rect.right() - 8.0, y),
                Align2::RIGHT_CENTER,
                &date,
                egui::FontId::proportional(12.0),
                theme.text_muted,
            );
            let author_r = p.text(
                egui::pos2(date_r.left() - 16.0, y),
                Align2::RIGHT_CENTER,
                &c.author,
                egui::FontId::proportional(12.0),
                theme.text_muted,
            );
            let mut x = rect.left() + 24.0;
            let sha = p.text(
                egui::pos2(x, y),
                Align2::LEFT_CENTER,
                &c.short,
                egui::FontId::monospace(12.0),
                theme.text_muted,
            );
            x = sha.right() + 10.0;
            let clip =
                egui::Rect::from_min_max(rect.min, egui::pos2(author_r.left() - 12.0, rect.max.y));
            let p = p.with_clip_rect(clip.intersect(p.clip_rect()));
            for r in &c.refs {
                let (label, colour) = if let Some(b) = r.strip_prefix("HEAD -> ") {
                    (format!("{} {b}", ph::GIT_BRANCH), theme.accent)
                } else if let Some(t) = r.strip_prefix("tag: ") {
                    (format!("{} {t}", ph::TAG), theme.warning)
                } else {
                    (r.clone(), theme.secondary)
                };
                let g = p.layout_no_wrap(label, egui::FontId::proportional(11.0), colour);
                let chip = egui::Rect::from_min_size(
                    egui::pos2(x, y - g.size().y / 2.0 - 2.0),
                    g.size() + Vec2::new(10.0, 4.0),
                );
                p.rect_stroke(
                    chip,
                    CornerRadius::same(4),
                    egui::Stroke::new(1.0, colour),
                    egui::StrokeKind::Inside,
                );
                p.galley(chip.min + Vec2::new(5.0, 2.0), g, colour);
                x = chip.right() + 6.0;
            }
            p.text(
                egui::pos2(x, y),
                Align2::LEFT_CENTER,
                &c.subject,
                egui::FontId::proportional(13.0),
                theme.text,
            );
            if resp.clicked() {
                pick = Some(i);
            }
        }
    });
    h.list_offset = out.state.offset.y;
    h.list_height = out.inner_rect.height();
    if let Some(i) = pick {
        h.select(i, s);
    }
    if need_more {
        h.load_more(s);
    }
}
