//! Code editor tool (docs/tools/editor.md): one file per tab, a project tree
//! in the sidebar with a file watcher, open/save with encodings and line
//! endings, external-change detection, Markdown preview and quick-open items
//! for the palette.

use hs_ui::tip::TipExt;
use std::borrow::Cow;
use std::path::{Path, PathBuf};
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

/// The folder the last editor tab opened, so new tabs start in the same project.
static LAST_ROOT: parking_lot::Mutex<Option<PathBuf>> = parking_lot::Mutex::new(None);

/// The folders expanded in the last-used project tree, with its root, so a
/// new tab on the same project shows the tree the way the user left it.
static LAST_EXPANDED: parking_lot::Mutex<Option<(PathBuf, Vec<PathBuf>)>> =
    parking_lot::Mutex::new(None);

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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct State {
    path: Option<PathBuf>,
    root: Option<PathBuf>,
    preview: bool,
    cursor: usize,
    /// Folders expanded in the project tree; only the user collapses them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    expanded: Vec<PathBuf>,
}

struct Watch {
    _watcher: notify::RecommendedWatcher,
    rx: mpsc::Receiver<notify::Result<notify::Event>>,
    file_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DiskState {
    Clean,
    Changed,
    Deleted,
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

pub struct EditorTool {
    state: State,
    buf: Buffer,
    cursors: Cursors,
    view: EditorView,
    read_only_reason: Option<String>,
    saved_mtime: Option<SystemTime>,
    just_saved: Option<Instant>,
    disk: DiskState,
    auto_reload: bool,
    trim_trailing: bool,
    final_newline: bool,

    nodes: Vec<TreeNode>,
    tree: TreeState,
    watch: Option<Watch>,
    index: Arc<StdMutex<Vec<PathBuf>>>,
    index_building: Arc<std::sync::atomic::AtomicBool>,
    index_dirty: bool,
    index_built_at: Option<Instant>,

    menu: Option<(PathBuf, egui::Pos2)>,
    prompt: Option<Prompt>,
    confirm_delete: Option<PathBuf>,
    preview_cache: (u64, String),
    tab_id: u64,
    ai: ai_panel::AiPanel,
}

impl EditorTool {
    fn new(services: &Services, mut state: State) -> Self {
        let st = services.settings.read();
        let dark = Theme::by_name(&st.appearance.theme).dark;
        let mut view = EditorView::new(
            egui::Id::new(("hs-editor", hs_core::new_id())),
            languages::plain(),
            dark,
            st.editor.font_size,
        );
        view.right_click_copy_paste = st.editor.right_click_copy_paste;
        let auto_reload = st.editor.auto_reload;
        let trim_trailing = st.editor.trim_trailing_whitespace;
        let final_newline = st.editor.insert_final_newline;
        drop(st);
        if state.root.is_none() {
            state.root = LAST_ROOT.lock().clone();
        }
        if state.expanded.is_empty()
            && let Some((root, dirs)) = LAST_EXPANDED.lock().clone()
            && state.root.as_ref() == Some(&root)
        {
            state.expanded = dirs;
        }
        let mut t = Self {
            state: State::default(),
            buf: Buffer::default(),
            cursors: Cursors::default(),
            view,
            read_only_reason: None,
            saved_mtime: None,
            just_saved: None,
            disk: DiskState::Clean,
            auto_reload,
            trim_trailing,
            final_newline,
            nodes: Vec::new(),
            tree: TreeState::default(),
            watch: None,
            index: Arc::new(StdMutex::new(Vec::new())),
            index_building: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            index_dirty: false,
            index_built_at: None,
            menu: None,
            prompt: None,
            confirm_delete: None,
            preview_cache: (u64::MAX, String::new()),
            tab_id: 0,
            ai: ai_panel::AiPanel::default(),
        };
        t.state.preview = state.preview;
        if let Some(root) = state.root.clone().filter(|r| r.is_dir()) {
            t.set_root(root, services);
            t.expand_dirs(&state.expanded);
        }
        if let Some(p) = state.path.clone().filter(|p| p.is_file()) {
            if let Err(e) = t.load(&p, services) {
                tracing::warn!("editor: could not restore {}: {e}", p.display());
            } else {
                let pos = state.cursor.min(t.buf.len_chars());
                t.cursors.set_single(Cursor::at(pos));
                t.view.scroll_to_cursor();
            }
        }
        t
    }

    /// How the assistant names the open file: relative to the project.
    fn ai_file_label(&self) -> String {
        match &self.state.path {
            Some(p) => self.relative(p),
            None => "untitled".into(),
        }
    }

    fn file_name(&self) -> String {
        self.state
            .path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default()
    }

    fn theme(s: &Services) -> (Theme, EditorTheme) {
        let name = s.settings.read().appearance.theme.clone();
        let ui_theme = Theme::by_name(&name);
        let ed = EditorTheme::for_theme(&ui_theme);
        (ui_theme, ed)
    }

    // ---- files ----------------------------------------------------------------

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
        self.state.path = Some(path.to_path_buf());
        self.state.preview = self.state.preview && self.view.language.is_markdown;
        self.watch_file_dir(path);
        if let Some(root) = &self.state.root
            && path.starts_with(root)
        {
            self.tree.selected = Some(path.to_string_lossy().to_string());
        }
        Ok(())
    }

    fn set_text(&mut self, text: &str, name_hint: Option<&str>) {
        self.buf = Buffer::from_text(text);
        self.cursors = Cursors::default();
        self.state.path = None;
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

    fn prepare_save(&mut self) {
        if !self.trim_trailing && !self.final_newline {
            return;
        }
        let before = self.cursors.snapshot();
        let mut edits = Vec::new();
        if self.trim_trailing {
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
        if self.final_newline
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

    fn save(&mut self, cx: &mut ToolContext) -> bool {
        let Some(path) = self.state.path.clone() else {
            return self.save_as(cx);
        };
        self.prepare_save();
        let bytes = self.buf.to_bytes();
        match hs_core::storage::write_atomic(&path, &bytes) {
            Ok(()) => {
                self.buf.mark_saved();
                self.saved_mtime = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
                self.just_saved = Some(Instant::now());
                self.disk = DiskState::Clean;
                cx.services.events.publish(Event::FileSaved {
                    uri: format!("file://{}", path.display()),
                });
                cx.status(
                    cx.services
                        .i18n
                        .t_args("editor-saved", &[("name", &self.file_name())]),
                );
                true
            }
            Err(e) => {
                let msg = cx.services.i18n.t_args(
                    "editor-save-failed",
                    &[("name", &self.file_name()), ("error", &e.to_string())],
                );
                cx.toast(ToastLevel::Error, msg);
                false
            }
        }
    }

    fn save_as(&mut self, cx: &mut ToolContext) -> bool {
        let mut dlg = rfd::FileDialog::new();
        if let Some(dir) = self
            .state
            .path
            .as_ref()
            .and_then(|p| p.parent())
            .or(self.state.root.as_deref())
        {
            dlg = dlg.set_directory(dir);
        }
        let name = self.file_name();
        if !name.is_empty() {
            dlg = dlg.set_file_name(name);
        }
        let Some(path) = dlg.save_file() else {
            return false;
        };
        self.state.path = Some(path.clone());
        self.view
            .set_language(languages::detect(&path.to_string_lossy()));
        self.view.read_only = false;
        self.read_only_reason = None;
        let ok = self.save(cx);
        if ok {
            self.watch_file_dir(&path);
            self.index_dirty = true;
        }
        ok
    }

    fn open_file_dialog(&mut self, cx: &mut ToolContext) {
        let mut dlg = rfd::FileDialog::new();
        if let Some(dir) = self.state.root.as_deref() {
            dlg = dlg.set_directory(dir);
        }
        if let Some(path) = dlg.pick_file() {
            self.open_path(path, None, cx);
        }
    }

    fn open_folder_dialog(&mut self, cx: &mut ToolContext) {
        let mut dlg = rfd::FileDialog::new();
        if let Some(dir) = self.state.root.as_deref() {
            dlg = dlg.set_directory(dir);
        }
        if let Some(dir) = dlg.pick_folder() {
            self.set_root(dir, cx.services);
        }
    }

    /// Open `path` here when this tab is empty, otherwise in a new editor tab
    /// (which then jumps to `line`).
    fn open_path(&mut self, path: PathBuf, line: Option<u32>, cx: &mut ToolContext) {
        if path.is_dir() {
            self.set_root(path, cx.services);
            return;
        }
        if self.state.path.as_deref() == Some(path.as_path()) {
            // Already showing this file: nothing to load (the shell focused us).
            return;
        }
        let here = self.state.path.is_none() && !self.buf.dirty();
        if here {
            if let Err(e) = self.load(&path, cx.services) {
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
        } else {
            cx.open(
                "editor",
                Some(OpenRequest::File {
                    uri: path.to_string_lossy().to_string(),
                    line,
                }),
            );
        }
    }

    // ---- project tree --------------------------------------------------------

    fn set_root(&mut self, root: PathBuf, services: &Services) {
        *LAST_ROOT.lock() = Some(root.clone());
        self.state.root = Some(root.clone());
        self.tree = TreeState::default();
        let id = root.to_string_lossy().to_string();
        let name = root
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| id.clone());
        let icon = fileicons::for_folder(&name);
        let mut node = TreeNode::branch(id.clone(), name, icon, load_children(&root));
        node.leaf = false;
        self.nodes = vec![node];
        self.tree.expanded.insert(id);
        self.start_watch(services);
        self.rebuild_index(services);
    }

    /// Expand these folders (outermost first, so each parent is loaded
    /// before its child is looked up). Folders gone from disk are skipped.
    fn expand_dirs(&mut self, dirs: &[PathBuf]) {
        let Some(root) = self.state.root.clone() else {
            return;
        };
        let mut dirs: Vec<&PathBuf> = dirs
            .iter()
            .filter(|d| d.starts_with(&root) && d.is_dir())
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

    /// The expanded folders in a stable order, for the saved layout.
    fn expanded_dirs(&self) -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = self.tree.expanded.iter().map(PathBuf::from).collect();
        v.sort();
        v
    }

    fn close_root(&mut self) {
        self.state.root = None;
        self.nodes.clear();
        self.tree = TreeState::default();
        self.watch = None;
        self.index.lock().map(|mut i| i.clear()).ok();
        if let Some(p) = self.state.path.clone() {
            self.watch_file_dir(&p);
        }
    }

    fn start_watch(&mut self, services: &Services) {
        let (tx, rx) = mpsc::channel();
        let waker = services.runtime.waker();
        let watcher = notify::recommended_watcher(move |res| {
            let _ = tx.send(res);
            waker.request_repaint();
        });
        match watcher {
            Ok(mut w) => {
                if let Some(root) = &self.state.root
                    && let Err(e) = w.watch(root, notify::RecursiveMode::Recursive)
                {
                    tracing::warn!("editor: watch {} failed: {e}", root.display());
                }
                self.watch = Some(Watch {
                    _watcher: w,
                    rx,
                    file_dir: None,
                });
                if let Some(p) = self.state.path.clone() {
                    self.watch_file_dir(&p);
                }
            }
            Err(e) => tracing::warn!("editor: file watcher unavailable: {e}"),
        }
    }

    /// Watch the open file's folder when it lies outside the project root.
    fn watch_file_dir(&mut self, path: &Path) {
        let Some(dir) = path.parent().map(Path::to_path_buf) else {
            return;
        };
        let inside_root = self
            .state
            .root
            .as_ref()
            .is_some_and(|r| path.starts_with(r));
        if self.watch.is_none() {
            let (tx, rx) = mpsc::channel();
            match notify::recommended_watcher(move |res| {
                let _ = tx.send(res);
            }) {
                Ok(w) => {
                    self.watch = Some(Watch {
                        _watcher: w,
                        rx,
                        file_dir: None,
                    })
                }
                Err(_) => return,
            }
        }
        let Some(w) = self.watch.as_mut() else { return };
        if let Some(old) = w.file_dir.take() {
            let _ = w._watcher.unwatch(&old);
        }
        if !inside_root
            && w._watcher
                .watch(&dir, notify::RecursiveMode::NonRecursive)
                .is_ok()
        {
            w.file_dir = Some(dir);
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
                    self.index_dirty = true;
                }
                if self.state.path.as_deref() == Some(p.as_path()) {
                    self.on_open_file_event(&ev.kind, services);
                }
            }
        }
        for dir in dirs_to_refresh {
            let id = dir.to_string_lossy().to_string();
            if node_has_children(&self.nodes, &id) {
                set_children(&mut self.nodes, &id, load_children(&dir));
            }
        }
        if self.index_dirty
            && self
                .index_built_at
                .is_none_or(|t| t.elapsed() > Duration::from_secs(2))
        {
            self.rebuild_index(services);
        }
    }

    fn on_open_file_event(&mut self, kind: &notify::EventKind, services: &Services) {
        let Some(path) = self.state.path.clone() else {
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
        let mtime = meta.modified().ok();
        if mtime == self.saved_mtime {
            return;
        }
        if !self.buf.dirty() && self.auto_reload {
            self.reload(services);
        } else {
            self.disk = DiskState::Changed;
        }
    }

    fn reload(&mut self, services: &Services) {
        let Some(path) = self.state.path.clone() else {
            return;
        };
        let pos = self.cursors.primary().head;
        let preview = self.state.preview;
        if self.load(&path, services).is_ok() {
            self.cursors
                .set_single(Cursor::at(pos.min(self.buf.len_chars())));
            self.state.preview = preview && self.view.language.is_markdown;
            self.view.scroll_to_cursor();
        }
    }

    fn rebuild_index(&mut self, _services: &Services) {
        let Some(root) = self.state.root.clone() else {
            return;
        };
        if self
            .index_building
            .swap(true, std::sync::atomic::Ordering::SeqCst)
        {
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
                building.store(false, std::sync::atomic::Ordering::SeqCst);
            })
            .ok();
    }

    fn relative(&self, path: &Path) -> String {
        match &self.state.root {
            Some(r) => path
                .strip_prefix(r)
                .unwrap_or(path)
                .to_string_lossy()
                .replace('\\', "/"),
            None => path.to_string_lossy().to_string(),
        }
    }

    // ---- ui pieces -----------------------------------------------------------

    fn context_menu(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        let Some((path, pos)) = self.menu.clone() else {
            return;
        };
        let s = cx.services;
        let is_dir = path.is_dir();
        let mut close = false;
        let area = egui::Area::new(self.view.id.with("ctx-menu"))
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
                        cx.open(
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
                    let is_root = self.state.root.as_deref() == Some(path.as_path());
                    if !is_root {
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
        let modal = egui::Modal::new(self.view.id.with("prompt")).show(ui.ctx(), |ui| {
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
                            if self.state.path.as_deref() == Some(old.as_path()) {
                                self.state.path = Some(new.clone());
                                self.view
                                    .set_language(languages::detect(&new.to_string_lossy()));
                            }
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

    fn refresh_tree(&mut self) {
        let expanded: Vec<String> = self.tree.expanded.iter().cloned().collect();
        for id in expanded {
            let dir = PathBuf::from(&id);
            if dir.is_dir() && node_has_children(&self.nodes, &id) {
                set_children(&mut self.nodes, &id, load_children(&dir));
            }
        }
        self.index_dirty = true;
    }

    fn status_row(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        let s = cx.services;
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
                if let Some(p) = &self.state.path {
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

    fn disk_banner(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        if self.disk == DiskState::Clean {
            return;
        }
        let s = cx.services;
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
                            if let Some(p) = &self.state.path {
                                self.saved_mtime =
                                    std::fs::metadata(p).and_then(|m| m.modified()).ok();
                            }
                        }
                    });
                });
            });
    }

    fn preview(&mut self, ui: &mut egui::Ui, dark: bool, theme: &Theme) {
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

impl Tool for EditorTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &DESCRIPTOR
    }

    fn title(&self) -> Cow<'_, str> {
        let n = self.file_name();
        if n.is_empty() {
            Cow::Borrowed("Untitled")
        } else {
            Cow::Owned(n)
        }
    }

    fn dirty(&self) -> bool {
        self.buf.dirty()
    }

    fn toolbar(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        let s = cx.services;
        self.tab_id = cx.tab_id;
        if ui
            .button(format!("{} {}", ph::FOLDER_OPEN, s.t("editor-open-folder")))
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
        ui.add_enabled_ui(!self.view.read_only, |ui| {
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
            self.view.open_find(
                false,
                Some(hs_editor::view::selected_text(&self.buf, &self.cursors)),
            );
        }
        if ui.button(s.t("editor-replace")).clicked() {
            self.view.open_find(
                true,
                Some(hs_editor::view::selected_text(&self.buf, &self.cursors)),
            );
        }
        if ui.button(s.t("editor-goto")).clicked() {
            self.view.goto_open = true;
            self.view.goto_text.clear();
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
        if self.view.language.is_markdown {
            ui.separator();
            ui.toggle_value(
                &mut self.state.preview,
                format!("{} {}", ph::EYE, s.t("editor-preview")),
            );
        }
    }

    fn sidebar(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) -> bool {
        let s = cx.services;
        let (theme, _) = Self::theme(s);
        hs_ui::form::header_bar(ui, &theme, |ui| {
            ui.label(
                egui::RichText::new(s.t("editor-project"))
                    .strong()
                    .size(15.0),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if self.state.root.is_some() {
                    if ui
                        .add(|ui: &mut egui::Ui| {
                            hs_ui::form::icon_button(ui, &theme, ph::X, 16.0, false)
                        })
                        .tip(s.t("editor-close-folder"))
                        .clicked()
                    {
                        self.close_root();
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
                    if ui
                        .add(|ui: &mut egui::Ui| {
                            hs_ui::form::icon_button(ui, &theme, ph::FILE_PLUS, 16.0, false)
                        })
                        .tip(s.t("editor-new-file"))
                        .clicked()
                        && let Some(root) = self.state.root.clone()
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
                        hs_ui::form::icon_button(ui, &theme, ph::FOLDER_OPEN, 16.0, false)
                    })
                    .tip(s.t("editor-open-folder"))
                    .clicked()
                {
                    self.open_folder_dialog(cx);
                }
            });
        });
        ui.add_space(4.0);
        if self.state.root.is_none() {
            ui.label(egui::RichText::new(s.t("editor-no-folder")).weak());
            return true;
        }
        let resp = egui::ScrollArea::both()
            .id_salt(self.view.id.with("tree"))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                hs_ui::tree::show(ui, &theme, &mut self.tree, &self.nodes)
            })
            .inner;
        for id in resp.load {
            let dir = PathBuf::from(&id);
            set_children(&mut self.nodes, &id, load_children(&dir));
        }
        if resp.clicked.is_some()
            && let Some(root) = self.state.root.clone()
        {
            *LAST_EXPANDED.lock() = Some((root, self.expanded_dirs()));
        }
        if let Some(id) = resp.clicked {
            let p = PathBuf::from(&id);
            if p.is_file() {
                self.open_path(p, None, cx);
            }
        }
        if let Some(id) = resp.double_clicked {
            let p = PathBuf::from(&id);
            if p.is_file() && self.state.path.as_deref() != Some(p.as_path()) {
                cx.open(
                    "editor",
                    Some(OpenRequest::File {
                        uri: id,
                        line: None,
                    }),
                );
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
        let (theme, ed_theme) = Self::theme(s);
        self.disk_banner(ui, cx);
        egui::Panel::bottom(self.view.id.with("status"))
            .exact_size(22.0)
            .resizable(false)
            .show(ui, |ui| self.status_row(ui, cx));
        if self.ai.open {
            let mut ed = ai_panel::EditorCtx {
                read_only: self.view.read_only,
                file_label: self.ai_file_label(),
                language: self.view.language.name,
                buf: &mut self.buf,
                cursors: &mut self.cursors,
            };
            let ai = &mut self.ai;
            egui::Panel::right(self.view.id.with("ai"))
                .default_size(380.0)
                .size_range(260.0..=(ui.available_width() - 200.0).max(260.0))
                .resizable(true)
                .show(ui, |ui| ai.ui(ui, s, &theme, &mut ed));
        }
        if self.state.preview && self.view.language.is_markdown {
            let w = ui.available_width() / 2.0;
            egui::Panel::right(self.view.id.with("preview"))
                .default_size(w)
                .size_range(160.0..=ui.available_width() - 160.0)
                .resizable(true)
                .show(ui, |ui| self.preview(ui, ed_theme.dark, &theme));
        }
        let r = self
            .view
            .show(ui, &mut self.buf, &mut self.cursors, &ed_theme);
        if r.changed {
            self.state.cursor = self.cursors.primary().head;
        }
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
            c("file.open_folder", "Editor: Open Folder", None),
            c("file.new", "Editor: New File", Some("file.new")),
            c("file.save", "Editor: Save", Some("file.save")),
            c("file.save_as", "Editor: Save As", Some("file.save_as")),
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
        match id {
            "file.open" => self.open_file_dialog(cx),
            "file.open_folder" => self.open_folder_dialog(cx),
            "file.new" => cx.open("editor", None),
            "file.save" => {
                self.save(cx);
            }
            "file.save_as" => {
                self.save_as(cx);
            }
            "editor.reload" => self.reload(cx.services),
            "editor.find" => self.view.open_find(
                false,
                Some(hs_editor::view::selected_text(&self.buf, &self.cursors)),
            ),
            "editor.replace" => self.view.open_find(
                true,
                Some(hs_editor::view::selected_text(&self.buf, &self.cursors)),
            ),
            "editor.goto" => {
                self.view.goto_open = true;
                self.view.goto_text.clear();
            }
            "editor.preview" => {
                if self.view.language.is_markdown {
                    self.state.preview = !self.state.preview;
                }
            }
            "editor.ai" => self.ai.toggle(),
            "editor.ai_explain" | "editor.ai_review" | "editor.ai_improve" => {
                let prompt = match id {
                    "editor.ai_explain" => "ai-explain-prompt",
                    "editor.ai_review" => "ai-review-prompt",
                    _ => "ai-improve-prompt",
                };
                let mut ed = ai_panel::EditorCtx {
                    read_only: self.view.read_only,
                    file_label: self.ai_file_label(),
                    language: self.view.language.name,
                    buf: &mut self.buf,
                    cursors: &mut self.cursors,
                };
                self.ai.quick(cx.services, &mut ed, prompt);
            }
            "editor.terminal_here" => {
                let dir = self
                    .state
                    .path
                    .as_ref()
                    .and_then(|p| p.parent().map(Path::to_path_buf))
                    .or(self.state.root.clone());
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
        self.state
            .path
            .as_ref()
            .map(|p| p.to_string_lossy().to_string())
    }

    fn quick_open_items(&self) -> Vec<QuickOpenItem> {
        let Ok(index) = self.index.lock() else {
            return Vec::new();
        };
        index
            .iter()
            .map(|p| QuickOpenItem {
                id: p.to_string_lossy().to_string(),
                title: p
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default(),
                subtitle: p.parent().map(|d| self.relative(d)).unwrap_or_default(),
            })
            .collect()
    }

    fn save_state(&self) -> serde_json::Value {
        let mut st = self.state.clone();
        st.cursor = self.cursors.primary().head;
        st.expanded = self.expanded_dirs();
        serde_json::to_value(&st).unwrap_or_default()
    }

    fn open(&mut self, item: OpenRequest, cx: &mut ToolContext) -> bool {
        match item {
            OpenRequest::File { uri, line } => {
                let path = PathBuf::from(uri.strip_prefix("file://").unwrap_or(&uri));
                self.open_path(path.clone(), line, cx);
                if let Some(l) = line
                    && self.state.path.as_deref() == Some(path.as_path())
                {
                    let pos = self.buf.line_col_to_char((l as usize).saturating_sub(1), 0);
                    self.cursors.set_single(Cursor::at(pos));
                    self.view.scroll_to_cursor();
                }
                true
            }
            OpenRequest::Text { mime, body } => {
                if self.state.path.is_some() || self.buf.dirty() {
                    cx.open("editor", Some(OpenRequest::Text { mime, body }));
                    return true;
                }
                let hint = match mime.as_str() {
                    m if m.contains("json") => Some("x.json"),
                    m if m.contains("yaml") => Some("x.yaml"),
                    m if m.contains("xml") => Some("x.xml"),
                    m if m.contains("html") => Some("x.html"),
                    m if m.contains("sql") => Some("x.sql"),
                    m if m.contains("markdown") => Some("x.md"),
                    _ => None,
                };
                self.set_text(&body, hint);
                true
            }
            _ => false,
        }
    }

    fn on_event(&mut self, ev: &Event, cx: &mut ToolContext) {
        if let Event::SettingsChanged = ev {
            let st = cx.services.settings.read();
            self.view.font_size = st.editor.font_size;
            self.view.right_click_copy_paste = st.editor.right_click_copy_paste;
            self.auto_reload = st.editor.auto_reload;
            self.trim_trailing = st.editor.trim_trailing_whitespace;
            self.final_newline = st.editor.insert_final_newline;
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
    fn sizes() {
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(3 * 1024 * 1024), "3.0 MB");
    }
}
