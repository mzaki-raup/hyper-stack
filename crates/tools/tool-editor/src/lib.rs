//! Code editor tool (docs/tools/editor.md): one window tab per project
//! (folder), titled with the folder's name, with the project tree in the
//! sidebar and the project's open files as a multi-row strip of file tabs
//! above the text. Files outside every project share an "Other Files" tab. Files open
//! with encodings and line endings, the watcher reports changes on disk,
//! Markdown files get a preview, and every project feeds the quick-open
//! palette.

use hs_ui::tip::TipExt;
use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
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
mod debug;
mod git_ops;
mod git_view;
mod langver;
mod lsp;
pub mod project;
mod terminal;
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
    /// The sidebar shows Source Control instead of the project tree.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    source_control: bool,
    /// Diffs side by side rather than inline.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    side_by_side: bool,
    /// Breakpoints per file (1-based lines).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    breakpoints: Vec<(PathBuf, Vec<u32>)>,
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
    /// Stable id for the file tab strip.
    uid: u64,
    git: git_view::DocGit,
}

static NEXT_DOC: AtomicU64 = AtomicU64::new(1);

impl Doc {
    fn new(s: &Services) -> Self {
        let st = s.settings.read();
        let dark = Theme::from_settings(&st.appearance).dark;
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
            uid: NEXT_DOC.fetch_add(1, Ordering::Relaxed),
            git: git_view::DocGit::default(),
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
        if services.settings.read().editor.format_minified_on_open {
            self.lay_out_minified();
        }
        Ok(())
    }

    /// A JSON file that arrives on one line (a source map, a minified
    /// payload) cannot be read: lay it out as one undoable edit. The file
    /// on disk is untouched until the user saves, and the tab shows the
    /// unsaved dot to say so. Files that are already laid out, and every
    /// other language, are left exactly as they are.
    fn lay_out_minified(&mut self) {
        if self.view.read_only || self.view.language.syntax_token != "json" {
            return;
        }
        let lines = self.buf.len_lines();
        let longest = self.buf.max_line_chars();
        if lines > 2 && longest <= 400 {
            return;
        }
        let text = self.buf.text();
        let Some(pretty) = hs_core::fmt::pretty_json(&text) else {
            return;
        };
        self.view
            .replace_all_text(&mut self.buf, &mut self.cursors, &pretty);
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

    fn reopen_with_encoding(&mut self, encoding: Encoding) -> Result<(), String> {
        let Some(path) = self.path.clone() else {
            return Err("This document has no file on disk to reopen.".into());
        };
        let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
        let pos = self.cursors.primary().head;
        self.buf = Buffer::from_bytes_with_encoding(&bytes, encoding);
        self.cursors
            .set_single(Cursor::at(pos.min(self.buf.len_chars())));
        self.buf.mark_saved();
        self.saved_mtime = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
        self.disk = DiskState::Clean;
        self.preview_cache.0 = u64::MAX;
        self.preview = self.preview && self.view.language.is_markdown;
        self.view.scroll_to_cursor();
        Ok(())
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

    fn status_row(
        &mut self,
        ui: &mut egui::Ui,
        s: &Services,
        lsp: Option<&lsp::LspStatus>,
        counts: (usize, usize),
    ) -> StatusRow {
        let mut reopen_error = None;
        let mut problems_clicked = false;
        let mut install_clicked = false;
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
            if counts.0 + counts.1 > 0 || lsp.is_some() {
                let text = format!(
                    "{} {}  {} {}",
                    ph::X_CIRCLE,
                    counts.0,
                    ph::WARNING,
                    counts.1
                );
                if ui
                    .add(egui::Button::new(small(text)).frame(false))
                    .tip(s.t("editor-problems-tip"))
                    .clicked()
                {
                    problems_clicked = true;
                }
            }
            if let Some(st) = lsp {
                // A progress message can carry a long path: keep it short.
                ui.scope(|ui| {
                    ui.set_max_width(260.0);
                    ui.add(
                        egui::Label::new(small(st.text.clone()).weak())
                            .truncate()
                            .selectable(false),
                    )
                    .tip(format!(
                        "{}

{}",
                        st.text, st.tip
                    ));
                });
                if let Some(install) = &st.install {
                    let label = small(format!(
                        "{} {}",
                        ph::DOWNLOAD_SIMPLE,
                        s.t("editor-lsp-install")
                    ));
                    let tip = if install.command.is_empty() {
                        s.i18n
                            .t_args("editor-lsp-install-page-tip", &[("url", &install.url)])
                    } else {
                        s.i18n.t_args(
                            "editor-lsp-install-run-tip",
                            &[("command", &install.command)],
                        )
                    };
                    if ui
                        .add(egui::Button::new(label.color(theme_accent(s))).frame(false))
                        .tip(tip)
                        .clicked()
                    {
                        install_clicked = true;
                    }
                }
                if st.busy {
                    ui.ctx().request_repaint_after(Duration::from_millis(400));
                }
            }
            if let Some(r) = &self.read_only_reason {
                ui.colored_label(egui::Color32::from_rgb(0xD9, 0xB0, 0x2A), small(r.clone()));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some(p) = &self.path {
                    // The path takes what is left after the other segments.
                    ui.scope(|ui| {
                        ui.set_max_width((ui.available_width() - 320.0).max(80.0));
                        ui.add(
                            egui::Label::new(small(p.to_string_lossy().to_string()).weak())
                                .truncate(),
                        );
                    });
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
                    ui.separator();
                    ui.menu_button(s.t("editor-reopen-with-encoding"), |ui| {
                        let can_reopen = self.path.is_some() && !self.buf.dirty();
                        for enc in Encoding::all() {
                            if ui
                                .add_enabled(
                                    can_reopen,
                                    egui::Button::new(enc.label())
                                        .selected(self.buf.encoding == enc),
                                )
                                .clicked()
                            {
                                if let Err(e) = self.reopen_with_encoding(enc) {
                                    reopen_error = Some(e);
                                }
                                ui.close();
                            }
                        }
                        if !can_reopen {
                            ui.separator();
                            ui.label(
                                egui::RichText::new(s.t("editor-reopen-with-encoding-hint"))
                                    .small()
                                    .weak(),
                            );
                        }
                    });
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
        StatusRow {
            encoding_error: reopen_error,
            problems_clicked,
            install_clicked,
        }
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

/// What the user asked for when the unsaved-files prompt went up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UnsavedFor {
    Run,
    Debug,
}

struct Project {
    /// `None`: the "Other Files" group.
    root: Option<PathBuf>,
    docs: Vec<Doc>,
    active: usize,
    index: Arc<StdMutex<Vec<PathBuf>>>,
    index_building: Arc<AtomicBool>,
    index_dirty: bool,
    index_built_at: Option<Instant>,
    /// What the root folder is (Maven, Cargo, npm, …), re-detected every
    /// few seconds rather than on every toolbar frame: detection stats
    /// the manifests, reads pom.xml/package.json and, for a folder with no
    /// manifest, lists the whole root.
    detected: Option<(Instant, Option<project::Project>)>,
}

const PROJECT_DETECT_TTL: Duration = Duration::from_secs(3);

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
            detected: None,
        };
        p.rebuild_index();
        p
    }

    /// The detected project kind, from the cache when it is fresh.
    fn detected(&mut self) -> Option<project::Project> {
        let now = Instant::now();
        if let Some((at, p)) = &self.detected
            && now.duration_since(*at) < PROJECT_DETECT_TTL
        {
            return p.clone();
        }
        let p = self.root.as_deref().and_then(project::detect);
        self.detected = Some((now, p.clone()));
        p
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
    /// The files left (`true`) or right of one.
    Side(usize, usize, bool),
    Project(usize),
}

/// What the status bar's buttons were asked for this frame.
#[derive(Default)]
struct StatusRow {
    /// Reopening with another encoding failed.
    encoding_error: Option<String>,
    problems_clicked: bool,
    /// The language server is missing and the user asked to install it.
    install_clicked: bool,
}

/// The accent of the current scheme (the status bar builds its own
/// labels and has no `Theme` to hand).
fn theme_accent(s: &Services) -> egui::Color32 {
    Theme::from_settings(&s.settings.read().appearance).accent
}

/// The outcome of a folder ZIP written on a worker: the archive path and
/// its file count, or the error; `None` while it is still running.
type ZipSlot = std::sync::Arc<parking_lot::Mutex<Option<Result<(PathBuf, usize), String>>>>;

pub struct EditorTool {
    projects: Vec<Project>,
    /// Index into `projects`; that project always has an open file.
    active: usize,
    auto_reload: bool,
    trim_trailing: bool,
    final_newline: bool,

    /// One root node per project with a folder, in project order.
    nodes: Vec<TreeNode>,
    tree: TreeState,
    watch: Option<Watch>,

    /// A folder download in progress: the worker's outcome, once.
    zip_result: Option<ZipSlot>,
    menu: Option<(PathBuf, egui::Pos2)>,
    prompt: Option<Prompt>,
    confirm_delete: Option<PathBuf>,
    confirm_close: Option<CloseTarget>,
    /// Run or Debug asked for while files are unsaved: which one, so the
    /// answer can go on to the right action.
    confirm_unsaved: Option<UnsavedFor>,
    id: egui::Id,
    tab_id: u64,
    ai: ai_panel::AiPanel,
    /// The editor's own terminal, at the foot of the pane.
    terminal: terminal::TerminalPanel,
    /// Projects of a tab saved by an older version that held several:
    /// they move to tabs of their own on the first frame.
    split_off: Vec<ProjectState>,
    git: git_view::Git,
    /// Language servers for this tab's project (completion, diagnostics,
    /// navigation).
    lsp: lsp::Lsp,
    /// The debug session, breakpoints and the debug panel.
    debug: debug::Debugger,
    /// Whether the work pane's left column (Project / Source Control) is
    /// showing. A toolbar toggle, because an editor on a narrow screen
    /// wants all the width it can get.
    show_column: bool,
    /// This tab is the activity's dashboard. The shell says so once, when
    /// the tab is made; it cannot arrive through `ToolContext`, because a
    /// tab nobody draws would never hear it.
    is_home: bool,
    /// The saved project's own name and language for the folder this tab
    /// has open, refreshed each frame from the store. Cached because
    /// `title()` and `tab_icon()` are asked every frame with no services
    /// to look anything up in.
    proj_name: String,
    proj_lang: String,
    /// The workspace's name, for the dashboard tab's label.
    ws_name: String,
}

impl EditorTool {
    fn new(services: &Services, state: State) -> Self {
        let st = services.settings.read();
        let auto_reload = st.editor.auto_reload;
        let trim_trailing = st.editor.trim_trailing_whitespace;
        let final_newline = st.editor.insert_final_newline;
        drop(st);
        let id = egui::Id::new(("hs-editor-tool", hs_core::new_id()));
        let lsp = lsp::Lsp::new(services, id);
        let mut debug = debug::Debugger::new(services, id);
        debug.restore_breakpoints(&state.breakpoints);
        let mut t = Self {
            projects: Vec::new(),
            active: 0,
            auto_reload,
            trim_trailing,
            final_newline,
            nodes: Vec::new(),
            tree: TreeState::default(),
            watch: None,
            menu: None,
            zip_result: None,
            prompt: None,
            confirm_delete: None,
            confirm_close: None,
            confirm_unsaved: None,
            show_column: true,
            is_home: false,
            proj_name: String::new(),
            proj_lang: String::new(),
            ws_name: String::new(),
            id,
            tab_id: 0,
            ai: ai_panel::AiPanel::default(),
            terminal: terminal::TerminalPanel::default(),
            split_off: Vec::new(),
            git: git_view::Git::new(state.source_control, state.side_by_side),
            lsp,
            debug,
        };
        t.start_watch(services);
        // One project per tab: keep the active one, hand the rest to tabs
        // of their own (layouts saved when a tab held several).
        let (projects, active) = state.into_projects();
        let mut mine = None;
        for (i, ps) in projects.into_iter().enumerate() {
            if ps.root.is_none() && ps.files.is_empty() {
                continue;
            }
            if i == active && mine.is_none() {
                mine = Some(ps);
            } else {
                t.split_off.push(ps);
            }
        }
        if mine.is_none() && !t.split_off.is_empty() {
            mine = Some(t.split_off.remove(0));
        }
        let mut restored_active = None;
        for (i, ps) in mine.into_iter().enumerate() {
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
            if i == 0 {
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
        let ui_theme = Theme::from_settings(&s.settings.read().appearance);
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
                .position(|d| {
                    d.path
                        .as_deref()
                        .is_some_and(|opened| same_path(opened, path))
                })
                .map(|di| (pi, di))
        })
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
    }

    fn activate(&mut self, pi: usize, di: usize) {
        if pi < self.projects.len() && di < self.projects[pi].docs.len() {
            self.active = pi;
            self.projects[pi].active = di;
            self.git.close_main();
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

    /// The project folder of this tab (`None`: the Other Files tab, or a
    /// tab with nothing in it yet).
    fn root(&self) -> Option<&Path> {
        self.projects.iter().find_map(|p| p.root.as_deref())
    }

    /// No project and nothing but an empty untitled file: the next folder
    /// or file opened becomes this tab's.
    fn is_empty_tab(&self) -> bool {
        self.root().is_none() && self.total_docs() <= 1 && self.doc().is_placeholder()
    }

    /// The user opened `path` from this tab (tree, Open File/Folder): open
    /// it here when it belongs here, otherwise let the shell route it to
    /// its project's tab, the Other Files tab or a new tab.
    /// A Ctrl+clicked link in the editor's terminal: a file opens here, in
    /// this project; a URL opens the way the dock terminal opens one -
    /// the system browser, or HyperStack's own with Shift held.
    fn open_terminal_link(
        &mut self,
        link: hs_term::links::LinkTarget,
        ctx: &egui::Context,
        cx: &mut ToolContext,
    ) {
        match link {
            hs_term::links::LinkTarget::Url(url) => {
                if ctx.input(|i| i.modifiers.shift) {
                    cx.open("browser", Some(OpenRequest::Url { url }));
                } else {
                    ctx.open_url(egui::OpenUrl::new_tab(url));
                }
            }
            hs_term::links::LinkTarget::Path { path, line, .. } => {
                let p = PathBuf::from(&path);
                let p = match (p.is_absolute(), self.projects[self.active].root.clone()) {
                    (true, _) => p,
                    (false, Some(root)) => root.join(p),
                    (false, None) => cx.services.workspace.read().root().join(p),
                };
                if p.is_file() {
                    self.open_path(p, line, cx);
                } else {
                    let msg = cx
                        .services
                        .i18n
                        .t_args("terminal-link-missing", &[("path", &path)]);
                    cx.toast(ToastLevel::Warning, msg);
                }
            }
        }
    }

    fn open_path(&mut self, path: PathBuf, line: Option<u32>, cx: &mut ToolContext) {
        let path = normalize_path(std::fs::canonicalize(&path).unwrap_or(path));
        // Pictures open in the graphics tool, not as bytes in a buffer.
        if path.is_file() && hs_core::sniff::is_image_path(&path) {
            cx.open(
                "graphics",
                Some(OpenRequest::File {
                    uri: path.to_string_lossy().to_string(),
                    line: None,
                }),
            );
            self.select_in_tree();
            return;
        }
        let here = if path.is_dir() {
            self.is_empty_tab() || self.root().is_some_and(|r| same_path(r, &path))
        } else {
            self.find_doc(&path).is_some()
                || self.root().is_some_and(|r| path.starts_with(r))
                || self.is_empty_tab()
        };
        if here {
            self.take_path(path, line, cx);
        } else {
            cx.open(
                "editor",
                Some(OpenRequest::File {
                    uri: path.to_string_lossy().to_string(),
                    line,
                }),
            );
            // The click selected it in this tab's tree; it opened elsewhere.
            self.select_in_tree();
        }
    }

    /// Open `path` in this tab (the shell routed it here, or it belongs here).
    fn take_path(&mut self, path: PathBuf, line: Option<u32>, cx: &mut ToolContext) {
        let path = normalize_path(std::fs::canonicalize(&path).unwrap_or(path));
        if path.is_dir() {
            if self.root().is_some_and(|r| same_path(r, &path)) {
                return;
            }
            if !self.is_empty_tab() {
                // A second project never shares a tab.
                cx.open_new_tab(
                    "editor",
                    Some(OpenRequest::File {
                        uri: path.to_string_lossy().to_string(),
                        line: None,
                    }),
                );
                return;
            }
            self.add_project(path);
            self.ensure_doc(cx.services);
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
            CloseTarget::Side(pi, di, left) => self.projects.get(pi).is_some_and(|p| {
                p.docs
                    .iter()
                    .enumerate()
                    .any(|(i, d)| (if left { i < di } else { i > di }) && d.buf.dirty())
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
            CloseTarget::Side(pi, di, left) => {
                let n = self.projects.get(pi).map_or(0, |p| p.docs.len());
                let range: Vec<usize> = if left {
                    (0..di).rev().collect()
                } else {
                    (di + 1..n).rev().collect()
                };
                for i in range {
                    self.close_file(pi, i, s);
                }
            }
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

    /// Every document with unsaved changes, in every project. Returns
    /// false if any of them could not be written, so the caller can stop
    /// rather than run code that is not on disk. A document that has never
    /// had a path would open a file dialog, so it is left alone: Run and
    /// Debug work on files that exist.
    fn save_all_dirty(&mut self, cx: &mut ToolContext) -> bool {
        let (was_project, was_doc) = (self.active, self.projects[self.active].active);
        let mut ok = true;
        for pi in 0..self.projects.len() {
            for di in 0..self.projects[pi].docs.len() {
                let d = &self.projects[pi].docs[di];
                if !d.buf.dirty() {
                    continue;
                }
                // A document with no path is saved too: `save` falls back
                // to Save As and asks where it should go. It used to be
                // skipped, so a brand-new file was never written and never
                // even mentioned - the one case where running what is on
                // screen rather than on disk is certain. Owner's
                // requirement, 2026-10-17.
                self.active = pi;
                self.projects[pi].active = di;
                ok &= self.save(cx);
            }
        }
        self.active = was_project;
        self.projects[was_project].active = was_doc;
        ok
    }

    /// The names of the documents that would be written, for the prompt.
    fn dirty_names(&self, s: &Services) -> Vec<String> {
        self.projects
            .iter()
            .flat_map(|p| p.docs.iter())
            .filter(|d| d.buf.dirty())
            .map(|d| {
                let n = d.file_name();
                if n.is_empty() {
                    s.t("editor-untitled")
                } else {
                    n
                }
            })
            .collect()
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
        let saved = match hs_core::storage::write_atomic(&path, &bytes) {
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
        };
        if saved {
            let pa = self.active;
            let p = &self.projects[pa];
            let d = &p.docs[p.active];
            self.lsp.saved(&path, &d.buf);
        }
        saved
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
        let roots: Vec<PathBuf> = self
            .projects
            .iter()
            .filter_map(|p| p.root.clone())
            .collect();
        self.git.sync(&roots);
        let mut want: Vec<(PathBuf, bool)> = roots.into_iter().map(|r| (r, true)).collect();
        for g in self.git.extra_watch_dirs() {
            want.push((g, false));
        }
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
                self.git.on_fs_event(p);
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

    fn apply_git(
        &mut self,
        actions: Vec<git_view::GitAction>,
        ui: &egui::Ui,
        cx: &mut ToolContext,
    ) {
        for a in actions {
            match a {
                git_view::GitAction::OpenFile(p) => {
                    self.git.close_main();
                    self.open_path(p, None, cx);
                }
                git_view::GitAction::CopyPath(p) => ui.ctx().copy_text(p),
                git_view::GitAction::Toast(level, text) => cx.toast(level, text),
            }
        }
    }

    fn relative(&self, path: &Path) -> String {
        match self.project_for(path) {
            Some(pi) => self.projects[pi].relative(path),
            None => path.to_string_lossy().to_string(),
        }
    }

    // ---- ui pieces -----------------------------------------------------------

    /// The open files of this tab as a multi-row strip of file tabs.
    fn file_tabs(&mut self, ui: &mut egui::Ui, s: &Services, theme: &Theme) {
        struct FileTab {
            id: u64,
            title: String,
            dirty: bool,
            tooltip: String,
        }
        impl hs_ui::tabs::TabItem for FileTab {
            fn id(&self) -> u64 {
                self.id
            }
            fn title(&self) -> String {
                self.title.clone()
            }
            fn icon(&self) -> &str {
                ph::FILE_TEXT
            }
            fn pinned(&self) -> bool {
                false
            }
            fn dirty(&self) -> bool {
                self.dirty
            }
            fn tooltip(&self) -> String {
                self.tooltip.clone()
            }
        }
        // (project, doc) per strip position.
        let order: Vec<(usize, usize)> = self
            .projects
            .iter()
            .enumerate()
            .flat_map(|(pi, p)| (0..p.docs.len()).map(move |di| (pi, di)))
            .collect();
        let tabs: Vec<FileTab> = order
            .iter()
            .map(|&(pi, di)| {
                let d = &self.projects[pi].docs[di];
                let title = if d.path.is_some() {
                    d.file_name()
                } else {
                    s.t("editor-untitled")
                };
                FileTab {
                    id: d.uid,
                    tooltip: d
                        .path
                        .as_ref()
                        .map(|p| p.display().to_string())
                        .unwrap_or_else(|| title.clone()),
                    title,
                    dirty: d.buf.dirty(),
                }
            })
            .collect();
        let pa = self.active;
        let active = self.projects[pa]
            .docs
            .get(self.projects[pa].active)
            .map(|d| d.uid);
        let labels = hs_ui::tabs::TabStripLabels {
            new_tab: s.t("editor-new-file"),
            close: s.t("editor-close-file"),
            close_others: s.t("editor-close-others"),
            close_left: s.t("tab-close-left"),
            close_right: s.t("tab-close-right"),
            pin: String::new(),
            unpin: String::new(),
            duplicate: String::new(),
            duplicate_blocked: String::new(),
            copy_path: s.t("editor-copy-path"),
        };
        let min_width = s.settings.read().appearance.min_file_tab_width;
        let r = ui
            .push_id(self.id.with("file-tabs"), |ui| {
                hs_ui::tabs::TabStrip {
                    theme,
                    min_width,
                    gap: 2.0,
                    active,
                    labels: &labels,
                    compact: true,
                    show_new_tab: true,
                }
                .show(ui, &tabs)
            })
            .inner;
        ui.add_space(2.0);
        let at = |id: u64| tabs.iter().position(|t| t.id == id).map(|i| order[i]);
        if let Some((pi, di)) = r.activated.and_then(at) {
            self.activate(pi, di);
        }
        if let Some((pi, di)) = r.close.and_then(at) {
            self.request_close(CloseTarget::File(pi, di), s);
        }
        if r.new_tab {
            self.new_file(s);
        }
        if let Some((from, to)) = r.reorder
            && let (Some(&(pf, df)), Some(&(pt, dt))) = (order.get(from), order.get(to))
            && pf == pt
        {
            let p = &mut self.projects[pf];
            let d = p.docs.remove(df);
            p.docs.insert(dt, d);
            p.active = dt;
            self.active = pf;
        }
        if let Some((id, action)) = r.menu
            && let Some((pi, di)) = at(id)
        {
            use hs_ui::tabs::TabMenuAction as M;
            match action {
                M::Close => self.request_close(CloseTarget::File(pi, di), s),
                M::CloseOthers => self.request_close(CloseTarget::Others(pi, di), s),
                M::CloseLeft => self.request_close(CloseTarget::Side(pi, di, true), s),
                M::CloseRight => self.request_close(CloseTarget::Side(pi, di, false), s),
                M::CopyPath => {
                    if let Some(p) = &self.projects[pi].docs[di].path {
                        ui.ctx().copy_text(p.to_string_lossy().to_string());
                    }
                }
                _ => {}
            }
        }
    }

    /// The full path of the active file, under the file tabs and separated
    /// from the code by a rule. Click copies it; untitled buffers say so.
    fn path_row(&self, ui: &mut egui::Ui, s: &Services, theme: &Theme) {
        let d = self.doc();
        let text = d
            .path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| s.t("editor-untitled"));
        egui::Frame::new()
            .fill(theme.panel_bg)
            .inner_margin(egui::Margin {
                left: 10,
                right: 8,
                top: 3,
                bottom: 3,
            })
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(ph::FILE_TEXT).color(theme.text_muted));
                    let r = ui.add(
                        egui::Label::new(
                            egui::RichText::new(&text).small().color(theme.text_muted),
                        )
                        .truncate()
                        .selectable(false)
                        .sense(egui::Sense::click()),
                    );
                    if r.tip(s.t("editor-path-row-tip")).clicked() {
                        ui.ctx().copy_text(text.clone());
                    }
                });
            });
        let rect = ui.available_rect_before_wrap();
        ui.painter().hline(
            rect.x_range(),
            rect.top(),
            egui::Stroke::new(1.0, theme.border),
        );
        ui.add_space(1.0);
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
            CloseTarget::Side(pi, di, left) => self
                .projects
                .get(pi)
                .map(|p| {
                    p.docs
                        .iter()
                        .enumerate()
                        .filter(|(i, d)| (if left { *i < di } else { *i > di }) && d.buf.dirty())
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

    /// Run or Debug was asked for with unsaved files: Save writes them all
    /// and carries on, Ignore carries on without, Cancel does neither.
    fn unsaved_before_running(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        let Some(what) = self.confirm_unsaved else {
            return;
        };
        let s = cx.services;
        let (theme, _) = Self::theme(s);
        let names = self.dirty_names(s);
        if names.is_empty() {
            self.confirm_unsaved = None;
            return;
        }
        let action = match what {
            UnsavedFor::Run => s.t("editor-run"),
            UnsavedFor::Debug => s.t("debug-button"),
        };
        let decision = hs_ui::dialog::three_way(
            ui.ctx(),
            &theme,
            "editor-run-unsaved",
            &s.t("editor-run-unsaved-title"),
            &s.i18n.t_args(
                "editor-run-unsaved-body",
                &[("names", &names.join(", ")), ("action", &action)],
            ),
            &s.t("editor-run-unsaved-save"),
            &s.t("editor-run-unsaved-ignore"),
            &s.t("cancel"),
        );
        match decision {
            hs_ui::dialog::ThreeWay::Primary => {
                self.confirm_unsaved = None;
                if self.save_all_dirty(cx) {
                    self.after_unsaved(what, cx);
                }
            }
            hs_ui::dialog::ThreeWay::Secondary => {
                self.confirm_unsaved = None;
                self.after_unsaved(what, cx);
            }
            hs_ui::dialog::ThreeWay::Cancel => self.confirm_unsaved = None,
            hs_ui::dialog::ThreeWay::Open => {}
        }
    }

    fn after_unsaved(&mut self, what: UnsavedFor, cx: &mut ToolContext) {
        match what {
            UnsavedFor::Run => self.run_now(cx),
            UnsavedFor::Debug => self.debug_now(cx),
        }
    }

    /// Zip a folder to a place the user picks. The archive is written on a
    /// worker; the toast arrives through `zip_result` when it is done.
    fn download_zip(&mut self, dir: &Path, cx: &mut ToolContext) {
        let name = dir
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "folder".into());
        let Some(target) = rfd::FileDialog::new()
            .set_file_name(format!("{name}.zip"))
            .add_filter("ZIP", &["zip"])
            .save_file()
        else {
            return;
        };
        let slot = std::sync::Arc::new(parking_lot::Mutex::new(None));
        self.zip_result = Some(slot.clone());
        let src = dir.to_path_buf();
        let waker = cx.services.runtime.waker();
        cx.toast(
            ToastLevel::Info,
            cx.services
                .i18n
                .t_args("editor-download-zipping", &[("name", &name)]),
        );
        std::thread::spawn(move || {
            let result = zip_dir(&src, &target).map(|n| (target, n));
            *slot.lock() = Some(result);
            waker.request_repaint();
        });
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
                        // The editor's own terminal, in that folder: a
                        // shell asked for from the project tree belongs
                        // beside the tree. Owner's report, 2026-10-18.
                        self.terminal.show_in(s, &dir);
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
                    if is_dir {
                        if item(ui, s.t("editor-download-zip")) {
                            self.download_zip(&path, cx);
                            close = true;
                        }
                    } else if item(ui, s.t("editor-download")) {
                        match hs_core::download::save_copy(&path) {
                            Ok(Some(to)) => cx.toast(
                                ToastLevel::Success,
                                s.i18n.t_args(
                                    "download-saved",
                                    &[("path", &to.display().to_string())],
                                ),
                            ),
                            Ok(None) => {}
                            Err(e) => cx.toast(ToastLevel::Error, e),
                        }
                        close = true;
                    }
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

impl EditorTool {
    /// What Run would do for the active file: the project's own runner
    /// when the root has one, else the interpreter for the file type.
    fn run_plan(&mut self, s: &Services) -> Option<(PathBuf, String)> {
        let project = self.projects[self.active].detected();
        let d = self.doc();
        let path = d.path.as_ref()?.clone();
        let st = s.settings.read();
        run_plan_with(
            &path,
            self.projects[self.active].root.as_deref(),
            project.as_ref(),
            &st.editor,
        )
    }

    /// Ctrl+F5: continue a stopped session, start the last configuration
    /// again, or open the picker.
    fn debug_start(&mut self, cx: &mut ToolContext) {
        // Debugging a file that is not on disk steps through stale code, so
        // ask first; the answer comes back through unsaved_before_running.
        if self
            .debug
            .state()
            .is_none_or(|st| st == debug::State::Ended)
            && !self.dirty_names(cx.services).is_empty()
        {
            self.confirm_unsaved = Some(UnsavedFor::Debug);
            return;
        }
        self.debug_now(cx);
    }

    fn debug_now(&mut self, cx: &mut ToolContext) {
        match self.debug.state() {
            Some(debug::State::Stopped) => self.debug.continue_(),
            Some(st) if st != debug::State::Ended => {}
            _ => {
                let root = self.projects[self.active].root.clone();
                let file = self.doc().path.clone();
                self.debug.refresh_configs(root.as_deref(), file.as_deref());
                if !self.debug.start_last(cx.services) {
                    self.debug.picker_open = true;
                }
            }
        }
    }

    /// Run the active file or project in a new terminal tab (F5), or
    /// continue a stopped debug session.
    fn run_current(&mut self, cx: &mut ToolContext) {
        if self.debug.state() == Some(debug::State::Stopped) {
            self.debug.continue_();
            return;
        }
        // Running what is on screen rather than what is on disk is the
        // classic wasted debugging hour: ask before it happens.
        if !self.dirty_names(cx.services).is_empty() {
            self.confirm_unsaved = Some(UnsavedFor::Run);
            return;
        }
        self.run_now(cx);
    }

    fn run_now(&mut self, cx: &mut ToolContext) {
        let Some((cwd, cmd)) = self.run_plan(cx.services) else {
            cx.toast(ToastLevel::Info, cx.services.t("editor-run-none"));
            return;
        };
        // In the editor's own terminal, not a tab of the Terminal
        // activity: running a project is editing work, and changing
        // activity to watch it is what interrupts it.
        // Owner's report, 2026-10-18.
        let web = self.project().is_some_and(|p| p.web);
        self.terminal.run(cx.services, Some(&cwd), &cmd, web);
    }

    /// The project this tab's folder holds (Maven, Gradle, Cargo, npm, …).
    fn project(&mut self) -> Option<project::Project> {
        self.projects[self.active].detected()
    }

    /// Build the project in the editor's terminal.
    fn build_now(&mut self, cx: &mut ToolContext) {
        let (Some(root), Some(p)) = (self.projects[self.active].root.clone(), self.project())
        else {
            cx.toast(ToastLevel::Info, cx.services.t("editor-build-none"));
            return;
        };
        let cmd = {
            let st = cx.services.settings.read();
            project::build_command(&p, &st.editor)
        };
        self.terminal.run(cx.services, Some(&root), &cmd, false);
    }
}

/// The command that runs `path`: a project runner found at `root` (Cargo,
/// npm, Make, Gradle, Maven, Go) wins over the file's interpreter.
pub fn run_plan(
    path: &Path,
    root: Option<&Path>,
    settings: &hs_core::settings::Editor,
) -> Option<(PathBuf, String)> {
    let project = root.and_then(project::detect);
    run_plan_with(path, root, project.as_ref(), settings)
}

/// `run_plan` with the project already detected (the editor caches it).
pub fn run_plan_with(
    path: &Path,
    root: Option<&Path>,
    project: Option<&project::Project>,
    settings: &hs_core::settings::Editor,
) -> Option<(PathBuf, String)> {
    let file = path.file_name()?.to_string_lossy().to_string();
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let dir = path.parent().map(Path::to_path_buf)?;
    let q = |p: &Path| {
        let t = p.to_string_lossy().to_string();
        if t.contains(' ') {
            format!("\"{t}\"")
        } else {
            t
        }
    };
    // What the folder itself is (Maven, Gradle, Cargo, npm, …) decides
    // how to run its own code: that is where Spring Boot's goal, a
    // project's wrapper and the configured Maven/Gradle paths come from.
    // Owner's requirement, 2026-10-17.
    if let Some(root) = root
        && let Some(p) = project
        && (project::owns_file(p.kind, &ext) || project::is_manifest(p.kind, &file))
    {
        return Some((root.to_path_buf(), project::run_command(p, settings)));
    }
    if let Some(root) = root {
        let has = |n: &str| root.join(n).is_file();
        let project = match ext.as_str() {
            "rs" if has("Cargo.toml") => Some("cargo run".to_string()),
            "go" if has("go.mod") => Some("go run .".to_string()),
            "java" | "kt" if has("gradlew") || has("build.gradle") || has("build.gradle.kts") => {
                Some(
                    if cfg!(windows) {
                        "gradlew.bat run"
                    } else {
                        "./gradlew run"
                    }
                    .to_string(),
                )
            }
            "java" if has("pom.xml") => Some("mvn -q compile exec:java".to_string()),
            "js" | "ts" | "jsx" | "tsx" | "mjs" if has("package.json") => {
                Some("npm start".to_string())
            }
            "cs" if std::fs::read_dir(root).ok().is_some_and(|rd| {
                rd.flatten()
                    .any(|e| e.path().extension().is_some_and(|x| x == "csproj"))
            }) =>
            {
                Some("dotnet run".to_string())
            }
            _ => None,
        };
        if let Some(cmd) = project {
            return Some((root.to_path_buf(), cmd));
        }
        if file.eq_ignore_ascii_case("makefile") {
            return Some((dir, "make".into()));
        }
        if file == "package.json" {
            return Some((dir, "npm start".into()));
        }
        if file == "Cargo.toml" {
            return Some((dir, "cargo run".into()));
        }
    }
    let cmd = match ext.as_str() {
        "py" => format!("python {}", q(path)),
        "js" | "mjs" | "cjs" => format!("node {}", q(path)),
        "ts" => format!("npx tsx {}", q(path)),
        "sh" | "bash" => format!("bash {}", q(path)),
        "ps1" => format!("pwsh -File {}", q(path)),
        "bat" | "cmd" => q(path),
        "rb" => format!("ruby {}", q(path)),
        "php" => format!("php {}", q(path)),
        "go" => format!("go run {}", q(path)),
        "rs" => format!(
            "rustc {} -o {} && {}",
            q(path),
            q(&dir.join(format!("{}.exe", path.file_stem()?.to_string_lossy()))),
            q(&dir.join(format!("{}.exe", path.file_stem()?.to_string_lossy())))
        ),
        "java" => format!("java {}", q(path)),
        "kts" => format!("kotlinc -script {}", q(path)),
        "lua" => format!("lua {}", q(path)),
        "pl" => format!("perl {}", q(path)),
        "r" => format!("Rscript {}", q(path)),
        "sql" => return None,
        _ => return None,
    };
    Some((dir, cmd))
}

/// The project list, the dashboard and the work pane's left column.
impl EditorTool {
    /// Where the editor's terminal should open.
    ///
    /// The project's own root, every time there is one. A tab showing a
    /// loose file has no project root, and the fallback used to be the
    /// *workspace* root - the app's own data folder, which is nobody's
    /// idea of where to run a build (owner's report, 2026-10-27). The
    /// file's own folder is the honest answer there.
    fn terminal_cwd(&self) -> Option<PathBuf> {
        if let Some(r) = self.projects[self.active].root.clone() {
            return Some(r);
        }
        // A tab may hold several projects; any real root beats none.
        if let Some(r) = self.projects.iter().find_map(|p| p.root.clone()) {
            return Some(r);
        }
        self.doc()
            .path
            .as_ref()
            .and_then(|p| p.parent())
            .map(Path::to_path_buf)
    }

    /// Add `root` to the workspace's project list and open it.
    ///
    /// A folder already listed is opened rather than added twice: two
    /// rows for one folder is never what the gesture meant.
    fn add_project_folder(&mut self, root: &Path, cx: &mut ToolContext) {
        let s = cx.services;
        let existing = s.projects.read().by_root(root).map(|p| p.id.clone());
        if existing.is_none() {
            let entry = hs_core::projects::ProjectEntry::for_root(root);
            if let Err(e) = s.projects.write().upsert(entry) {
                cx.toast(ToastLevel::Error, e.to_string());
                return;
            }
        }
        cx.open(
            "editor",
            Some(OpenRequest::File {
                uri: root.to_string_lossy().to_string(),
                line: None,
            }),
        );
    }

    /// Keep the cached project name, language and workspace name true.
    ///
    /// `title()` and `tab_icon()` are asked every frame and handed no
    /// services, so what they show has to be sitting on the tool already.
    /// One lock read a frame, and nothing is cloned unless it changed.
    fn sync_project(&mut self, s: &Services) {
        let ws = s.workspace.read().meta.name.clone();
        if ws != self.ws_name {
            self.ws_name = ws;
        }
        let Some(root) = self.root().map(|r| r.to_path_buf()) else {
            self.proj_name.clear();
            self.proj_lang.clear();
            return;
        };
        let found = s
            .projects
            .read()
            .by_root(&root)
            .map(|p| (p.label(), p.language.clone()));
        match found {
            Some((name, lang)) => {
                if name != self.proj_name {
                    self.proj_name = name;
                }
                if lang != self.proj_lang {
                    self.proj_lang = lang;
                }
            }
            None => {
                // A folder opened without being saved to the list: it is
                // still a project as far as this tab is concerned, so it
                // keeps the folder's name and a detected language rather
                // than reading as nothing at all.
                self.proj_name.clear();
                let lang = hs_core::projects::detect_language(&root);
                if lang != self.proj_lang {
                    self.proj_lang = lang.to_string();
                }
            }
        }
    }

    /// The activity's dashboard: what this workspace has, and a card per
    /// project to open it.
    fn dashboard(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        let s = cx.services;
        let (theme, _) = Self::theme(s);
        let projects: Vec<(String, String, String, &'static str)> = s
            .projects
            .read()
            .all()
            .iter()
            .map(|p| {
                (
                    p.id.clone(),
                    p.label(),
                    p.root.display().to_string(),
                    hs_ui::icons::language_icon(&p.language),
                )
            })
            .collect();
        let open_here = s.open_tabs.projects().len();
        hs_ui::form::page_title(
            ui,
            &theme,
            &s.t("tool-editor"),
            Some(&s.t("editor-dash-sub")),
        );
        ui.horizontal(|ui| {
            hs_ui::dashboard::stat(
                ui,
                &theme,
                &projects.len().to_string(),
                &s.t("editor-dash-projects"),
            );
            hs_ui::dashboard::stat(ui, &theme, &open_here.to_string(), &s.t("dash-open-tabs"));
        });
        ui.add_space(6.0);
        // Adding a folder has to be reachable from the dashboard too: it
        // is the first thing someone sees in an empty workspace, and
        // sending them to the sidebar or to Settings to get started
        // would be the long way round.
        let mut add = false;
        ui.horizontal(|ui| {
            if hs_ui::form::primary_button(
                ui,
                &theme,
                &format!("{} {}", ph::FOLDER_PLUS, s.t("editor-add-project")),
            )
            .clicked()
            {
                add = true;
            }
            if ui
                .button(format!("{} {}", ph::GEAR, s.t("editor-manage-projects")))
                .clicked()
            {
                cx.actions.push(hs_core::tool::Action::RunCommand {
                    id: "settings:editor".into(),
                });
            }
        });
        if add && let Some(root) = rfd::FileDialog::new().pick_folder() {
            self.add_project_folder(&root, cx);
            return;
        }
        ui.add_space(8.0);
        egui::ScrollArea::vertical()
            .id_salt(self.id.with("dashboard"))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if projects.is_empty() {
                    hs_ui::form::note(ui, &theme, ph::LIGHTBULB, &s.t("editor-no-projects"));
                    return;
                }
                let mut open: Option<String> = None;
                for (id, name, path, icon) in &projects {
                    if hs_ui::form::list_row(ui, &theme, false, icon, name, Some(path)).clicked() {
                        open = Some(id.clone());
                    }
                    ui.add_space(2.0);
                }
                if let Some(id) = open {
                    let root = s.projects.read().by_id(&id).map(|p| p.root.clone());
                    if let Some(root) = root {
                        cx.open(
                            "editor",
                            Some(OpenRequest::File {
                                uri: root.to_string_lossy().to_string(),
                                line: None,
                            }),
                        );
                    }
                }
                // The shared Open Here grid, as every other activity's
                // dashboard shows it: the project list above says what
                // could be opened, this says what already is and takes
                // the reader back to it (owner's rule on one UI
                // language, 2026-10-20; asked for here 2026-10-27).
                let tabs = s.open_tabs.of("editor");
                let labels = hs_ui::dashboard::DashboardLabels {
                    open_heading: &s.t("dash-open"),
                    nothing_open: &s.t("dash-nothing-open"),
                    no_preview: &s.t("dash-no-preview"),
                };
                if let Some(id) = hs_ui::dashboard::open_tabs(ui, &theme, &tabs, &labels) {
                    cx.actions
                        .push(hs_core::tool::Action::FocusTab { tab_id: id });
                }
            });
    }

    /// Add `root` as a project (or find it) and make it the one new files go to.
    fn add_project(&mut self, root: PathBuf) -> usize {
        let root = normalize_path(std::fs::canonicalize(&root).unwrap_or(root));
        if let Some(i) = self
            .projects
            .iter()
            .position(|p| p.root.as_deref().is_some_and(|old| same_path(old, &root)))
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
        at
    }

    /// The work pane's left column: Project (the file tree) and Source
    /// Control, the two views of the folder this tab has open.
    ///
    /// This used to be the activity sidebar. The sidebar now lists every
    /// project in the workspace, which is a different question - "what
    /// could I open?" rather than "what is in the thing I have open?" -
    /// and the two want to be visible at the same time (owner's
    /// requirement, 2026-10-26).
    fn workspace_column(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) -> bool {
        let s = cx.services;
        let (theme, _) = Self::theme(s);
        let changes = self.git.change_count();
        let git_label = if changes > 0 {
            format!(
                "{} {} ({changes})",
                ph::GIT_BRANCH,
                s.t("git-source-control")
            )
        } else {
            format!("{} {}", ph::GIT_BRANCH, s.t("git-source-control"))
        };
        let files_label = format!("{} {}", ph::FOLDERS, s.t("editor-project"));
        if let Some(i) = segmented(
            ui,
            &theme,
            &[&files_label, &git_label],
            usize::from(self.git.sidebar),
        ) {
            self.git.sidebar = i == 1;
        }
        ui.add_space(2.0);
        if self.git.sidebar {
            let projects: Vec<(PathBuf, String)> = self
                .projects
                .iter()
                .filter_map(|p| {
                    p.root.clone().map(|r| {
                        let name = r
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_else(|| r.to_string_lossy().to_string());
                        (r, name)
                    })
                })
                .collect();
            let actions = self.git.sidebar_ui(ui, s, &theme, &projects);
            self.apply_git(actions, ui, cx);
            return true;
        }
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
        self.git.decorate(&mut self.nodes, &theme);
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
}

impl Tool for EditorTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &DESCRIPTOR
    }

    fn title(&self) -> Cow<'_, str> {
        if self.is_home {
            return Cow::Owned(self.ws_name.clone());
        }
        // The *project's* name, which the user chose, in preference to
        // the folder's - they are often not the same thing.
        if !self.proj_name.is_empty() {
            return Cow::Owned(self.proj_name.clone());
        }
        if let Some(r) = self.root() {
            return Cow::Owned(
                r.file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| r.to_string_lossy().to_string()),
            );
        }
        let n = self.doc().file_name();
        if n.is_empty() {
            Cow::Borrowed("Untitled")
        } else {
            Cow::Owned(n)
        }
    }

    /// Project name, what language it is and which toolchain is
    /// installed for it.
    fn header(&self) -> Option<hs_core::tool::ToolHeader> {
        if self.is_home {
            return None;
        }
        let name = if self.proj_name.is_empty() {
            self.title().to_string()
        } else {
            self.proj_name.clone()
        };
        let language = langver::display_name(&self.proj_lang);
        // "Rust 1.89.0", or nothing at all while the probe is still out
        // or when the toolchain is not installed - an empty fact is left
        // out of the band rather than shown as "unknown".
        let version = match langver::cached(&self.proj_lang) {
            Some(v) if !language.is_empty() => format!("{language} {v}"),
            _ => String::new(),
        };
        Some(
            hs_core::tool::ToolHeader::new(hs_ui::icons::language_icon(&self.proj_lang), name)
                .fact("Language", language)
                .fact("Version", version)
                // Where the project actually lives, last in the band
                // (owner's requirement, 2026-10-27). A tab with no root
                // - a loose file, or the workspace tab - leaves it out
                // rather than showing an empty fact.
                .fact(
                    "Path",
                    self.root()
                        .map(|r| r.display().to_string())
                        .unwrap_or_default(),
                ),
        )
    }

    /// The dashboard wears the workspace; a project tab wears its
    /// language.
    fn tab_icon(&self) -> Option<&'static str> {
        if self.is_home {
            return Some(hs_ui::icons::WORKSPACE);
        }
        if self.proj_lang.is_empty() {
            return None;
        }
        Some(hs_ui::icons::language_icon(&self.proj_lang))
    }

    /// The dashboard has no file to save, no folder to close and no
    /// terminal to open, so the strip of editor verbs above it acted on
    /// nothing. The shell leaves the bar out entirely rather than
    /// reserving an empty one.
    fn draws_toolbar(&self) -> bool {
        !self.is_home
    }

    /// All three, or the activity opens a working tab with nothing in it
    /// and draws its "this has been deleted" empty state (CLAUDE.md).
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
        self.is_home = home;
        // The workspace's name is read *here*, not in `ui`: a tab nobody
        // has drawn never runs `ui`, so the dashboard tab would sit in
        // the strip with no label until the first time it was opened
        // (CLAUDE.md - this is why the trait hands `set_home_tab` the
        // services at all).
        self.ws_name = services.workspace.read().meta.name.clone();
    }

    fn dirty(&self) -> bool {
        self.projects
            .iter()
            .any(|p| p.docs.iter().any(|d| d.buf.dirty()))
    }

    fn project_roots(&self) -> Vec<PathBuf> {
        // The dashboard holds no project, so it is never "a tab under
        // this root" when one is removed from the list.
        if self.is_home {
            return Vec::new();
        }
        self.projects
            .iter()
            .filter_map(|p| p.root.clone())
            .collect()
    }

    fn toolbar(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        let s = cx.services;
        self.tab_id = cx.tab_id;
        // The column holding the file tree and Source Control: a toggle
        // rather than a fixture, because an editor on a narrow screen
        // wants all the width it can get.
        if ui
            .toggle_value(
                &mut self.show_column,
                format!("{} {}", ph::SIDEBAR_SIMPLE, s.t("editor-column")),
            )
            .tip(s.t("editor-column-tip"))
            .clicked()
        {
            // nothing more: the flag is the state.
        }
        ui.separator();
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
        if ui
            .button(format!(
                "{} {}",
                ph::FLOPPY_DISK_BACK,
                s.t("editor-save-as")
            ))
            .clicked()
        {
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
        if ui
            .button(format!("{} {}", ph::SWAP, s.t("editor-replace")))
            .clicked()
        {
            let d = self.doc_mut();
            let sel = hs_editor::view::selected_text(&d.buf, &d.cursors);
            d.view.open_find(true, Some(sel));
        }
        if ui
            .button(format!("{} {}", ph::LIST_NUMBERS, s.t("editor-goto")))
            .clicked()
        {
            let d = self.doc_mut();
            d.view.goto_open = true;
            d.view.goto_text.clear();
        }
        if let Some((_, cmd)) = self.run_plan(s) {
            ui.separator();
            if ui
                .button(format!("{} {}", ph::PLAY, s.t("editor-run")))
                .tip(s.i18n.t_args("editor-run-tip", &[("command", &cmd)]))
                .clicked()
            {
                self.run_current(cx);
            }
        }
        // Build, for a folder that is a project: the command says which
        // build tool was found. Owner's requirement, 2026-10-17.
        if let Some(p) = self.project() {
            let cmd = {
                let st = s.settings.read();
                project::build_command(&p, &st.editor)
            };
            if ui
                .button(format!("{} {}", ph::HAMMER, s.t("editor-build")))
                .tip(s.i18n.t_args(
                    "editor-build-tip",
                    &[("tool", p.kind.label()), ("command", &cmd)],
                ))
                .clicked()
            {
                self.build_now(cx);
            }
        }
        {
            let (theme, _) = Self::theme(s);
            let root = self.projects[self.active].root.clone();
            let file = self.doc().path.clone();
            ui.separator();
            self.debug
                .toolbar(ui, s, &theme, root.as_deref(), file.as_deref());
            ui.separator();
            let mut open = self.terminal.open;
            if ui
                .toggle_value(
                    &mut open,
                    format!("{} {}", ph::TERMINAL_WINDOW, s.t("editor-terminal")),
                )
                .tip(s.t("editor-terminal-tip"))
                .clicked()
            {
                let cwd = self.terminal_cwd();
                self.terminal.toggle(s, cwd.as_deref());
            }
        }
        if self.doc().view.language.is_markdown {
            ui.separator();
            let d = self.doc_mut();
            ui.toggle_value(
                &mut d.preview,
                format!("{} {}", ph::EYE, s.t("editor-preview")),
            );
        }
        hs_ui::toolbar::hard_right(ui, |ui| {
            // The assistant is the same control in every tool, so it sits in
            // the same place: hard right of the toolbar.
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
        });
    }

    /// The activity sidebar: every project saved in this workspace, in
    /// the order Settings -> Editor puts them in.
    ///
    /// Clicking one opens it, or focuses the tab it already has - a
    /// project is worth exactly one tab (owner's requirement,
    /// 2026-10-26), which the editor's `uri_affinity` already arranges
    /// by claiming any request for a folder it has open.
    fn sidebar(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) -> bool {
        let s = cx.services;
        let (theme, _) = Self::theme(s);
        if hs_ui::form::sidebar_header(
            ui,
            &theme,
            &s.t("editor-projects"),
            &[(ph::FOLDER_PLUS, &s.t("editor-add-project"))],
        )
        .is_some()
        {
            // One place manages a project, as one place manages a
            // connection: the sidebar's + takes the user to the form in
            // Settings with the folder chooser already open (owner's
            // requirement, 2026-10-27).
            cx.actions.push(hs_core::tool::Action::RunCommand {
                id: "settings:project:new:pick".into(),
            });
        }
        // (id, name, path, icon, is open here) - read once, because the
        // store is behind a lock and this runs every frame.
        let open_root = self.root().map(hs_core::projects::normalise);
        let rows: Vec<(String, String, String, &'static str, bool)> = s
            .projects
            .read()
            .all()
            .iter()
            .map(|p| {
                let mine =
                    open_root.as_deref() == Some(hs_core::projects::normalise(&p.root).as_str());
                (
                    p.id.clone(),
                    p.label(),
                    p.root.display().to_string(),
                    hs_ui::icons::language_icon(&p.language),
                    mine,
                )
            })
            .collect();
        if rows.is_empty() {
            hs_ui::form::note(ui, &theme, ph::LIGHTBULB, &s.t("editor-no-projects"));
            return true;
        }
        let mut reorder = hs_ui::reorder::Reorder::new("editor-project-list");
        let mut open: Option<String> = None;
        let mut edit: Option<String> = None;
        let mut remove: Option<String> = None;
        egui::ScrollArea::vertical()
            .id_salt(self.id.with("projects"))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for (i, (id, name, path, icon, mine)) in rows.iter().enumerate() {
                    reorder.row(ui, i, |ui| {
                        let row = hs_ui::form::list_row(ui, &theme, *mine, icon, name, Some(path));
                        if row.clicked() {
                            open = Some(id.clone());
                        }
                        row.context_menu(|ui| {
                            if ui.button(s.t("editor-project-edit")).clicked() {
                                edit = Some(id.clone());
                                ui.close();
                            }
                            ui.separator();
                            if ui.button(s.t("settings-project-remove")).clicked() {
                                remove = Some(id.clone());
                                ui.close();
                            }
                        });
                    });
                    ui.add_space(2.0);
                }
            });
        if let Some((from, to)) = reorder.finish(ui, theme.accent)
            && let Err(e) = s.projects.write().reorder(from, to)
        {
            cx.toast(ToastLevel::Error, e.to_string());
        }
        if let Some(id) = open {
            let root = s.projects.read().by_id(&id).map(|p| p.root.clone());
            if let Some(root) = root {
                // `open` rather than `open_new_tab`: the shell matches the
                // folder against what every editor tab has open, so a
                // project that is already open is focused instead of
                // opened twice.
                cx.open(
                    "editor",
                    Some(OpenRequest::File {
                        uri: root.to_string_lossy().to_string(),
                        line: None,
                    }),
                );
            }
        }
        if let Some(id) = edit {
            cx.actions.push(hs_core::tool::Action::RunCommand {
                id: format!("settings:project:{id}"),
            });
        }
        if let Some(id) = remove {
            cx.actions.push(hs_core::tool::Action::RunCommand {
                id: format!("project:remove:{id}"),
            });
        }
        true
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        let zipped = self.zip_result.as_ref().and_then(|slot| slot.lock().take());
        if let Some(result) = zipped {
            self.zip_result = None;
            match result {
                Ok((path, n)) => cx.toast(
                    ToastLevel::Success,
                    cx.services.i18n.t_args(
                        "editor-download-zipped",
                        &[
                            ("count", &n.to_string()),
                            ("path", &path.display().to_string()),
                        ],
                    ),
                ),
                Err(e) => cx.toast(ToastLevel::Error, e),
            }
        }
        let s = cx.services;
        self.tab_id = cx.tab_id;
        let had_split = !self.split_off.is_empty();
        for ps in std::mem::take(&mut self.split_off) {
            let file = |p: &Path| {
                Some(OpenRequest::File {
                    uri: p.to_string_lossy().to_string(),
                    line: None,
                })
            };
            if let Some(r) = &ps.root {
                cx.open_new_tab("editor", file(r));
            }
            for f in &ps.files {
                cx.open("editor", file(&f.path));
            }
        }
        if had_split {
            // Opening those tabs focused them; this one was the active tab.
            cx.actions
                .push(hs_core::tool::Action::FocusTab { tab_id: cx.tab_id });
        }
        self.poll_watch(s);
        self.git.poll(s, ui.ctx());
        self.prompt_modal(ui, cx);
        self.delete_dialog(ui, cx);
        self.close_dialog(ui, s);
        self.unsaved_before_running(ui, cx);
        self.ensure_doc(s);
        self.lsp.poll(s);
        self.lsp_sync_docs();
        self.lsp_apply_actions(cx);
        self.debug.poll(s);
        self.debug_apply_actions(cx);
        self.sync_project(s);
        // Asks the toolchain once per language, on the blocking pool;
        // every later frame reads the cached answer.
        langver::ensure(&s.runtime, &self.proj_lang);
        let (theme, ed_theme) = Self::theme(s);
        // The home tab is the dashboard, full stop. Letting a file or a
        // folder in it turn it back into an editor is what made the
        // Editor read as having no dashboard at all: open one file and
        // it was gone for the life of the workspace, with no way back
        // (owner's report, 2026-10-27). The file is not lost - with no
        // uri and no affinity below, the shell opens it in a tab beside
        // this one, which is what every other activity does.
        if self.is_home {
            self.dashboard(ui, cx);
            return;
        }
        // The work pane is two columns: the folder on the left, the
        // editor on the right. The left panel is declared first so the
        // tab strip, the path row and the editor lay out in what is
        // left - the toolbar above them spans both, because the shell
        // draws it outside this pane (owner's requirement, 2026-10-26).
        let mut column = egui::Panel::left(self.id.with("workspace-column"))
            .default_size(260.0)
            .size_range(180.0..=(ui.available_width() * 0.5).max(180.0))
            .resizable(true);
        if !self.show_column {
            column = column.exact_size(0.0).resizable(false);
        }
        let mut column_actions = Vec::new();
        column.show(ui, |ui| {
            if self.show_column {
                let mut inner = ToolContext {
                    services: cx.services,
                    tab_id: cx.tab_id,
                    env_id: cx.env_id,
                    actions: &mut column_actions,
                    is_home: cx.is_home,
                };
                self.workspace_column(ui, &mut inner);
            }
        });
        cx.actions.append(&mut column_actions);
        self.file_tabs(ui, s, &theme);
        if self.git.has_main() {
            let size = self.doc().view.font_size;
            let actions = self.git.main_ui(ui, s, &theme, size);
            self.apply_git(actions, ui, cx);
            return;
        }
        {
            let pa = self.active;
            let p = &mut self.projects[pa];
            let d = &mut p.docs[p.active];
            if let Some(m) =
                self.git
                    .update_marks(&mut d.git, d.path.as_deref(), &d.buf, s, ui.ctx())
            {
                d.view.line_marks = m;
            }
        }
        self.path_row(ui, s, &theme);
        self.doc_mut().disk_banner(ui, s);
        let lsp_badge = {
            let pa = self.active;
            let p = &self.projects[pa];
            let d = &p.docs[p.active];
            self.lsp.status(s, d.path.as_deref(), d.view.language.name)
        };
        let counts = self.lsp.counts();
        let mut status = StatusRow::default();
        egui::Panel::bottom(self.id.with("status"))
            .exact_size(22.0)
            .resizable(false)
            .show(ui, |ui| {
                status = self.doc_mut().status_row(ui, s, lsp_badge.as_ref(), counts);
            });
        let encoding_error = status.encoding_error.take();
        if status.problems_clicked {
            self.lsp.toggle_problems();
        }
        if status.install_clicked
            && let Some(install) = lsp_badge.as_ref().and_then(|b| b.install.clone())
        {
            self.install_language_server(&install, ui, cx);
        }
        if let Some(error) = encoding_error {
            cx.toast(ToastLevel::Error, error);
        }
        if self.terminal.open {
            let root = self.projects[self.active].root.clone();
            let mut out = terminal::PanelOut::default();
            egui::Panel::bottom(self.id.with("terminal"))
                .default_size(220.0)
                .size_range(80.0..=600.0)
                .resizable(true)
                .show(ui, |ui| {
                    out = self.terminal.panel_ui(ui, s, &theme, root.as_deref());
                });
            if let Some(link) = out.link {
                self.open_terminal_link(link, ui.ctx(), cx);
            }
            // A web application that has just said where it listens opens
            // in the Browser, as a page of its own.
            // Owner's requirement, 2026-10-17.
            if let Some(url) = out.url {
                cx.toast(
                    ToastLevel::Info,
                    s.i18n.t_args("editor-run-serving", &[("url", &url)]),
                );
                cx.open("browser", Some(OpenRequest::Url { url }));
            }
        }
        if self.debug.panel_open {
            let root = self.projects[self.active].root.clone();
            egui::Panel::bottom(self.id.with("debug"))
                .default_size(200.0)
                .size_range(80.0..=600.0)
                .resizable(true)
                .show(ui, |ui| self.debug.panel_ui(ui, s, &theme, root.as_deref()));
        }
        if self.lsp.panel_open {
            let root = self.projects[self.active].root.clone();
            egui::Panel::bottom(self.id.with("problems"))
                .default_size(170.0)
                .size_range(80.0..=600.0)
                .resizable(true)
                .show(ui, |ui| self.lsp.panel_ui(ui, s, &theme, root.as_deref()));
        }
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
        let (pa, pd) = (self.active, self.projects[self.active].active);
        let d = &mut self.projects[pa].docs[pd];
        if d.preview && d.view.language.is_markdown {
            let w = ui.available_width() / 2.0;
            egui::Panel::right(d.view.id.with("preview"))
                .default_size(w)
                .size_range(160.0..=(ui.available_width() - 160.0).max(160.0))
                .resizable(true)
                .show(ui, |ui| d.preview_ui(ui, ed_theme.dark, &theme));
        }
        match d.path.clone() {
            Some(path) => {
                d.view.squiggles = self.lsp.squiggles(&path, &d.buf);
                let (bps, exec) = self.debug.marks(&path);
                d.view.breakpoints = bps;
                d.view.exec_line = exec;
            }
            None => {
                d.view.squiggles.clear();
                d.view.breakpoints.clear();
                d.view.exec_line = None;
            }
        }
        let r = d.view.show(ui, &mut d.buf, &mut d.cursors, &ed_theme);
        let path = d.path.clone();
        if let Some(line) = r.gutter_clicked
            && let Some(p) = &path
        {
            self.debug.toggle_breakpoint(p, line as u32 + 1);
        }
        self.lsp.after_show(
            ui,
            s,
            &theme,
            path.as_deref(),
            &mut d.buf,
            &mut d.cursors,
            &mut d.view,
            &r,
        );
        self.lsp_apply_actions(cx);
        if r.save_requested {
            self.save(cx);
        }
    }

    fn speakable(&self) -> Vec<hs_core::tool::Speakable> {
        let mut out = Vec::new();
        let d = self.doc();
        let sel = hs_editor::view::selected_text(&d.buf, &d.cursors);
        if !sel.trim().is_empty() {
            out.push(hs_core::tool::Speakable::new("Selection", sel));
        }
        let text = d.buf.text();
        if !text.trim().is_empty() {
            out.push(hs_core::tool::Speakable::new(
                format!("File: {}", d.file_name()),
                text,
            ));
        }
        if let Some(a) = self.ai.last_answer() {
            out.push(hs_core::tool::Speakable::new("Assistant's last answer", a));
        }
        out
    }

    fn printable(&self) -> Option<hs_core::tool::Printable> {
        let d = self.doc();
        let text = d.buf.text();
        let title = d.file_name();
        let subtitle = d
            .path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        let html = if d.preview && d.view.language.is_markdown {
            hs_editor::markdown::to_html(&text)
        } else {
            hs_editor::highlight::to_html(&text, &d.view.language, false)
        };
        Some(hs_core::tool::Printable {
            title,
            subtitle,
            html,
        })
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
            c("editor.run", "Editor: Run File or Project", Some("run.alt")),
            c(
                "editor.complete",
                "Editor: Show Completions",
                Some("editor.complete"),
            ),
            c("editor.hover", "Editor: Show Hover", Some("editor.hover")),
            c(
                "editor.definition",
                "Editor: Go to Definition",
                Some("editor.definition"),
            ),
            c(
                "editor.references",
                "Editor: Find References",
                Some("editor.references"),
            ),
            c(
                "editor.rename_symbol",
                "Editor: Rename Symbol",
                Some("editor.rename_symbol"),
            ),
            c(
                "editor.format",
                "Editor: Format Document",
                Some("editor.format"),
            ),
            c(
                "editor.problems",
                "Editor: Toggle Problems",
                Some("editor.problems"),
            ),
            c(
                "debug.start",
                "Debug: Start or Continue",
                Some("debug.start"),
            ),
            c("debug.stop", "Debug: Stop", Some("debug.stop")),
            c(
                "debug.step_over",
                "Debug: Step Over",
                Some("debug.step_over"),
            ),
            c(
                "debug.step_into",
                "Debug: Step Into",
                Some("debug.step_into"),
            ),
            c("debug.step_out", "Debug: Step Out", Some("debug.step_out")),
            c(
                "debug.toggle_breakpoint",
                "Debug: Toggle Breakpoint",
                Some("debug.toggle_breakpoint"),
            ),
            c(
                "debug.panel",
                "Debug: Toggle Debug Panel",
                Some("debug.panel"),
            ),
            c(
                "editor.terminal",
                "Editor: Toggle Terminal",
                Some("editor.terminal"),
            ),
            c(
                "editor.build",
                "Editor: Build the Project",
                Some("editor.build"),
            ),
            c("editor.ai", "AI: Toggle Assistant", Some("editor.ai")),
            c(
                "git.source_control",
                "Git: Toggle Source Control",
                Some("git.source_control"),
            ),
            c("git.history", "Git: Show History", Some("git.history")),
            c("git.refresh", "Git: Refresh Status", None),
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
            "file.new" => self.new_file(s),
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
            "editor.complete" => {
                if let Some((path, d)) = Self::lsp_target(&self.projects, self.active) {
                    self.lsp.request_completion(&path, &d.buf, &d.cursors, None);
                }
            }
            "editor.hover" => {
                if let Some((path, d)) = Self::lsp_target(&self.projects, self.active) {
                    self.lsp
                        .request_hover_at_cursor(&path, &d.buf, &d.cursors, &d.view);
                }
            }
            "editor.definition" => {
                if let Some((path, d)) = Self::lsp_target(&self.projects, self.active) {
                    self.lsp
                        .request_definition(&path, &d.buf, d.cursors.primary().head);
                }
            }
            "editor.references" => {
                if let Some((path, d)) = Self::lsp_target(&self.projects, self.active) {
                    self.lsp
                        .request_references(&path, &d.buf, d.cursors.primary().head);
                }
            }
            "editor.rename_symbol" => {
                let ok = match Self::lsp_target(&self.projects, self.active) {
                    Some((path, d)) => self.lsp.start_rename(&path, &d.buf, &d.cursors),
                    None => false,
                };
                if !ok {
                    cx.toast(ToastLevel::Info, s.t("editor-rename-none"));
                }
            }
            "editor.format" => {
                let ok = match Self::lsp_target(&self.projects, self.active) {
                    Some((path, d)) => self.lsp.request_formatting(&path, &d.buf, &d.view),
                    None => false,
                };
                if !ok {
                    cx.toast(ToastLevel::Info, s.t("editor-format-none"));
                }
            }
            "editor.problems" => self.lsp.toggle_problems(),
            "debug.start" => self.debug_start(cx),
            "debug.stop" => self.debug.stop(),
            "debug.step_over" => self.debug.step_over(),
            "debug.step_into" => self.debug.step_into(),
            "debug.step_out" => self.debug.step_out(),
            "debug.toggle_breakpoint" => {
                let d = self.doc();
                if let Some(path) = d.path.clone() {
                    let line = d.buf.char_to_line_col(d.cursors.primary().head).0 as u32 + 1;
                    self.debug.toggle_breakpoint(&path, line);
                }
            }
            "debug.panel" => self.debug.panel_open = !self.debug.panel_open,
            "editor.terminal" => {
                let cwd = self.terminal_cwd();
                self.terminal.toggle(s, cwd.as_deref());
            }
            "editor.build" => self.build_now(cx),
            "git.source_control" => self.git.sidebar = !self.git.sidebar,
            "git.history" => {
                let path = self
                    .doc()
                    .path
                    .clone()
                    .or_else(|| self.projects[self.active].root.clone());
                if !self.git.open_history_for(path.as_deref(), s) {
                    cx.toast(ToastLevel::Info, s.t("git-no-repo-toast"));
                }
            }
            "git.refresh" => self.git.refresh_all(),
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
            "editor.run" | "run.alt" => self.run_current(cx),
            "editor.terminal_here" => {
                let dir = self
                    .doc()
                    .path
                    .as_ref()
                    .and_then(|p| p.parent().map(Path::to_path_buf))
                    .or(self.projects[self.active].root.clone());
                match dir {
                    Some(d) => self.terminal.show_in(s, &d),
                    None => {
                        let cwd = self.terminal_cwd();
                        self.terminal.toggle(s, cwd.as_deref());
                    }
                }
            }
            _ => return false,
        }
        true
    }

    fn current_uri(&self) -> Option<String> {
        if self.is_home {
            return None;
        }
        self.doc()
            .path
            .as_ref()
            .map(|p| p.to_string_lossy().to_string())
    }

    fn uri_affinity(&self, uri: &str) -> Option<u8> {
        // The dashboard answers for nothing: a home tab that claims a
        // file is handed it, and stops being a dashboard.
        if self.is_home {
            return None;
        }
        let raw_path = PathBuf::from(uri.strip_prefix("file://").unwrap_or(uri));
        let path = normalize_path(std::fs::canonicalize(&raw_path).unwrap_or(raw_path.clone()));
        if self.find_doc(&path).is_some() {
            return Some(0);
        }
        match self.root() {
            Some(r) if same_path(&path, r) => Some(0),
            // The project holding the file; of nested projects the
            // innermost (fewest folders between it and the file).
            Some(r) if !path.is_dir() && (path.starts_with(r) || raw_path.starts_with(r)) => {
                let path = if raw_path.starts_with(r) {
                    &raw_path
                } else {
                    &path
                };
                let depth = path.strip_prefix(r).map_or(0, |p| p.components().count());
                Some((1 + depth).min(150) as u8)
            }
            Some(_) => None,
            None if path.is_dir() => self.is_empty_tab().then_some(200),
            // The Other Files tab (or an empty one) takes any other file.
            None if path.is_absolute() => Some(200),
            None => None,
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
            source_control: self.git.sidebar,
            side_by_side: self.git.side_by_side,
            breakpoints: self.debug.saved_breakpoints(),
            ..Default::default()
        };
        serde_json::to_value(&st).unwrap_or_default()
    }

    fn open(&mut self, item: OpenRequest, cx: &mut ToolContext) -> bool {
        if self.is_home {
            return false;
        }
        match item {
            OpenRequest::File { uri, line } => {
                let path = PathBuf::from(uri.strip_prefix("file://").unwrap_or(&uri));
                self.take_path(path, line, cx);
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
            self.trim_trailing = st.editor.trim_trailing_whitespace;
            self.final_newline = st.editor.insert_final_newline;
            drop(st);
            self.lsp.reconfigure(cx.services);
            self.debug.read_settings(cx.services);
            for p in &mut self.projects {
                for d in &mut p.docs {
                    d.apply_settings(cx.services);
                }
            }
        }
    }
}

/// Compare project paths after filesystem canonicalization where possible;
/// Windows paths are case-insensitive and may arrive with mixed separators.
fn same_path(a: &Path, b: &Path) -> bool {
    let a = normalize_path(std::fs::canonicalize(a).unwrap_or_else(|_| a.to_path_buf()));
    let b = normalize_path(std::fs::canonicalize(b).unwrap_or_else(|_| b.to_path_buf()));
    let a = a.to_string_lossy().replace('\\', "/");
    let b = b.to_string_lossy().replace('\\', "/");
    if cfg!(windows) {
        a.eq_ignore_ascii_case(&b)
    } else {
        a == b
    }
}

fn normalize_path(path: PathBuf) -> PathBuf {
    #[cfg(windows)]
    {
        if let Some(path) = path.to_string_lossy().strip_prefix(r"\\?\") {
            return PathBuf::from(path);
        }
    }
    path
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

/// A row of equal-width toggle segments across the available width (never
/// wider, so a resizable side panel does not grow). Returns the clicked one.
fn segmented(ui: &mut egui::Ui, theme: &Theme, labels: &[&str], selected: usize) -> Option<usize> {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 26.0), egui::Sense::hover());
    let w = rect.width() / labels.len().max(1) as f32;
    let mut clicked = None;
    for (i, label) in labels.iter().enumerate() {
        let r = egui::Rect::from_min_size(
            egui::pos2(rect.left() + i as f32 * w, rect.top()),
            egui::vec2(w, rect.height()),
        )
        .shrink2(egui::vec2(1.0, 0.0));
        let resp = ui.interact(r, ui.id().with(("segment", i)), egui::Sense::click());
        resp.widget_info(|| {
            egui::WidgetInfo::selected(
                egui::WidgetType::SelectableLabel,
                true,
                i == selected,
                *label,
            )
        });
        let on = i == selected;
        let fill = if on {
            theme.accent.linear_multiply(0.3)
        } else if resp.hovered() {
            theme.surface_hover
        } else {
            theme.surface
        };
        ui.painter()
            .rect_filled(r, egui::CornerRadius::same(4), fill);
        let p = ui.painter().with_clip_rect(r);
        p.text(
            r.center(),
            egui::Align2::CENTER_CENTER,
            *label,
            egui::FontId::proportional(13.0),
            if on { theme.text } else { theme.text_muted },
        );
        if resp.clicked() {
            clicked = Some(i);
        }
    }
    clicked
}

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

/// Write `dir` (recursively, `.git` skipped) as a deflate ZIP at `target`.
/// Returns the number of files stored.
pub fn zip_dir(dir: &Path, target: &Path) -> Result<usize, String> {
    use std::io::Write;
    let file = std::fs::File::create(target).map_err(|e| format!("{}: {e}", target.display()))?;
    let mut zip = zip::ZipWriter::new(std::io::BufWriter::new(file));
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o644);
    let base = dir
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "folder".into());
    let mut count = 0usize;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let mut entries: Vec<_> = std::fs::read_dir(&d)
            .map_err(|e| format!("{}: {e}", d.display()))?
            .flatten()
            .collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let p = e.path();
            let rel = p.strip_prefix(dir).unwrap_or(&p);
            let name = format!("{base}/{}", rel.to_string_lossy().replace('\\', "/"));
            if p.is_dir() {
                if p.file_name().is_some_and(|n| n == ".git") {
                    continue;
                }
                zip.add_directory(format!("{name}/"), opts)
                    .map_err(|e| e.to_string())?;
                stack.push(p);
            } else if p.is_file() {
                zip.start_file(name, opts).map_err(|e| e.to_string())?;
                let bytes = std::fs::read(&p).map_err(|e| format!("{}: {e}", p.display()))?;
                zip.write_all(&bytes).map_err(|e| e.to_string())?;
                count += 1;
            }
        }
    }
    zip.finish().map_err(|e| e.to_string())?;
    Ok(count)
}

impl EditorTool {
    /// Tell the language servers about every open file: new ones are
    /// opened, closed ones closed, edited ones synced (debounced).
    fn lsp_sync_docs(&mut self) {
        let mut open: Vec<PathBuf> = Vec::new();
        for p in &self.projects {
            for d in &p.docs {
                let Some(path) = &d.path else { continue };
                open.push(path.clone());
                if !d.view.read_only && !self.lsp.knows(path) {
                    self.lsp
                        .open_doc(path, d.view.language.name, &d.buf.text(), p.root.as_deref());
                }
                self.lsp.sync(path, &d.buf);
            }
        }
        self.lsp.retain_open(&open);
    }

    /// Carry out what the language server asked for.
    fn lsp_apply_actions(&mut self, cx: &mut ToolContext) {
        for a in self.lsp.take_actions() {
            match a {
                lsp::LspAction::GoTo {
                    path,
                    line,
                    col,
                    enc,
                } => {
                    self.open_path(path.clone(), Some(line + 1), cx);
                    if let Some((pi, di)) = self.find_doc(&path) {
                        let d = &mut self.projects[pi].docs[di];
                        let text = d.buf.line_text(line as usize);
                        let c = hs_lsp::text::col_from_lsp(
                            text.trim_end_matches(['\r', '\n']),
                            col,
                            enc,
                        );
                        let pos = d.buf.line_col_to_char(line as usize, c);
                        d.cursors.set_single(Cursor::at(pos));
                        d.view.scroll_to_cursor();
                    }
                }
                lsp::LspAction::ApplyEdits { path, edits, enc } => {
                    if let Some((pi, di)) = self.find_doc(&path) {
                        let d = &mut self.projects[pi].docs[di];
                        let chars =
                            hs_lsp::text::edits_to_chars(&lsp::BufLines(&d.buf), &edits, enc);
                        d.view.apply_edits(&mut d.buf, &mut d.cursors, &chars, None);
                    } else {
                        match std::fs::read_to_string(&path) {
                            Ok(old) => {
                                let new = hs_lsp::text::apply_edits(&old, &edits, enc);
                                if new != old
                                    && let Err(e) =
                                        hs_core::storage::write_atomic(&path, new.as_bytes())
                                {
                                    cx.toast(ToastLevel::Error, e.to_string());
                                }
                            }
                            Err(e) => cx.toast(ToastLevel::Error, e.to_string()),
                        }
                    }
                }
                lsp::LspAction::Toast(level, text) => cx.toast(level, text),
            }
        }
    }

    /// Install a missing language server: run its command in a new
    /// terminal tab, or open its download page when no single command
    /// works on every platform.
    fn install_language_server(
        &mut self,
        install: &lsp::Install,
        ui: &egui::Ui,
        cx: &mut ToolContext,
    ) {
        if !install.command.is_empty() {
            let cwd = self.projects[self.active]
                .root
                .clone()
                .map(|r| r.to_string_lossy().to_string());
            cx.open_new_tab(
                "terminal",
                Some(OpenRequest::Terminal {
                    cwd,
                    host: None,
                    profile: None,
                    command: Some(install.command.clone()),
                }),
            );
            let msg = cx.services.i18n.t_args(
                "editor-lsp-installing",
                &[("server", &install.server), ("command", &install.command)],
            );
            cx.toast(ToastLevel::Info, msg);
        } else if !install.url.is_empty() {
            ui.ctx()
                .open_url(egui::OpenUrl::new_tab(install.url.clone()));
        }
    }

    /// Carry out what the debugger asked for.
    fn debug_apply_actions(&mut self, cx: &mut ToolContext) {
        for a in self.debug.take_actions() {
            match a {
                debug::DebugAction::GoTo { path, line } => {
                    self.open_path(path, Some(line), cx);
                }
                debug::DebugAction::Open(path) => self.open_path(path, None, cx),
                debug::DebugAction::Toast(level, text) => cx.toast(level, text),
            }
        }
    }

    /// The active file for a language-server request (borrows only the
    /// projects, so `self.lsp` stays free).
    fn lsp_target(projects: &[Project], active: usize) -> Option<(PathBuf, &Doc)> {
        let p = &projects[active];
        let d = &p.docs[p.active];
        Some((d.path.clone()?, d))
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

#[cfg(test)]
mod run_plan_tests {
    use super::run_plan;
    use std::path::Path;

    fn settings() -> hs_core::settings::Editor {
        hs_core::settings::Editor::default()
    }

    #[test]
    fn project_runner_wins_over_file_interpreter() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("Cargo.toml"), "[package]").unwrap();
        let src = tmp.path().join("src").join("main.rs");
        let (cwd, cmd) = run_plan(&src, Some(tmp.path()), &settings()).unwrap();
        assert_eq!(cwd, tmp.path());
        assert_eq!(cmd, "cargo run");
        let (cwd, cmd) = run_plan(Path::new("C:/x/tool.py"), None, &settings()).unwrap();
        assert_eq!(cwd, Path::new("C:/x"));
        assert_eq!(cmd, "python C:/x/tool.py");
        assert!(run_plan(Path::new("C:/x/notes.txt"), None, &settings()).is_none());
    }

    #[test]
    fn the_projects_build_file_runs_the_project() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("pom.xml"), "<project/>").unwrap();
        let (cwd, cmd) = run_plan(&tmp.path().join("pom.xml"), Some(tmp.path()), &settings())
            .expect("pom.xml is runnable: it is the project");
        assert_eq!(cwd, tmp.path());
        assert_eq!(cmd, "mvn -q compile exec:java");
    }

    #[test]
    fn a_spring_boot_project_runs_its_goal_and_a_script_in_it_still_runs_itself() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("pom.xml"),
            "<project><artifactId>spring-boot-starter-web</artifactId></project>",
        )
        .unwrap();
        let java = tmp.path().join("src/main/java/App.java");
        let (cwd, cmd) = run_plan(&java, Some(tmp.path()), &settings()).unwrap();
        assert_eq!(cwd, tmp.path());
        assert_eq!(cmd, "mvn spring-boot:run");
        // A helper script inside the same project is still a script.
        let py = tmp.path().join("tools/gen.py");
        let (_, cmd) = run_plan(&py, Some(tmp.path()), &settings()).unwrap();
        assert!(cmd.starts_with("python "), "{cmd}");
    }
}

#[cfg(test)]
mod zip_tests {
    use super::zip_dir;

    #[test]
    fn a_folder_zips_recursively_without_its_git_dir() {
        let dir = std::env::temp_dir().join(format!("hs-zip-{}", hs_core::new_id()));
        std::fs::create_dir_all(dir.join("proj/src")).unwrap();
        std::fs::create_dir_all(dir.join("proj/.git")).unwrap();
        std::fs::write(dir.join("proj/README.md"), "# hi\n").unwrap();
        std::fs::write(dir.join("proj/src/main.rs"), "fn main() {}\n").unwrap();
        std::fs::write(dir.join("proj/.git/HEAD"), "ref: refs/heads/main\n").unwrap();
        let target = dir.join("proj.zip");
        let n = zip_dir(&dir.join("proj"), &target).unwrap();
        assert_eq!(n, 2);
        let file = std::fs::File::open(&target).unwrap();
        let mut archive = zip::ZipArchive::new(file).unwrap();
        let names: Vec<String> = (0..archive.len())
            // zip 9 returns the name as a Result: a mangled entry name is an
            // error rather than lossy text.
            .map(|i| archive.by_index(i).unwrap().name().unwrap().to_string())
            .collect();
        assert!(names.contains(&"proj/README.md".to_string()));
        assert!(names.contains(&"proj/src/main.rs".to_string()));
        assert!(names.iter().all(|n| !n.contains(".git")));
        let mut body = String::new();
        std::io::Read::read_to_string(&mut archive.by_name("proj/src/main.rs").unwrap(), &mut body)
            .unwrap();
        assert_eq!(body, "fn main() {}\n");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod minified_tests {
    use hs_editor::Buffer;

    /// What `Doc::lay_out_minified` decides, without a `Services`: a JSON
    /// file on one line is laid out, a tidy one is left alone.
    fn is_minified(text: &str) -> bool {
        let mut buf = Buffer::from_text(text);
        buf.len_lines() <= 2 || buf.max_line_chars() > 400
    }

    #[test]
    fn only_a_minified_json_file_is_laid_out_on_open() {
        let map = r#"{"version":3,"sources":["a.ts"],"mappings":"AAAA,SAAS"}"#;
        assert!(is_minified(map));
        assert!(hs_core::fmt::pretty_json(map).is_some());

        let tidy = "{\n  \"a\": 1,\n  \"b\": [\n    1\n  ]\n}\n";
        assert!(!is_minified(tidy), "an already laid-out file is left alone");

        let long_line = format!("{{\"k\":\"{}\"}}\n\n\n", "x".repeat(500));
        assert!(is_minified(&long_line), "one very long line counts too");
    }

    #[test]
    fn source_maps_are_json_for_the_highlighter() {
        assert_eq!(hs_editor::languages::detect("bundle.js.map").name, "JSON");
        assert_eq!(hs_editor::languages::detect("app.json").name, "JSON");
    }
}
