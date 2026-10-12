//! Code editor tool (docs/tools/editor.md): one editor tab holds several
//! projects (folders), each with its own row of open files above the text,
//! and a project tree in the sidebar listing every project root. Files open
//! with encodings and line endings, the watcher reports changes on disk,
//! Markdown files get a preview, and every project feeds the quick-open
//! palette.

use hs_ui::tip::TipExt;
use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, Instant, SystemTime};

use hs_core::Services;
use hs_core::events::Event;
use hs_core::tool::{
    Accept, CommandDescriptor, OpenRequest, QuickOpenItem, ToastLevel, Tool, ToolCategory,
    ToolContext, ToolDescriptor, ToolFactory,
};
use hs_editor::buffer::{Edit, EditKind, Encoding, Eol};
use hs_editor::view::{EditorTheme, EditorView};
use hs_editor::{Buffer, Cursor, Cursors, languages};
use hs_ui::fileicons;
use hs_ui::icons::ph;
use hs_ui::theme::Theme;
use hs_ui::tree::{TreeNode, TreeState};

mod ai_panel;
use notify::Watcher;
use serde::{Deserialize, Serialize};

pub static DESCRIPTOR: ToolDescriptor = ToolDescriptor {
    id: "editor",
    title_key: "tool-editor",
    icon: ph::CODE,
    category: ToolCategory::Code,
    accepts: &[Accept::LocalFile, Accept::Text],
};

/// Files above this open read-only without highlighting.
const READ_ONLY_BYTES: u64 = 32 * 1024 * 1024;
/// Files above this are refused until the streaming mode exists.
const MAX_BYTES: u64 = 256 * 1024 * 1024;
const INDEX_CAP: usize = 10_000;
const SKIP_DIRS: &[&str] = &[
    ".git",
    "target",
    "node_modules",
    "HyperStackData",
    ".hg",
    ".svn",
];

/// The projects (folders, with the subfolders expanded in the tree) of the
/// editor tab used last, so a new editor tab starts with the same projects
/// and the tree the way the user left it. With "Separate Tabs" every file
/// opens in a new tab, which is how the tabs share one project list.
static LAST_PROJECTS: parking_lot::Mutex<Vec<ProjectState>> = parking_lot::Mutex::new(Vec::new());

pub struct EditorFactory;

impl ToolFactory for EditorFactory {
    fn descriptor(&self) -> &ToolDescriptor {
        &DESCRIPTOR
    }
    fn create(&self, services: &Services, restore: Option<serde_json::Value>) -> Box<dyn Tool> {
        let state: State = restore
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default();
        Box::new(EditorTool::new(services, state))
    }
}

// ---- saved state -------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct State {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    projects: Vec<ProjectState>,
    #[serde(default)]
    active: usize,
    // The layout before several projects: one file and one folder per tab.
    // Read once to migrate, never written.
    #[serde(default, skip_serializing)]
    path: Option<PathBuf>,
    #[serde(default, skip_serializing)]
    root: Option<PathBuf>,
    #[serde(default, skip_serializing)]
    preview: bool,
    #[serde(default, skip_serializing)]
    cursor: usize,
    #[serde(default, skip_serializing)]
    expanded: Vec<PathBuf>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
struct ProjectState {
    /// `None` is the "Other Files" group for files outside every project.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    root: Option<PathBuf>,
    /// Folders expanded in the tree; only the user collapses them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    expanded: Vec<PathBuf>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    files: Vec<FileState>,
    #[serde(default)]
    active: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct FileState {
    path: PathBuf,
    #[serde(default)]
    cursor: usize,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    preview: bool,
}

impl State {
    /// The projects to restore, turning a one-file layout into projects.
    fn into_projects(self) -> (Vec<ProjectState>, usize) {
        if !self.projects.is_empty() {
            return (self.projects, self.active);
        }
        let file = self.path.map(|path| FileState {
            path,
            cursor: self.cursor,
            preview: self.preview,
        });
        let mut out = Vec::new();
        let mut loose = None;
        if let Some(root) = self.root {
            let mut p = ProjectState {
                root: Some(root.clone()),
                expanded: self.expanded,
                ..Default::default()
            };
            match file {
                Some(f) if f.path.starts_with(&root) => p.files.push(f),
                other => loose = other,
            }
            out.push(p);
        } else {
            loose = file;
        }
        let mut active = 0;
        if let Some(f) = loose {
            out.push(ProjectState {
                files: vec![f],
                ..Default::default()
            });
            active = out.len() - 1;
        }
        (out, active)
    }
}

// ---- one open file -----------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DiskState {
    Clean,
    Changed,
    Deleted,
}

struct Doc {
    path: Option<PathBuf>,
    buf: Buffer,
    cursors: Cursors,
    view: EditorView,
    read_only_reason: Option<String>,
    saved_mtime: Option<SystemTime>,
    just_saved: Option<Instant>,
    disk: DiskState,
    preview: bool,
    preview_cache: (u64, String),
}

impl Doc {
    fn new(s: &Services) -> Self {
        let st = s.settings.read();
        let dark = Theme::by_name(&st.appearance.theme).dark;
        let mut view = EditorView::new(
            egui::Id::new(("hs-editor", hs_core::new_id())),
            languages::plain(),
            dark,
            st.editor.font_size,
        );
        view.right_click_copy_paste = st.editor.right_click_copy_paste;
        Self {
            path: None,
            buf: Buffer::default(),
            cursors: Cursors::default(),
            view,
            read_only_reason: None,
            saved_mtime: None,
            just_saved: None,
            disk: DiskState::Clean,
            preview: false,
            preview_cache: (u64::MAX, String::new()),
        }
    }

    fn open(path: &Path, s: &Services) -> Result<Self, String> {
        let mut d = Self::new(s);
        d.load(path, s)?;
        Ok(d)
    }

    fn file_name(&self) -> String {
        self.path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default()
    }

    /// The empty untitled file a fresh tab starts with; opening a file
    /// replaces it instead of adding a tab next to it.
    fn is_placeholder(&self) -> bool {
        self.path.is_none() && !self.buf.dirty() && self.buf.len_chars() == 0
    }

    fn load(&mut self, path: &Path, services: &Services) -> Result<(), String> {
        let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
        if meta.len() > MAX_BYTES {
            return Err(services.i18n.t_args(
                "editor-too-large",
                &[
                    (
                        "name",
                        &path
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_default(),
                    ),
                    ("size", &human_size(meta.len())),
                ],
            ));
        }
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        self.buf = Buffer::from_bytes(&bytes);
        self.cursors = Cursors::default();
        let lang = languages::detect(&path.to_string_lossy());
        if meta.len() > READ_ONLY_BYTES {
            self.view.set_language(languages::plain());
            self.view.read_only = true;
            self.read_only_reason = Some(services.t("editor-read-only"));
        } else {
            self.view.set_language(lang);
            self.view.read_only = !is_writable(&meta);
            self.read_only_reason = self.view.read_only.then(|| services.t("editor-read-only"));
        }
        self.saved_mtime = meta.modified().ok();
        self.disk = DiskState::Clean;
        self.path = Some(path.to_path_buf());
        self.preview = self.preview && self.view.language.is_markdown;
        Ok(())
    }

    fn set_text(&mut self, text: &str, name_hint: Option<&str>) {
        self.buf = Buffer::from_text(text);
        self.cursors = Cursors::default();
        self.path = None;
        self.saved_mtime = None;
        self.disk = DiskState::Clean;
        self.view.read_only = false;
        self.read_only_reason = None;
        self.view.set_language(
            name_hint
                .map(languages::detect)
                .unwrap_or_else(languages::plain),
        );
    }

    fn goto_line(&mut self, line: u32) {
        let pos = self
            .buf
            .line_col_to_char((line as usize).saturating_sub(1), 0);
        self.cursors.set_single(Cursor::at(pos));
        self.view.scroll_to_cursor();
    }

    fn prepare_save(&mut self, trim_trailing: bool, final_newline: bool) {
        if !trim_trailing && !final_newline {
            return;
        }
        let before = self.cursors.snapshot();
        let mut edits = Vec::new();
        if trim_trailing {
            for line in (0..self.buf.len_lines()).rev() {
                let text = self.buf.line_text(line);
                let trimmed = text.trim_end_matches([' ', '\t']);
                if trimmed.len() != text.len() {
                    let start = self.buf.line_start(line) + trimmed.chars().count();
                    edits.push(Edit {
                        pos: start,
                        removed: text[trimmed.len()..].to_string(),
                        inserted: String::new(),
                    });
                }
            }
        }
        if final_newline
            && self.buf.len_chars() > 0
            && self.buf.char_at(self.buf.len_chars() - 1) != Some('\n')
        {
            edits.insert(
                0,
                Edit {
                    pos: self.buf.len_chars(),
                    removed: String::new(),
                    inserted: "\n".into(),
                },
            );
        }
        if !edits.is_empty() {
            self.buf
                .apply(edits, EditKind::Other, before.clone(), before.clone());
            self.cursors.restore(&before, self.buf.len_chars());
            self.view.scroll_to_cursor();
        }
    }

    fn reload(&mut self, services: &Services) {
        let Some(path) = self.path.clone() else {
            return;
        };
        let pos = self.cursors.primary().head;
        let preview = self.preview;
        if self.load(&path, services).is_ok() {
            self.cursors
                .set_single(Cursor::at(pos.min(self.buf.len_chars())));
            self.preview = preview && self.view.language.is_markdown;
            self.view.scroll_to_cursor();
        }
    }

    fn on_disk_event(&mut self, kind: &notify::EventKind, auto_reload: bool, s: &Services) {
        let Some(path) = self.path.clone() else {
            return;
        };
        if matches!(kind, notify::EventKind::Remove(_)) && !path.exists() {
            self.disk = DiskState::Deleted;
            self.buf.mark_modified_externally();
            return;
        }
        if self
            .just_saved
            .is_some_and(|t| t.elapsed() < Duration::from_millis(1500))
        {
            return;
        }
        let Ok(meta) = std::fs::metadata(&path) else {
            return;
        };
        if meta.modified().ok() == self.saved_mtime {
            return;
        }
        if !self.buf.dirty() && auto_reload {
            self.reload(s);
        } else {
            self.disk = DiskState::Changed;
        }
    }

    fn apply_settings(&mut self, s: &Services) {
        let st = s.settings.read();
        self.view.font_size = st.editor.font_size;
        self.view.right_click_copy_paste = st.editor.right_click_copy_paste;
    }

    fn status_row(&mut self, ui: &mut egui::Ui, s: &Services) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 14.0;
            let c = self.cursors.primary();
            let (line, col) = self.buf.char_to_line_col(c.head);
            let mut pos = s.i18n.t_args(
                "editor-position",
                &[
                    ("line", &(line + 1).to_string()),
                    ("col", &(col + 1).to_string()),
                ],
            );
            let selected: usize = self
                .cursors
                .list
                .iter()
                .map(|c| c.range().1 - c.range().0)
                .sum();
            if selected > 0 {
                pos.push_str(&format!(
                    "  ({})",
                    s.i18n
                        .t_args("editor-selected", &[("n", &selected.to_string())])
                ));
            }
            if self.cursors.list.len() > 1 {
                pos.push_str(&format!("  ×{}", self.cursors.list.len()));
            }
            let small = |t: String| egui::RichText::new(t).size(12.0);
            ui.label(small(pos));
            if let Some(r) = &self.read_only_reason {
                ui.colored_label(egui::Color32::from_rgb(0xD9, 0xB0, 0x2A), small(r.clone()));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some(p) = &self.path {
                    ui.add(
                        egui::Label::new(small(p.to_string_lossy().to_string()).weak()).truncate(),
                    );
                }
                // Language.
                let lang_name = self.view.language.name.to_string();
                ui.menu_button(small(lang_name), |ui| {
                    for (name, ext) in LANGUAGE_MENU {
                        if ui
                            .selectable_label(self.view.language.name == *name, *name)
                            .clicked()
                        {
                            self.view.set_language(languages::by_extension(ext));
                            ui.close();
                        }
                    }
                });
                // EOL.
                ui.menu_button(small(self.buf.eol.label().into()), |ui| {
                    for eol in [Eol::Lf, Eol::CrLf] {
                        if ui
                            .selectable_label(self.buf.eol == eol, eol.label())
                            .clicked()
                        {
                            if self.buf.eol != eol {
                                self.buf.eol = eol;
                                self.buf.mark_modified_externally();
                            }
                            ui.close();
                        }
                    }
                });
                // Encoding.
                ui.menu_button(small(self.buf.encoding.label().into()), |ui| {
                    for enc in Encoding::all() {
                        if ui
                            .selectable_label(self.buf.encoding == enc, enc.label())
                            .clicked()
                        {
                            if self.buf.encoding != enc {
                                self.buf.encoding = enc;
                                self.buf.mark_modified_externally();
                            }
                            ui.close();
                        }
                    }
                });
                // Indentation.
                let indent_label = if self.view.tab_inserts_spaces {
                    s.i18n.t_args(
                        "editor-spaces",
                        &[("n", &self.view.language.indent.to_string())],
                    )
                } else {
                    s.t("editor-tabs")
                };
                ui.menu_button(small(indent_label), |ui| {
                    for n in [2usize, 4, 8] {
                        let label = s.i18n.t_args("editor-spaces", &[("n", &n.to_string())]);
                        if ui
                            .selectable_label(
                                self.view.tab_inserts_spaces && self.view.language.indent == n,
                                label,
                            )
                            .clicked()
                        {
                            self.view.tab_inserts_spaces = true;
                            self.view.language.indent = n;
                            ui.close();
                        }
                    }
                    if ui
                        .selectable_label(!self.view.tab_inserts_spaces, s.t("editor-tabs"))
                        .clicked()
                    {
                        self.view.tab_inserts_spaces = false;
                        ui.close();
                    }
                });
            });
        });
    }

    fn disk_banner(&mut self, ui: &mut egui::Ui, s: &Services) {
        if self.disk == DiskState::Clean {
            return;
        }
        let name = self.file_name();
        let colour = egui::Color32::from_rgb(0xD9, 0xB0, 0x2A);
        egui::Frame::new()
            .fill(colour.linear_multiply(0.18))
            .inner_margin(egui::Margin::symmetric(10, 6))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let key = if self.disk == DiskState::Deleted {
                        "editor-deleted-on-disk"
                    } else {
                        "editor-changed-on-disk"
                    };
                    ui.label(s.i18n.t_args(key, &[("name", &name)]));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if self.disk == DiskState::Changed
                            && ui.button(s.t("editor-reload")).clicked()
                        {
                            self.reload(s);
                        }
                        if ui.button(s.t("editor-keep-mine")).clicked() {
                            self.disk = DiskState::Clean;
                            if let Some(p) = &self.path {
                                self.saved_mtime =
                                    std::fs::metadata(p).and_then(|m| m.modified()).ok();
                            }
                        }
                    });
                });
            });
    }

    fn preview_ui(&mut self, ui: &mut egui::Ui, dark: bool, theme: &Theme) {
        if self.preview_cache.0 != self.buf.version() {
            self.preview_cache = (self.buf.version(), self.buf.text());
        }
        let style = hs_editor::markdown::MarkdownStyle {
            dark,
            text: theme.text,
            muted: theme.text_muted,
            link: theme.secondary,
            code_bg: theme.surface,
            quote_bar: theme.border,
            base_size: 14.0,
        };
        let text = self.preview_cache.1.clone();
        egui::ScrollArea::vertical()
            .id_salt(self.view.id.with("preview-scroll"))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                egui::Frame::new()
                    .inner_margin(egui::Margin::same(16))
                    .show(ui, |ui| {
                        hs_editor::markdown::render(ui, &text, &style);
                    });
            });
    }
}

// ---- one project -------------------------------------------------------------

struct Project {
    /// `None`: the "Other Files" group.
    root: Option<PathBuf>,
    docs: Vec<Doc>,
    active: usize,
    index: Arc<StdMutex<Vec<PathBuf>>>,
    index_building: Arc<AtomicBool>,
    index_dirty: bool,
    index_built_at: Option<Instant>,
}

impl Project {
    fn new(root: Option<PathBuf>) -> Self {
        let mut p = Self {
            root,
            docs: Vec::new(),
            active: 0,
            index: Arc::new(StdMutex::new(Vec::new())),
            index_building: Arc::new(AtomicBool::new(false)),
            index_dirty: false,
            index_built_at: None,
        };
        p.rebuild_index();
        p
    }

    fn name(&self, s: &Services) -> String {
        match &self.root {
            Some(r) => r
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| r.to_string_lossy().to_string()),
            None => s.t("editor-other-files"),
        }
    }

    fn contains(&self, path: &Path) -> bool {
        self.root.as_ref().is_some_and(|r| path.starts_with(r))
    }

    fn relative(&self, path: &Path) -> String {
        match &self.root {
            Some(r) => path
                .strip_prefix(r)
                .unwrap_or(path)
                .to_string_lossy()
                .replace('\\', "/"),
            None => path.to_string_lossy().to_string(),
        }
    }

    fn rebuild_index(&mut self) {
        let Some(root) = self.root.clone() else {
            return;
        };
        if self.index_building.swap(true, Ordering::SeqCst) {
            return;
        }
        self.index_dirty = false;
        self.index_built_at = Some(Instant::now());
        let index = self.index.clone();
        let building = self.index_building.clone();
        std::thread::Builder::new()
            .name("hs-editor-index".into())
            .spawn(move || {
                let mut out = Vec::new();
                walk(&root, &mut out, INDEX_CAP);
                if let Ok(mut i) = index.lock() {
                    *i = out;
                }
                building.store(false, Ordering::SeqCst);
            })
            .ok();
    }
}

// ---- the tool ----------------------------------------------------------------

/// One watcher for every project root (recursive) and the folders of open
/// files outside them (not recursive).
struct Watch {
    watcher: notify::RecommendedWatcher,
    rx: mpsc::Receiver<notify::Result<notify::Event>>,
    watched: Vec<(PathBuf, bool)>,
}

#[derive(Debug, Clone)]
enum PromptKind {
    NewFile(PathBuf),
    NewFolder(PathBuf),
    Rename(PathBuf),
}

#[derive(Debug, Clone)]
struct Prompt {
    kind: PromptKind,
    text: String,
    focus: bool,
}

/// Closing that would throw away unsaved changes, waiting for a yes.
#[derive(Debug, Clone, Copy, PartialEq)]
enum CloseTarget {
    File(usize, usize),
    Others(usize, usize),
    Project(usize),
}

/// What a click in the file rows asks for, applied after drawing.
enum RowAction {
    Activate(usize, usize),
    Close(usize, usize),
    CloseOthers(usize, usize),
    CloseProject(usize),
    CopyPath(String),
}

pub struct EditorTool {
    projects: Vec<Project>,
    /// Index into `projects`; that project always has an open file.
    active: usize,
    auto_reload: bool,
    /// Settings → Editor → Open Files In = Project Rows.
    rows_mode: bool,
    trim_trailing: bool,
    final_newline: bool,

    /// One root node per project with a folder, in project order.
    nodes: Vec<TreeNode>,
    tree: TreeState,
    watch: Option<Watch>,

    menu: Option<(PathBuf, egui::Pos2)>,
    prompt: Option<Prompt>,
    confirm_delete: Option<PathBuf>,
    confirm_close: Option<CloseTarget>,
    id: egui::Id,
    tab_id: u64,
    ai: ai_panel::AiPanel,
}

impl EditorTool {
    fn new(services: &Services, state: State) -> Self {
        let st = services.settings.read();
        let auto_reload = st.editor.auto_reload;
        let rows_mode = st.editor.open_files_in == hs_core::settings::OpenFilesIn::Rows;
        let trim_trailing = st.editor.trim_trailing_whitespace;
        let final_newline = st.editor.insert_final_newline;
        drop(st);
        let mut t = Self {
            projects: Vec::new(),
            active: 0,
            auto_reload,
            rows_mode,
            trim_trailing,
            final_newline,
            nodes: Vec::new(),
            tree: TreeState::default(),
            watch: None,
            menu: None,
            prompt: None,
            confirm_delete: None,
            confirm_close: None,
            id: egui::Id::new(("hs-editor-tool", hs_core::new_id())),
            tab_id: 0,
            ai: ai_panel::AiPanel::default(),
        };
        t.start_watch(services);
        let (mut projects, active) = state.into_projects();
        if projects.is_empty() {
            projects = LAST_PROJECTS.lock().clone();
        }
        let mut restored_active = None;
        for (i, ps) in projects.into_iter().enumerate() {
            let pi = match ps.root.clone().filter(|r| r.is_dir()) {
                Some(root) => {
                    let pi = t.add_project(root);
                    t.expand_dirs(&ps.expanded);
                    pi
                }
                None if ps.files.is_empty() => continue,
                None => t.loose_project(),
            };
            let mut active_doc = None;
            for (fi, f) in ps.files.iter().enumerate() {
                if !f.path.is_file() {
                    continue;
                }
                match Doc::open(&f.path, services) {
                    Ok(mut d) => {
                        let pos = f.cursor.min(d.buf.len_chars());
                        d.cursors.set_single(Cursor::at(pos));
                        d.view.scroll_to_cursor();
                        d.preview = f.preview && d.view.language.is_markdown;
                        let target = t.project_for(&f.path).unwrap_or(pi);
                        t.projects[target].docs.push(d);
                        if fi == ps.active && target == pi {
                            active_doc = Some(t.projects[pi].docs.len() - 1);
                        }
                    }
                    Err(e) => tracing::warn!("editor: could not restore {}: {e}", f.path.display()),
                }
            }
            if let Some(d) = active_doc {
                t.projects[pi].active = d;
            }
            if i == active {
                restored_active = Some(pi);
            }
        }
        if let Some(pi) = restored_active {
            t.active = pi;
        }
        t.ensure_doc(services);
        t.sync_watches();
        t.select_in_tree();
        t
    }

    fn theme(s: &Services) -> (Theme, EditorTheme) {
        let name = s.settings.read().appearance.theme.clone();
        let ui_theme = Theme::by_name(&name);
        let ed = EditorTheme::for_theme(&ui_theme);
        (ui_theme, ed)
    }

    // ---- projects and files ---------------------------------------------------

    fn doc(&self) -> &Doc {
        let p = &self.projects[self.active];
        &p.docs[p.active]
    }

    fn doc_mut(&mut self) -> &mut Doc {
        let p = &mut self.projects[self.active];
        &mut p.docs[p.active]
    }

    fn total_docs(&self) -> usize {
        self.projects.iter().map(|p| p.docs.len()).sum()
    }

    /// Keep the invariant: the active project has an open file (an empty
    /// untitled one when nothing else is open) and every index is in range.
    fn ensure_doc(&mut self, s: &Services) {
        if self.projects.is_empty() {
            self.projects.push(Project::new(None));
        }
        if self.total_docs() == 0 {
            self.active = self.active.min(self.projects.len() - 1);
            self.projects[self.active].docs.push(Doc::new(s));
        }
        if self
            .projects
            .get(self.active)
            .is_none_or(|p| p.docs.is_empty())
        {
            self.active = self
                .projects
                .iter()
                .position(|p| !p.docs.is_empty())
                .unwrap_or(0);
        }
        for p in &mut self.projects {
            p.active = p.active.min(p.docs.len().saturating_sub(1));
        }
    }

    /// The project whose folder holds `path` (the innermost one when
    /// projects are nested).
    fn project_for(&self, path: &Path) -> Option<usize> {
        self.projects
            .iter()
            .enumerate()
            .filter(|(_, p)| p.contains(path))
            .max_by_key(|(_, p)| p.root.as_ref().map_or(0, |r| r.components().count()))
            .map(|(i, _)| i)
    }

    /// The "Other Files" group, created on first use.
    fn loose_project(&mut self) -> usize {
        if let Some(i) = self.projects.iter().position(|p| p.root.is_none()) {
            return i;
        }
        self.projects.push(Project::new(None));
        self.projects.len() - 1
    }

    fn find_doc(&self, path: &Path) -> Option<(usize, usize)> {
        self.projects.iter().enumerate().find_map(|(pi, p)| {
            p.docs
                .iter()
                .position(|d| d.path.as_deref() == Some(path))
                .map(|di| (pi, di))
        })
    }

    /// Hand this tab's projects (not its files) to the next editor tab.
    fn remember_projects(&self) {
        *LAST_PROJECTS.lock() = self
            .projects
            .iter()
            .filter_map(|p| {
                p.root.as_ref().map(|r| ProjectState {
                    root: Some(r.clone()),
                    expanded: self.expanded_dirs(r),
                    ..Default::default()
                })
            })
            .collect();
    }

    /// Add `root` as a project (or find it) and make it the one new files go to.
    fn add_project(&mut self, root: PathBuf) -> usize {
        if let Some(i) = self
            .projects
            .iter()
            .position(|p| p.root.as_ref() == Some(&root))
        {
            return i;
        }
        let id = root.to_string_lossy().to_string();
        let name = root
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| id.clone());
        let icon = fileicons::for_folder(&name);
        let mut node = TreeNode::branch(id.clone(), name, icon, load_children(&root));
        node.leaf = false;
        self.nodes.push(node);
        self.tree.expanded.insert(id);
        // Keep "Other Files" last.
        let at = self
            .projects
            .iter()
            .position(|p| p.root.is_none())
            .unwrap_or(self.projects.len());
        let before = self.projects.len();
        self.projects.insert(at, Project::new(Some(root)));
        if self.active < before && self.active >= at {
            // The active project moved one place down.
            self.active += 1;
        }
        // Files open in "Other Files" that belong to the new project move to it.
        if let Some(li) = self.projects.iter().position(|p| p.root.is_none()) {
            let mut moved = Vec::new();
            let mut i = 0;
            while i < self.projects[li].docs.len() {
                let inside = self.projects[li].docs[i]
                    .path
                    .as_ref()
                    .is_some_and(|p| self.projects[at].contains(p));
                if inside {
                    moved.push(self.projects[li].docs.remove(i));
                } else {
                    i += 1;
                }
            }
            self.projects[at].docs.extend(moved);
        }
        self.projects[at].rebuild_index();
        self.sync_watches();
        self.remember_projects();
        at
    }

    fn close_project(&mut self, pi: usize, services: &Services) {
        if pi >= self.projects.len() {
            return;
        }
        let p = self.projects.remove(pi);
        if let Some(root) = &p.root {
            let id = root.to_string_lossy().to_string();
            self.nodes.retain(|n| n.id != id);
            self.tree
                .expanded
                .retain(|e| !Path::new(e).starts_with(root));
        }
        if self.active > pi || self.active >= self.projects.len() {
            self.active = self.active.saturating_sub(1);
        }
        self.ensure_doc(services);
        self.sync_watches();
        self.select_in_tree();
        self.remember_projects();
    }

    fn activate(&mut self, pi: usize, di: usize) {
        if pi < self.projects.len() && di < self.projects[pi].docs.len() {
            self.active = pi;
            self.projects[pi].active = di;
            self.select_in_tree();
        }
    }

    fn select_in_tree(&mut self) {
        if let Some(p) = self.doc().path.clone()
            && self.project_for(&p).is_some()
        {
            self.tree.selected = Some(p.to_string_lossy().to_string());
        }
    }

    /// Open `path` in this tab, in the row of the project that holds it
    /// (or "Other Files"), or switch to it when it is already open.
    fn open_file(&mut self, path: &Path, line: Option<u32>, s: &Services) -> Result<(), String> {
        if let Some((pi, di)) = self.find_doc(path) {
            self.activate(pi, di);
        } else {
            let doc = Doc::open(path, s)?;
            let pi = match self.project_for(path) {
                Some(pi) => pi,
                None => self.loose_project(),
            };
            self.drop_placeholder();
            let p = &mut self.projects[pi];
            let di = (p.active + 1).min(p.docs.len());
            p.docs.insert(di, doc);
            self.activate(pi, di);
            self.sync_watches();
        }
        if let Some(l) = line {
            self.doc_mut().goto_line(l);
        }
        Ok(())
    }

    /// Remove the active empty untitled file before something replaces it.
    fn drop_placeholder(&mut self) {
        let a = self.active;
        let p = &mut self.projects[a];
        if p.docs.get(p.active).is_some_and(Doc::is_placeholder) {
            p.docs.remove(p.active);
            p.active = p.active.saturating_sub(1);
        }
    }

    fn open_path(&mut self, path: PathBuf, line: Option<u32>, cx: &mut ToolContext) {
        if path.is_dir() {
            let pi = self.add_project(path);
            if !self.projects[pi].docs.is_empty() {
                self.active = pi;
            }
            self.ensure_doc(cx.services);
            return;
        }
        // "Separate Tabs": a file this tab does not show gets its own tab
        // (the shell focuses a tab that already shows it), unless this tab
        // is still empty.
        if !self.rows_mode && self.find_doc(&path).is_none() && !self.doc().is_placeholder() {
            self.remember_projects();
            cx.open(
                "editor",
                Some(OpenRequest::File {
                    uri: path.to_string_lossy().to_string(),
                    line,
                }),
            );
            // The click selected the file in this tab's tree; it opened
            // elsewhere, so the tree goes back to this tab's file.
            self.select_in_tree();
            return;
        }
        if let Err(e) = self.open_file(&path, line, cx.services) {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            cx.toast(
                ToastLevel::Error,
                cx.services
                    .i18n
                    .t_args("editor-open-failed", &[("name", &name), ("error", &e)]),
            );
        }
    }

    fn new_file(&mut self, s: &Services) {
        let pi = self.active;
        let p = &mut self.projects[pi];
        let di = (p.active + 1).min(p.docs.len());
        p.docs.insert(di, Doc::new(s));
        self.activate(pi, di);
    }

    fn close_file(&mut self, pi: usize, di: usize, s: &Services) {
        if pi >= self.projects.len() || di >= self.projects[pi].docs.len() {
            return;
        }
        let p = &mut self.projects[pi];
        p.docs.remove(di);
        if p.active > di || p.active >= p.docs.len() {
            p.active = p.active.saturating_sub(1);
        }
        self.ensure_doc(s);
        self.sync_watches();
        self.select_in_tree();
    }

    fn close_others(&mut self, pi: usize, di: usize, s: &Services) {
        if pi >= self.projects.len() || di >= self.projects[pi].docs.len() {
            return;
        }
        let p = &mut self.projects[pi];
        let keep = p.docs.remove(di);
        p.docs.clear();
        p.docs.push(keep);
        p.active = 0;
        self.activate(pi, 0);
        self.ensure_doc(s);
        self.sync_watches();
    }

    /// Close at once when nothing would be lost, otherwise ask first.
    fn request_close(&mut self, target: CloseTarget, s: &Services) {
        let dirty = match target {
            CloseTarget::File(pi, di) => self
                .projects
                .get(pi)
                .and_then(|p| p.docs.get(di))
                .is_some_and(|d| d.buf.dirty()),
            CloseTarget::Others(pi, di) => self.projects.get(pi).is_some_and(|p| {
                p.docs
                    .iter()
                    .enumerate()
                    .any(|(i, d)| i != di && d.buf.dirty())
            }),
            CloseTarget::Project(pi) => self
                .projects
                .get(pi)
                .is_some_and(|p| p.docs.iter().any(|d| d.buf.dirty())),
        };
        if dirty {
            self.confirm_close = Some(target);
        } else {
            self.do_close(target, s);
        }
    }

    fn do_close(&mut self, target: CloseTarget, s: &Services) {
        match target {
            CloseTarget::File(pi, di) => self.close_file(pi, di, s),
            CloseTarget::Others(pi, di) => self.close_others(pi, di, s),
            CloseTarget::Project(pi) => self.close_project(pi, s),
        }
    }

    /// Step through every open file, project by project.
    fn cycle(&mut self, forward: bool) {
        let all: Vec<(usize, usize)> = self
            .projects
            .iter()
            .enumerate()
            .flat_map(|(pi, p)| (0..p.docs.len()).map(move |di| (pi, di)))
            .collect();
        let cur = (self.active, self.projects[self.active].active);
        let Some(i) = all.iter().position(|x| *x == cur) else {
            return;
        };
        let n = all.len();
        let next = if forward {
            (i + 1) % n
        } else {
            (i + n - 1) % n
        };
        let (pi, di) = all[next];
        self.activate(pi, di);
    }

    fn save(&mut self, cx: &mut ToolContext) -> bool {
        let Some(path) = self.doc().path.clone() else {
            return self.save_as(cx);
        };
        let (trim, final_nl) = (self.trim_trailing, self.final_newline);
        let d = self.doc_mut();
        d.prepare_save(trim, final_nl);
        let bytes = d.buf.to_bytes();
        let name = d.file_name();
        match hs_core::storage::write_atomic(&path, &bytes) {
            Ok(()) => {
                d.buf.mark_saved();
                d.saved_mtime = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
                d.just_saved = Some(Instant::now());
                d.disk = DiskState::Clean;
                cx.services.events.publish(Event::FileSaved {
                    uri: format!("file://{}", path.display()),
                });
                cx.status(cx.services.i18n.t_args("editor-saved", &[("name", &name)]));
                true
            }
            Err(e) => {
                let msg = cx.services.i18n.t_args(
                    "editor-save-failed",
                    &[("name", &name), ("error", &e.to_string())],
                );
                cx.toast(ToastLevel::Error, msg);
                false
            }
        }
    }

    fn save_as(&mut self, cx: &mut ToolContext) -> bool {
        let mut dlg = rfd::FileDialog::new();
        let root = self.projects[self.active].root.clone();
        if let Some(dir) = self
            .doc()
            .path
            .as_ref()
            .and_then(|p| p.parent())
            .or(root.as_deref())
        {
            dlg = dlg.set_directory(dir);
        }
        let name = self.doc().file_name();
        if !name.is_empty() {
            dlg = dlg.set_file_name(name);
        }
        let Some(path) = dlg.save_file() else {
            return false;
        };
        let d = self.doc_mut();
        d.path = Some(path.clone());
        d.view
            .set_language(languages::detect(&path.to_string_lossy()));
        d.view.read_only = false;
        d.read_only_reason = None;
        let ok = self.save(cx);
        if ok {
            self.sync_watches();
            if let Some(pi) = self.project_for(&path) {
                self.projects[pi].index_dirty = true;
            }
        }
        ok
    }

    fn open_file_dialog(&mut self, cx: &mut ToolContext) {
        let mut dlg = rfd::FileDialog::new();
        if let Some(dir) = self.projects[self.active].root.as_deref() {
            dlg = dlg.set_directory(dir);
        }
        if let Some(path) = dlg.pick_file() {
            self.open_path(path, None, cx);
        }
    }

    fn open_folder_dialog(&mut self, cx: &mut ToolContext) {
        let mut dlg = rfd::FileDialog::new();
        if let Some(dir) = self.projects[self.active].root.as_deref() {
            dlg = dlg.set_directory(dir);
        }
        if let Some(dir) = dlg.pick_folder() {
            self.open_path(dir, None, cx);
        }
    }

    /// How the assistant names the open file: relative to its project.
    fn ai_file_label(&self) -> String {
        match &self.doc().path {
            Some(p) => self.projects[self.active].relative(p),
            None => "untitled".into(),
        }
    }

    // ---- project tree and watcher ----------------------------------------------

    /// Expand these folders (outermost first, so each parent is loaded
    /// before its child is looked up). Folders gone from disk are skipped.
    fn expand_dirs(&mut self, dirs: &[PathBuf]) {
        let mut dirs: Vec<&PathBuf> = dirs
            .iter()
            .filter(|d| self.project_for(d).is_some() && d.is_dir())
            .collect();
        dirs.sort_by_key(|d| d.components().count());
        for d in dirs {
            let id = d.to_string_lossy().to_string();
            if !node_has_children(&self.nodes, &id) {
                set_children(&mut self.nodes, &id, load_children(d));
            }
            self.tree.expanded.insert(id);
        }
    }

    /// The expanded folders under `root` in a stable order, for the layout.
    fn expanded_dirs(&self, root: &Path) -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = self
            .tree
            .expanded
            .iter()
            .map(PathBuf::from)
            .filter(|p| p.starts_with(root))
            .collect();
        v.sort();
        v
    }

    fn start_watch(&mut self, services: &Services) {
        let (tx, rx) = mpsc::channel();
        let waker = services.runtime.waker();
        match notify::recommended_watcher(move |res| {
            let _ = tx.send(res);
            waker.request_repaint();
        }) {
            Ok(watcher) => {
                self.watch = Some(Watch {
                    watcher,
                    rx,
                    watched: Vec::new(),
                })
            }
            Err(e) => tracing::warn!("editor: file watcher unavailable: {e}"),
        }
    }

    /// Watch every project root and the folders of open files outside them.
    fn sync_watches(&mut self) {
        let mut want: Vec<(PathBuf, bool)> = self
            .projects
            .iter()
            .filter_map(|p| p.root.clone().map(|r| (r, true)))
            .collect();
        for p in &self.projects {
            for d in &p.docs {
                if let Some(path) = &d.path
                    && self.project_for(path).is_none()
                    && let Some(dir) = path.parent()
                    && !want.iter().any(|(w, _)| w == dir)
                {
                    want.push((dir.to_path_buf(), false));
                }
            }
        }
        let Some(w) = self.watch.as_mut() else { return };
        for old in w.watched.clone() {
            if !want.contains(&old) {
                let _ = w.watcher.unwatch(&old.0);
                w.watched.retain(|x| *x != old);
            }
        }
        for (dir, recursive) in want {
            if w.watched.contains(&(dir.clone(), recursive)) {
                continue;
            }
            let mode = if recursive {
                notify::RecursiveMode::Recursive
            } else {
                notify::RecursiveMode::NonRecursive
            };
            match w.watcher.watch(&dir, mode) {
                Ok(()) => w.watched.push((dir, recursive)),
                Err(e) => tracing::warn!("editor: watch {} failed: {e}", dir.display()),
            }
        }
    }

    fn poll_watch(&mut self, services: &Services) {
        let Some(w) = &self.watch else { return };
        let mut events = Vec::new();
        while let Ok(ev) = w.rx.try_recv() {
            if let Ok(ev) = ev {
                events.push(ev);
            }
        }
        if events.is_empty() {
            return;
        }
        let mut dirs_to_refresh: Vec<PathBuf> = Vec::new();
        for ev in events {
            use notify::EventKind as K;
            let structural = matches!(
                ev.kind,
                K::Create(_) | K::Remove(_) | K::Modify(notify::event::ModifyKind::Name(_))
            );
            for p in &ev.paths {
                if structural {
                    if let Some(parent) = p.parent()
                        && !dirs_to_refresh.iter().any(|d| d == parent)
                    {
                        dirs_to_refresh.push(parent.to_path_buf());
                    }
                    for pr in &mut self.projects {
                        if pr.contains(p) {
                            pr.index_dirty = true;
                        }
                    }
                }
                let auto_reload = self.auto_reload;
                for pr in &mut self.projects {
                    for d in &mut pr.docs {
                        if d.path.as_deref() == Some(p.as_path()) {
                            d.on_disk_event(&ev.kind, auto_reload, services);
                        }
                    }
                }
            }
        }
        for dir in dirs_to_refresh {
            let id = dir.to_string_lossy().to_string();
            if node_has_children(&self.nodes, &id) {
                set_children(&mut self.nodes, &id, load_children(&dir));
            }
        }
        for pr in &mut self.projects {
            if pr.index_dirty
                && pr
                    .index_built_at
                    .is_none_or(|t| t.elapsed() > Duration::from_secs(2))
            {
                pr.rebuild_index();
            }
        }
    }

    fn refresh_tree(&mut self) {
        let expanded: Vec<String> = self.tree.expanded.iter().cloned().collect();
        for id in expanded {
            let dir = PathBuf::from(&id);
            if dir.is_dir() && node_has_children(&self.nodes, &id) {
                set_children(&mut self.nodes, &id, load_children(&dir));
            }
        }
        for p in &mut self.projects {
            p.index_dirty = true;
        }
    }

    fn relative(&self, path: &Path) -> String {
        match self.project_for(path) {
            Some(pi) => self.projects[pi].relative(path),
            None => path.to_string_lossy().to_string(),
        }
    }

    // ---- ui pieces -----------------------------------------------------------

    /// One row of file tabs per project that has open files. Hidden while
    /// the tab holds a single file, so the plain one-file editor looks as
    /// before.
    fn file_rows(&mut self, ui: &mut egui::Ui, s: &Services, theme: &Theme) {
        let with_docs = self.projects.iter().filter(|p| !p.docs.is_empty()).count();
        // Project Rows: always, so a new file visibly lands beside the
        // others. Separate Tabs: only in the rare tab that holds several.
        if !self.rows_mode && self.total_docs() <= 1 {
            return;
        }
        let mut actions = Vec::new();
        let multi = with_docs > 1;
        for (pi, p) in self.projects.iter().enumerate() {
            if p.docs.is_empty() {
                continue;
            }
            let row_active = pi == self.active;
            let fill = if row_active && multi {
                theme.surface
            } else {
                egui::Color32::TRANSPARENT
            };
            egui::Frame::new()
                .fill(fill)
                .inner_margin(egui::Margin::symmetric(6, 2))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        let name = p.name(s);
                        let colour = if row_active {
                            theme.accent
                        } else {
                            theme.text_muted
                        };
                        let icon = if p.root.is_some() {
                            ph::FOLDER
                        } else {
                            ph::FILES
                        };
                        let chip = ui
                            .allocate_ui_with_layout(
                                egui::vec2(120.0, 20.0),
                                egui::Layout::left_to_right(egui::Align::Center),
                                |ui| {
                                    ui.set_min_width(120.0);
                                    ui.add(
                                        egui::Label::new(
                                            egui::RichText::new(format!("{icon} {name}"))
                                                .strong()
                                                .color(colour),
                                        )
                                        .truncate()
                                        .sense(egui::Sense::click()),
                                    )
                                    .on_hover_text(
                                        p.root
                                            .as_ref()
                                            .map(|r| r.display().to_string())
                                            .unwrap_or_else(|| s.t("editor-other-files-tip")),
                                    )
                                },
                            )
                            .inner;
                        if chip.clicked() {
                            actions.push(RowAction::Activate(pi, p.active));
                        }
                        chip.context_menu(|ui| {
                            if ui.button(s.t("editor-close-project")).clicked() {
                                actions.push(RowAction::CloseProject(pi));
                                ui.close();
                            }
                        });
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing.x = 2.0;
                            for (di, d) in p.docs.iter().enumerate() {
                                let selected = row_active && di == p.active;
                                let name = if d.path.is_some() {
                                    d.file_name()
                                } else {
                                    s.t("editor-untitled")
                                };
                                let label = if d.buf.dirty() {
                                    format!("{name} ●")
                                } else {
                                    name.clone()
                                };
                                let mut text = egui::RichText::new(label).size(13.0);
                                if !selected && !(di == p.active && !row_active) {
                                    text = text.color(theme.text_muted);
                                }
                                let icon = egui::Image::new(fileicons::for_file(&name))
                                    .fit_to_exact_size(egui::vec2(14.0, 14.0));
                                let r = ui.add(
                                    egui::Button::image_and_text(icon, text)
                                        .selected(selected)
                                        .frame_when_inactive(di == p.active),
                                );
                                let r = match &d.path {
                                    Some(path) => r.on_hover_text(path.display().to_string()),
                                    None => r,
                                };
                                if r.clicked() {
                                    actions.push(RowAction::Activate(pi, di));
                                }
                                if r.middle_clicked() {
                                    actions.push(RowAction::Close(pi, di));
                                }
                                r.context_menu(|ui| {
                                    if ui.button(s.t("editor-close-file")).clicked() {
                                        actions.push(RowAction::Close(pi, di));
                                        ui.close();
                                    }
                                    if ui.button(s.t("editor-close-others")).clicked() {
                                        actions.push(RowAction::CloseOthers(pi, di));
                                        ui.close();
                                    }
                                    if let Some(path) = &d.path
                                        && ui.button(s.t("editor-copy-path")).clicked()
                                    {
                                        actions.push(RowAction::CopyPath(
                                            path.to_string_lossy().to_string(),
                                        ));
                                        ui.close();
                                    }
                                });
                                if ui
                                    .add(
                                        egui::Button::new(egui::RichText::new(ph::X).size(11.0))
                                            .frame(false),
                                    )
                                    .tip(s.t("editor-close-file"))
                                    .clicked()
                                {
                                    actions.push(RowAction::Close(pi, di));
                                }
                                ui.add_space(4.0);
                            }
                        });
                    });
                });
        }
        ui.separator();
        for a in actions {
            match a {
                RowAction::Activate(pi, di) => self.activate(pi, di),
                RowAction::Close(pi, di) => self.request_close(CloseTarget::File(pi, di), s),
                RowAction::CloseOthers(pi, di) => {
                    self.request_close(CloseTarget::Others(pi, di), s)
                }
                RowAction::CloseProject(pi) => self.request_close(CloseTarget::Project(pi), s),
                RowAction::CopyPath(p) => ui.ctx().copy_text(p),
            }
        }
    }

    fn close_dialog(&mut self, ui: &mut egui::Ui, s: &Services) {
        let Some(target) = self.confirm_close else {
            return;
        };
        let (theme, _) = Self::theme(s);
        let names: Vec<String> = match target {
            CloseTarget::File(pi, di) => self
                .projects
                .get(pi)
                .and_then(|p| p.docs.get(di))
                .map(|d| vec![d.file_name()])
                .unwrap_or_default(),
            CloseTarget::Others(pi, di) => self
                .projects
                .get(pi)
                .map(|p| {
                    p.docs
                        .iter()
                        .enumerate()
                        .filter(|(i, d)| *i != di && d.buf.dirty())
                        .map(|(_, d)| d.file_name())
                        .collect()
                })
                .unwrap_or_default(),
            CloseTarget::Project(pi) => self
                .projects
                .get(pi)
                .map(|p| {
                    p.docs
                        .iter()
                        .filter(|d| d.buf.dirty())
                        .map(|d| d.file_name())
                        .collect()
                })
                .unwrap_or_default(),
        };
        let names: Vec<String> = names
            .into_iter()
            .map(|n| {
                if n.is_empty() {
                    s.t("editor-untitled")
                } else {
                    n
                }
            })
            .collect();
        match hs_ui::dialog::confirm(
            ui.ctx(),
            &theme,
            "editor-close-unsaved",
            &s.t("editor-unsaved-title"),
            &s.i18n
                .t_args("editor-unsaved-body", &[("names", &names.join(", "))]),
            &s.t("editor-discard-close"),
            &s.t("cancel"),
            true,
        ) {
            hs_ui::dialog::DialogResult::Ok => {
                self.confirm_close = None;
                self.do_close(target, s);
            }
            hs_ui::dialog::DialogResult::Cancel => self.confirm_close = None,
            hs_ui::dialog::DialogResult::Open => {}
        }
    }

    fn context_menu(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        let Some((path, pos)) = self.menu.clone() else {
            return;
        };
        let s = cx.services;
        let is_dir = path.is_dir();
        let root_of = self
            .projects
            .iter()
            .position(|p| p.root.as_deref() == Some(path.as_path()));
        let mut close = false;
        let area = egui::Area::new(self.id.with("ctx-menu"))
            .order(egui::Order::Foreground)
            .fixed_pos(pos)
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_min_width(200.0);
                    ui.set_max_width(240.0);
                    let item = |ui: &mut egui::Ui, label: String| {
                        ui.add(
                            egui::Button::new(label)
                                .frame(false)
                                .min_size(egui::vec2(ui.available_width(), 0.0)),
                        )
                        .clicked()
                    };
                    if !is_dir && item(ui, s.t("editor-open-in-new-tab")) {
                        cx.open_new_tab(
                            "editor",
                            Some(OpenRequest::File {
                                uri: path.to_string_lossy().to_string(),
                                line: None,
                            }),
                        );
                        close = true;
                    }
                    if item(ui, s.t("editor-open-terminal-here")) {
                        let dir = if is_dir {
                            path.clone()
                        } else {
                            path.parent().map(Path::to_path_buf).unwrap_or(path.clone())
                        };
                        cx.open(
                            "terminal",
                            Some(OpenRequest::Terminal {
                                cwd: Some(dir.to_string_lossy().to_string()),
                                host: None,
                                profile: None,
                            }),
                        );
                        close = true;
                    }
                    ui.separator();
                    if is_dir {
                        if item(ui, s.t("editor-new-file")) {
                            self.prompt = Some(Prompt {
                                kind: PromptKind::NewFile(path.clone()),
                                text: String::new(),
                                focus: true,
                            });
                            close = true;
                        }
                        if item(ui, s.t("editor-new-folder")) {
                            self.prompt = Some(Prompt {
                                kind: PromptKind::NewFolder(path.clone()),
                                text: String::new(),
                                focus: true,
                            });
                            close = true;
                        }
                    }
                    if let Some(pi) = root_of {
                        if item(ui, s.t("editor-close-project")) {
                            self.request_close(CloseTarget::Project(pi), s);
                            close = true;
                        }
                    } else {
                        if item(ui, s.t("editor-rename")) {
                            let name = path
                                .file_name()
                                .map(|n| n.to_string_lossy().to_string())
                                .unwrap_or_default();
                            self.prompt = Some(Prompt {
                                kind: PromptKind::Rename(path.clone()),
                                text: name,
                                focus: true,
                            });
                            close = true;
                        }
                        if item(ui, s.t("editor-delete")) {
                            self.confirm_delete = Some(path.clone());
                            close = true;
                        }
                    }
                    ui.separator();
                    if item(ui, s.t("editor-copy-path")) {
                        ui.ctx().copy_text(path.to_string_lossy().to_string());
                        close = true;
                    }
                    if item(ui, s.t("editor-copy-relative-path")) {
                        ui.ctx().copy_text(self.relative(&path));
                        close = true;
                    }
                    if item(ui, s.t("editor-reveal")) {
                        reveal(&path);
                        close = true;
                    }
                });
            });
        let clicked_outside = ui.input(|i| i.pointer.any_pressed())
            && !ui
                .input(|i| i.pointer.interact_pos())
                .is_some_and(|p| area.response.rect.contains(p));
        if close || clicked_outside || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.menu = None;
        }
    }

    fn prompt_modal(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        let Some(mut prompt) = self.prompt.take() else {
            return;
        };
        let s = cx.services;
        let title = match &prompt.kind {
            PromptKind::NewFile(_) => s.t("editor-new-file"),
            PromptKind::NewFolder(_) => s.t("editor-new-folder"),
            PromptKind::Rename(_) => s.t("editor-rename"),
        };
        let mut done = false;
        let mut cancel = false;
        let modal = egui::Modal::new(self.id.with("prompt")).show(ui.ctx(), |ui| {
            ui.set_width(380.0);
            ui.heading(title);
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(s.t("editor-name-prompt"));
                let r = ui
                    .add(egui::TextEdit::singleline(&mut prompt.text).desired_width(f32::INFINITY));
                if prompt.focus {
                    r.request_focus();
                    prompt.focus = false;
                }
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    done = true;
                }
            });
            ui.add_space(10.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(s.t("ok")).clicked() {
                    done = true;
                }
                if ui.button(s.t("cancel")).clicked() {
                    cancel = true;
                }
            });
        });
        if modal.should_close() {
            cancel = true;
        }
        if done && !prompt.text.trim().is_empty() {
            let name = prompt.text.trim();
            let result = match &prompt.kind {
                PromptKind::NewFile(dir) => {
                    let p = dir.join(name);
                    if p.exists() {
                        Err("exists".to_string())
                    } else {
                        if let Some(parent) = p.parent() {
                            let _ = std::fs::create_dir_all(parent);
                        }
                        std::fs::write(&p, b"")
                            .map(|_| Some(p))
                            .map_err(|e| e.to_string())
                    }
                }
                PromptKind::NewFolder(dir) => std::fs::create_dir_all(dir.join(name))
                    .map(|_| None)
                    .map_err(|e| e.to_string()),
                PromptKind::Rename(old) => {
                    let new = old.with_file_name(name);
                    std::fs::rename(old, &new)
                        .map(|_| {
                            self.renamed(old, &new);
                            None
                        })
                        .map_err(|e| e.to_string())
                }
            };
            match result {
                Ok(Some(new_file)) => self.open_path(new_file, None, cx),
                Ok(None) => {}
                Err(e) => cx.toast(ToastLevel::Error, e),
            }
            self.refresh_tree();
        } else if !done && !cancel {
            self.prompt = Some(prompt);
        }
    }

    /// A file or folder was renamed in the tree: open files follow it.
    fn renamed(&mut self, old: &Path, new: &Path) {
        for p in &mut self.projects {
            for d in &mut p.docs {
                let Some(path) = d.path.clone() else { continue };
                if let Ok(rest) = path.strip_prefix(old) {
                    let moved = if rest.as_os_str().is_empty() {
                        new.to_path_buf()
                    } else {
                        new.join(rest)
                    };
                    if path == old {
                        d.view
                            .set_language(languages::detect(&moved.to_string_lossy()));
                    }
                    d.path = Some(moved);
                }
            }
        }
        self.select_in_tree();
    }

    fn delete_dialog(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        let Some(path) = self.confirm_delete.clone() else {
            return;
        };
        let s = cx.services;
        let (theme, _) = Self::theme(s);
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        match hs_ui::dialog::confirm(
            ui.ctx(),
            &theme,
            "editor-delete",
            &s.t("editor-delete"),
            &s.i18n.t_args("editor-delete-confirm", &[("name", &name)]),
            &s.t("editor-delete"),
            &s.t("cancel"),
            true,
        ) {
            hs_ui::dialog::DialogResult::Ok => {
                let r = if path.is_dir() {
                    std::fs::remove_dir_all(&path)
                } else {
                    std::fs::remove_file(&path)
                };
                if let Err(e) = r {
                    cx.toast(ToastLevel::Error, e.to_string());
                }
                self.confirm_delete = None;
                self.refresh_tree();
            }
            hs_ui::dialog::DialogResult::Cancel => self.confirm_delete = None,
            hs_ui::dialog::DialogResult::Open => {}
        }
    }
}

impl Tool for EditorTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &DESCRIPTOR
    }

    fn title(&self) -> Cow<'_, str> {
        let n = self.doc().file_name();
        if n.is_empty() {
            Cow::Borrowed("Untitled")
        } else {
            Cow::Owned(n)
        }
    }

    fn dirty(&self) -> bool {
        self.projects
            .iter()
            .any(|p| p.docs.iter().any(|d| d.buf.dirty()))
    }

    fn toolbar(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        let s = cx.services;
        self.tab_id = cx.tab_id;
        if ui
            .button(format!("{} {}", ph::FOLDER_OPEN, s.t("editor-open-folder")))
            .tip(s.t("editor-open-folder-tip"))
            .clicked()
        {
            self.open_folder_dialog(cx);
        }
        if ui
            .button(format!("{} {}", ph::FILE, s.t("editor-open-file")))
            .clicked()
        {
            self.open_file_dialog(cx);
        }
        let read_only = self.doc().view.read_only;
        ui.add_enabled_ui(!read_only, |ui| {
            if ui
                .button(format!("{} {}", ph::FLOPPY_DISK, s.t("editor-save")))
                .clicked()
            {
                self.save(cx);
            }
        });
        if ui.button(s.t("editor-save-as")).clicked() {
            self.save_as(cx);
        }
        ui.separator();
        if ui
            .button(format!("{} {}", ph::MAGNIFYING_GLASS, s.t("editor-find")))
            .clicked()
        {
            let d = self.doc_mut();
            let sel = hs_editor::view::selected_text(&d.buf, &d.cursors);
            d.view.open_find(false, Some(sel));
        }
        if ui.button(s.t("editor-replace")).clicked() {
            let d = self.doc_mut();
            let sel = hs_editor::view::selected_text(&d.buf, &d.cursors);
            d.view.open_find(true, Some(sel));
        }
        if ui.button(s.t("editor-goto")).clicked() {
            let d = self.doc_mut();
            d.view.goto_open = true;
            d.view.goto_text.clear();
        }
        ui.separator();
        let mut ai_open = self.ai.open;
        if ui
            .toggle_value(
                &mut ai_open,
                format!("{} {}", ph::SPARKLE, s.t("ai-assistant")),
            )
            .tip(s.t("ai-toggle-tip"))
            .clicked()
        {
            self.ai.toggle();
        }
        if self.doc().view.language.is_markdown {
            ui.separator();
            let d = self.doc_mut();
            ui.toggle_value(
                &mut d.preview,
                format!("{} {}", ph::EYE, s.t("editor-preview")),
            );
        }
    }

    fn sidebar(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) -> bool {
        let s = cx.services;
        let (theme, _) = Self::theme(s);
        let has_folder = !self.nodes.is_empty();
        hs_ui::form::header_bar(ui, &theme, |ui| {
            ui.label(
                egui::RichText::new(s.t("editor-project"))
                    .strong()
                    .size(15.0),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if has_folder {
                    if self.projects[self.active].root.is_some()
                        && ui
                            .add(|ui: &mut egui::Ui| {
                                hs_ui::form::icon_button(ui, &theme, ph::X, 16.0, false)
                            })
                            .tip(s.t("editor-close-project-tip"))
                            .clicked()
                    {
                        self.request_close(CloseTarget::Project(self.active), s);
                    }
                    if ui
                        .add(|ui: &mut egui::Ui| {
                            hs_ui::form::icon_button(ui, &theme, ph::ARROW_CLOCKWISE, 16.0, false)
                        })
                        .tip(s.t("editor-refresh"))
                        .clicked()
                    {
                        self.refresh_tree();
                    }
                    let root = self.projects[self.active]
                        .root
                        .clone()
                        .or_else(|| self.projects.iter().find_map(|p| p.root.clone()));
                    if ui
                        .add(|ui: &mut egui::Ui| {
                            hs_ui::form::icon_button(ui, &theme, ph::FILE_PLUS, 16.0, false)
                        })
                        .tip(s.t("editor-new-file"))
                        .clicked()
                        && let Some(root) = root
                    {
                        let dir = self
                            .tree
                            .selected
                            .as_ref()
                            .map(PathBuf::from)
                            .map(|p| {
                                if p.is_dir() {
                                    p
                                } else {
                                    p.parent().map(Path::to_path_buf).unwrap_or(root.clone())
                                }
                            })
                            .unwrap_or(root);
                        self.prompt = Some(Prompt {
                            kind: PromptKind::NewFile(dir),
                            text: String::new(),
                            focus: true,
                        });
                    }
                }
                if ui
                    .add(|ui: &mut egui::Ui| {
                        hs_ui::form::icon_button(ui, &theme, ph::FOLDER_PLUS, 16.0, false)
                    })
                    .tip(s.t("editor-open-folder-tip"))
                    .clicked()
                {
                    self.open_folder_dialog(cx);
                }
            });
        });
        ui.add_space(4.0);
        if !has_folder {
            ui.label(egui::RichText::new(s.t("editor-no-folder")).weak());
            return true;
        }
        let resp = egui::ScrollArea::both()
            .id_salt(self.id.with("tree"))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                hs_ui::tree::show(ui, &theme, &mut self.tree, &self.nodes)
            })
            .inner;
        for id in resp.load {
            let dir = PathBuf::from(&id);
            set_children(&mut self.nodes, &id, load_children(&dir));
        }
        if resp.clicked.is_some() {
            // A folder may have been expanded or collapsed.
            self.remember_projects();
        }
        if let Some(id) = resp.clicked.or(resp.double_clicked) {
            let p = PathBuf::from(&id);
            if p.is_file() {
                self.open_path(p, None, cx);
            }
        }
        if let Some(id) = resp.secondary
            && let Some(pos) = ui.input(|i| i.pointer.interact_pos())
        {
            self.menu = Some((PathBuf::from(id), pos));
        }
        self.context_menu(ui, cx);
        true
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        let s = cx.services;
        self.tab_id = cx.tab_id;
        self.poll_watch(s);
        self.prompt_modal(ui, cx);
        self.delete_dialog(ui, cx);
        self.close_dialog(ui, s);
        self.ensure_doc(s);
        let (theme, ed_theme) = Self::theme(s);
        self.file_rows(ui, s, &theme);
        self.doc_mut().disk_banner(ui, s);
        egui::Panel::bottom(self.id.with("status"))
            .exact_size(22.0)
            .resizable(false)
            .show(ui, |ui| self.doc_mut().status_row(ui, s));
        if self.ai.open {
            let file_label = self.ai_file_label();
            let pa = self.active;
            let p = &mut self.projects[pa];
            let d = &mut p.docs[p.active];
            let mut ed = ai_panel::EditorCtx {
                read_only: d.view.read_only,
                file_label,
                language: d.view.language.name,
                buf: &mut d.buf,
                cursors: &mut d.cursors,
            };
            let ai = &mut self.ai;
            egui::Panel::right(self.id.with("ai"))
                .default_size(380.0)
                .size_range(260.0..=(ui.available_width() - 200.0).max(260.0))
                .resizable(true)
                .show(ui, |ui| ai.ui(ui, s, &theme, &mut ed));
        }
        let d = self.doc_mut();
        if d.preview && d.view.language.is_markdown {
            let w = ui.available_width() / 2.0;
            egui::Panel::right(d.view.id.with("preview"))
                .default_size(w)
                .size_range(160.0..=ui.available_width() - 160.0)
                .resizable(true)
                .show(ui, |ui| d.preview_ui(ui, ed_theme.dark, &theme));
        }
        let r = d.view.show(ui, &mut d.buf, &mut d.cursors, &ed_theme);
        if r.save_requested {
            self.save(cx);
        }
    }

    fn commands(&self) -> Vec<CommandDescriptor> {
        let c = |id: &str, title: &str, sc: Option<&str>| CommandDescriptor {
            id: id.into(),
            title: title.into(),
            shortcut_action: sc.map(str::to_string),
        };
        vec![
            c("file.open", "Editor: Open File", Some("file.open")),
            c("file.open_folder", "Editor: Open Folder as Project", None),
            c("file.new", "Editor: New File", Some("file.new")),
            c("file.save", "Editor: Save", Some("file.save")),
            c("file.save_as", "Editor: Save As", Some("file.save_as")),
            c(
                "editor.close_file",
                "Editor: Close File",
                Some("editor.close_file"),
            ),
            c(
                "editor.next_file",
                "Editor: Next Open File",
                Some("editor.next_file"),
            ),
            c(
                "editor.prev_file",
                "Editor: Previous Open File",
                Some("editor.prev_file"),
            ),
            c("editor.close_project", "Editor: Close Project", None),
            c("editor.reload", "Editor: Reload From Disk", None),
            c("editor.find", "Editor: Find", None),
            c("editor.replace", "Editor: Replace", None),
            c("editor.goto", "Editor: Go to Line", None),
            c(
                "editor.preview",
                "Editor: Toggle Markdown Preview",
                Some("editor.preview"),
            ),
            c("editor.terminal_here", "Editor: Open Terminal Here", None),
            c("editor.ai", "AI: Toggle Assistant", Some("editor.ai")),
            c("editor.ai_explain", "AI: Explain Selection or File", None),
            c("editor.ai_review", "AI: Review Selection or File", None),
            c("editor.ai_improve", "AI: Improve Selection or File", None),
        ]
    }

    fn run_command(&mut self, id: &str, cx: &mut ToolContext) -> bool {
        let s = cx.services;
        match id {
            "file.open" => self.open_file_dialog(cx),
            "file.open_folder" => self.open_folder_dialog(cx),
            "file.new" if self.rows_mode => self.new_file(s),
            "file.new" => cx.open("editor", None),
            "file.save" => {
                self.save(cx);
            }
            "file.save_as" => {
                self.save_as(cx);
            }
            "editor.close_file" => {
                let t = CloseTarget::File(self.active, self.projects[self.active].active);
                self.request_close(t, s);
            }
            "editor.next_file" => self.cycle(true),
            "editor.prev_file" => self.cycle(false),
            "editor.close_project" => self.request_close(CloseTarget::Project(self.active), s),
            "editor.reload" => self.doc_mut().reload(s),
            "editor.find" | "editor.replace" => {
                let d = self.doc_mut();
                let sel = hs_editor::view::selected_text(&d.buf, &d.cursors);
                d.view.open_find(id == "editor.replace", Some(sel));
            }
            "editor.goto" => {
                let d = self.doc_mut();
                d.view.goto_open = true;
                d.view.goto_text.clear();
            }
            "editor.preview" => {
                let d = self.doc_mut();
                if d.view.language.is_markdown {
                    d.preview = !d.preview;
                }
            }
            "editor.ai" => self.ai.toggle(),
            "editor.ai_explain" | "editor.ai_review" | "editor.ai_improve" => {
                let prompt = match id {
                    "editor.ai_explain" => "ai-explain-prompt",
                    "editor.ai_review" => "ai-review-prompt",
                    _ => "ai-improve-prompt",
                };
                let file_label = self.ai_file_label();
                let pa = self.active;
                let p = &mut self.projects[pa];
                let d = &mut p.docs[p.active];
                let mut ed = ai_panel::EditorCtx {
                    read_only: d.view.read_only,
                    file_label,
                    language: d.view.language.name,
                    buf: &mut d.buf,
                    cursors: &mut d.cursors,
                };
                self.ai.quick(s, &mut ed, prompt);
            }
            "editor.terminal_here" => {
                let dir = self
                    .doc()
                    .path
                    .as_ref()
                    .and_then(|p| p.parent().map(Path::to_path_buf))
                    .or(self.projects[self.active].root.clone());
                cx.open(
                    "terminal",
                    Some(OpenRequest::Terminal {
                        cwd: dir.map(|d| d.to_string_lossy().to_string()),
                        host: None,
                        profile: None,
                    }),
                );
            }
            _ => return false,
        }
        true
    }

    fn current_uri(&self) -> Option<String> {
        self.doc()
            .path
            .as_ref()
            .map(|p| p.to_string_lossy().to_string())
    }

    fn uri_affinity(&self, uri: &str) -> Option<u8> {
        let path = Path::new(uri.strip_prefix("file://").unwrap_or(uri));
        if self.find_doc(path).is_some() {
            Some(0)
        } else if path.is_dir() {
            // A folder becomes another project of an existing editor tab.
            Some(2)
        } else if !self.rows_mode {
            // Separate Tabs: any other file gets a tab of its own.
            None
        } else if self.project_for(path).is_some() {
            Some(1)
        } else if path.is_absolute() {
            // Any other local file joins "Other Files" here rather than
            // starting a new editor tab.
            Some(2)
        } else {
            None
        }
    }

    fn quick_open_items(&self) -> Vec<QuickOpenItem> {
        let roots = self.projects.iter().filter(|p| p.root.is_some()).count();
        let mut out = Vec::new();
        for p in &self.projects {
            let Ok(index) = p.index.lock() else {
                continue;
            };
            let name = p
                .root
                .as_ref()
                .and_then(|r| r.file_name())
                .map(|n| n.to_string_lossy().to_string());
            for f in index.iter() {
                let mut subtitle = f.parent().map(|d| p.relative(d)).unwrap_or_default();
                if roots > 1
                    && let Some(n) = &name
                {
                    subtitle = if subtitle.is_empty() {
                        n.clone()
                    } else {
                        format!("{n}/{subtitle}")
                    };
                }
                out.push(QuickOpenItem {
                    id: f.to_string_lossy().to_string(),
                    title: f
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default(),
                    subtitle,
                });
            }
        }
        out
    }

    fn save_state(&self) -> serde_json::Value {
        let projects: Vec<ProjectState> = self
            .projects
            .iter()
            .map(|p| {
                let mut active = 0;
                let mut files = Vec::new();
                for (di, d) in p.docs.iter().enumerate() {
                    if let Some(path) = &d.path {
                        if di == p.active {
                            active = files.len();
                        }
                        files.push(FileState {
                            path: path.clone(),
                            cursor: d.cursors.primary().head,
                            preview: d.preview,
                        });
                    }
                }
                ProjectState {
                    root: p.root.clone(),
                    expanded: p
                        .root
                        .as_ref()
                        .map(|r| self.expanded_dirs(r))
                        .unwrap_or_default(),
                    files,
                    active,
                }
            })
            .collect();
        let st = State {
            projects,
            active: self.active,
            ..Default::default()
        };
        serde_json::to_value(&st).unwrap_or_default()
    }

    fn open(&mut self, item: OpenRequest, cx: &mut ToolContext) -> bool {
        match item {
            OpenRequest::File { uri, line } => {
                let path = PathBuf::from(uri.strip_prefix("file://").unwrap_or(&uri));
                self.open_path(path, line, cx);
                true
            }
            OpenRequest::Text { mime, body } => {
                let hint = match mime.as_str() {
                    m if m.contains("json") => Some("x.json"),
                    m if m.contains("yaml") => Some("x.yaml"),
                    m if m.contains("xml") => Some("x.xml"),
                    m if m.contains("html") => Some("x.html"),
                    m if m.contains("sql") => Some("x.sql"),
                    m if m.contains("markdown") => Some("x.md"),
                    _ => None,
                };
                if !self.doc().is_placeholder() {
                    if !self.rows_mode {
                        cx.open("editor", Some(OpenRequest::Text { mime, body }));
                        return true;
                    }
                    self.new_file(cx.services);
                }
                self.doc_mut().set_text(&body, hint);
                true
            }
            _ => false,
        }
    }

    fn on_event(&mut self, ev: &Event, cx: &mut ToolContext) {
        if let Event::SettingsChanged = ev {
            let st = cx.services.settings.read();
            self.auto_reload = st.editor.auto_reload;
            self.rows_mode = st.editor.open_files_in == hs_core::settings::OpenFilesIn::Rows;
            self.trim_trailing = st.editor.trim_trailing_whitespace;
            self.final_newline = st.editor.insert_final_newline;
            drop(st);
            for p in &mut self.projects {
                for d in &mut p.docs {
                    d.apply_settings(cx.services);
                }
            }
        }
    }
}

const LANGUAGE_MENU: &[(&str, &str)] = &[
    ("Plain Text", "txt"),
    ("Rust", "rs"),
    ("TypeScript", "ts"),
    ("JavaScript", "js"),
    ("JSON", "json"),
    ("YAML", "yaml"),
    ("TOML", "toml"),
    ("INI", "ini"),
    ("Markdown", "md"),
    ("HTML", "html"),
    ("XML", "xml"),
    ("CSS", "css"),
    ("SQL", "sql"),
    ("Shell", "sh"),
    ("PowerShell", "ps1"),
    ("Batch", "bat"),
    ("Python", "py"),
    ("Go", "go"),
    ("Java", "java"),
    ("Kotlin", "kt"),
    ("C#", "cs"),
    ("C", "c"),
    ("C++", "cpp"),
    ("PHP", "php"),
    ("Ruby", "rb"),
    ("Lua", "lua"),
    ("HCL", "hcl"),
    ("GraphQL", "graphql"),
    ("Protobuf", "proto"),
    ("Diff", "diff"),
    ("Log", "log"),
    ("CSV", "csv"),
];

fn is_writable(meta: &std::fs::Metadata) -> bool {
    !meta.permissions().readonly()
}

fn human_size(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = n as f64;
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{n} B")
    } else {
        format!("{v:.1} {}", UNITS[i])
    }
}

fn load_children(dir: &Path) -> Vec<TreeNode> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut dirs = Vec::new();
    let mut files = Vec::new();
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
        let id = e.path().to_string_lossy().to_string();
        if is_dir {
            if name == ".git" {
                continue;
            }
            let mut n = TreeNode::leaf(id, name.clone(), fileicons::for_folder(&name));
            n.leaf = false;
            n.children = None;
            dirs.push(n);
        } else {
            files.push(TreeNode::leaf(id, name.clone(), fileicons::for_file(&name)));
        }
    }
    dirs.sort_by_key(|n| n.label.to_lowercase());
    files.sort_by_key(|n| n.label.to_lowercase());
    dirs.extend(files);
    dirs
}

fn set_children(nodes: &mut [TreeNode], id: &str, children: Vec<TreeNode>) -> bool {
    let mut slot = Some(children);
    set_children_inner(nodes, id, &mut slot)
}

fn set_children_inner(nodes: &mut [TreeNode], id: &str, slot: &mut Option<Vec<TreeNode>>) -> bool {
    for n in nodes.iter_mut() {
        if n.id == id {
            // Keep what was already loaded below each surviving child, so a
            // refresh (or a file saved in the folder) never collapses an
            // expanded subfolder.
            let mut old: std::collections::HashMap<String, Vec<TreeNode>> = n
                .children
                .take()
                .unwrap_or_default()
                .into_iter()
                .filter_map(|c| c.children.map(|ch| (c.id, ch)))
                .collect();
            let mut new = slot.take();
            if let Some(new) = new.as_mut() {
                for c in new.iter_mut() {
                    if !c.leaf
                        && c.children.is_none()
                        && let Some(ch) = old.remove(&c.id)
                    {
                        c.children = Some(ch);
                    }
                }
            }
            n.children = new;
            return true;
        }
        if let Some(c) = n.children.as_mut()
            && set_children_inner(c, id, slot)
        {
            return true;
        }
    }
    false
}

fn node_has_children(nodes: &[TreeNode], id: &str) -> bool {
    for n in nodes {
        if n.id == id {
            return n.children.is_some();
        }
        if let Some(c) = &n.children
            && node_has_children(c, id)
        {
            return true;
        }
    }
    false
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>, cap: usize) {
    if out.len() >= cap {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = rd.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        if out.len() >= cap {
            return;
        }
        let name = e.file_name().to_string_lossy().to_string();
        let ft = e.file_type();
        if ft.as_ref().map(|t| t.is_dir()).unwrap_or(false) {
            if SKIP_DIRS.contains(&name.as_str()) {
                continue;
            }
            walk(&e.path(), out, cap);
        } else if ft.map(|t| t.is_file()).unwrap_or(false) {
            out.push(e.path());
        }
    }
}

fn reveal(path: &Path) {
    #[cfg(windows)]
    {
        let arg = if path.is_dir() {
            path.to_string_lossy().to_string()
        } else {
            format!("/select,{}", path.display())
        };
        let _ = std::process::Command::new("explorer").arg(arg).spawn();
    }
    #[cfg(not(windows))]
    {
        let target = if path.is_dir() {
            path.to_path_buf()
        } else {
            path.parent().map(Path::to_path_buf).unwrap_or_default()
        };
        let _ = std::process::Command::new("xdg-open").arg(target).spawn();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tree_children_sort_folders_first_and_nested_set_works() {
        let tmp = std::env::temp_dir().join(format!("hs-editor-test-{}", hs_core::new_id()));
        std::fs::create_dir_all(tmp.join("src")).unwrap();
        std::fs::create_dir_all(tmp.join(".git")).unwrap();
        std::fs::write(tmp.join("b.txt"), "b").unwrap();
        std::fs::write(tmp.join("a.rs"), "a").unwrap();
        std::fs::write(tmp.join("src/main.rs"), "fn main() {}").unwrap();
        let children = load_children(&tmp);
        let labels: Vec<&str> = children.iter().map(|n| n.label.as_str()).collect();
        assert_eq!(labels, vec!["src", "a.rs", "b.txt"]);
        let root_id = tmp.to_string_lossy().to_string();
        let mut nodes = vec![TreeNode::branch(
            root_id,
            "root",
            fileicons::for_folder("root"),
            children,
        )];
        let src_id = tmp.join("src").to_string_lossy().to_string();
        assert!(!node_has_children(&nodes, &src_id));
        assert!(set_children(
            &mut nodes,
            &src_id,
            load_children(&tmp.join("src"))
        ));
        assert!(node_has_children(&nodes, &src_id));
        // Reloading the parent (a refresh, a file saved there) keeps the
        // expanded subfolder's children instead of collapsing it.
        let root_id = tmp.to_string_lossy().to_string();
        std::fs::write(tmp.join("c.txt"), "c").unwrap();
        assert!(set_children(&mut nodes, &root_id, load_children(&tmp)));
        assert!(node_has_children(&nodes, &src_id));
        std::fs::remove_file(tmp.join("c.txt")).unwrap();
        let mut index = Vec::new();
        walk(&tmp, &mut index, 100);
        assert_eq!(index.len(), 3);
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn one_file_layouts_become_projects() {
        let st: State = serde_json::from_value(serde_json::json!({
            "root": "/p", "path": "/p/a.rs", "cursor": 5, "expanded": ["/p/src"]
        }))
        .unwrap();
        let (projects, active) = st.into_projects();
        assert_eq!(active, 0);
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].root.as_deref(), Some(Path::new("/p")));
        assert_eq!(projects[0].expanded, vec![PathBuf::from("/p/src")]);
        assert_eq!(projects[0].files[0].cursor, 5);
        // A file outside its folder goes to "Other Files", which is active.
        let st: State =
            serde_json::from_value(serde_json::json!({ "root": "/p", "path": "/q/b.rs" })).unwrap();
        let (projects, active) = st.into_projects();
        assert_eq!(projects.len(), 2);
        assert!(projects[0].files.is_empty());
        assert_eq!(projects[1].root, None);
        assert_eq!(active, 1);
        // Nothing saved: nothing to restore.
        assert!(State::default().into_projects().0.is_empty());
    }

    #[test]
    fn sizes() {
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(3 * 1024 * 1024), "3.0 MB");
    }
}
