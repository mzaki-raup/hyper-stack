//! The **Files** view (FEAT-0101, docs/tools/git-activity.md): the working
//! tree as a folder tree in the centre column and the picked file in the
//! right column - read-only until **Edit**, then the editor engine; **Save**
//! writes the file and the pane's status refreshes, so the change lands in
//! File Status for the commit box; **New File** and **New Folder** create
//! under the picked folder; **Open in Editor** hands the file to the Editor
//! activity. Every read and write runs on the blocking pool and comes back
//! through a slot; the UI thread only draws the last answer.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use hs_core::services::Services;
use hs_core::tool::{OpenRequest, ToastLevel, ToolContext};
use hs_editor::view::EditorTheme;
use hs_editor::{Buffer, Cursors, EditorView, languages};
use hs_ui::form;
use hs_ui::icons::ph;
use hs_ui::theme::Theme;
use hs_ui::tip::TipExt;
use hs_ui::tree::{TreeIcon, TreeNode, TreeState};
use parking_lot::Mutex;

type Slot<T> = Arc<Mutex<Option<T>>>;

/// Files above this are not opened in the preview: the editor engine is
/// for source, not for archives.
const MAX_TEXT: u64 = 2 * 1024 * 1024;

/// One folder's listing: (name, is a folder), folders first, by name.
type Listing = Vec<(String, bool)>;

/// Listings back from the pool, several at once: (folder, outcome).
type DirSlot = Arc<Mutex<Vec<(String, Result<Listing, String>)>>>;

/// What a read of a file gave.
#[derive(Debug, Clone)]
pub enum Loaded {
    Text(String),
    Binary { size: u64 },
    TooBig { size: u64 },
}

/// The file in the right column.
struct OpenFile {
    /// Repository-relative, `/`-separated.
    path: String,
    loaded: Loaded,
    buf: Buffer,
    cursors: Cursors,
    view: EditorView,
    editing: bool,
}

/// A name being asked for.
enum Prompt {
    NewFile { dir: String, name: String },
    NewFolder { dir: String, name: String },
}

/// What the pane does after the view: a write happened, so the status is
/// read again and File Status shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Changed,
}

#[derive(Default)]
pub struct FilesView {
    root: Option<PathBuf>,
    pub tree: TreeState,
    /// Listed folders by relative path (`""` is the root).
    dirs: BTreeMap<String, Listing>,
    loading: HashSet<String>,
    dir_slot: DirSlot,
    file: Option<OpenFile>,
    file_slot: Slot<(String, Result<Loaded, String>)>,
    /// A write (save, create) on its way: (what, outcome).
    write_slot: Slot<(String, Result<(), String>)>,
    prompt: Option<Prompt>,
    /// Material icons for the rows, read from Settings each frame the tree
    /// is drawn.
    material: bool,
    error: Option<String>,
}

impl FilesView {
    pub fn set_root(&mut self, root: PathBuf) {
        *self = Self::default();
        self.root = Some(root);
    }

    /// New File from the palette or the toolbar: ask for a name under the
    /// picked folder.
    pub fn prompt_new_file(&mut self) {
        self.prompt = Some(Prompt::NewFile {
            dir: self.target_dir(),
            name: String::new(),
        });
    }

    pub fn prompt_new_folder(&mut self) {
        self.prompt = Some(Prompt::NewFolder {
            dir: self.target_dir(),
            name: String::new(),
        });
    }

    /// Forget the listings; open folders are listed again as they are
    /// drawn. The open file stays.
    pub fn refresh(&mut self) {
        self.dirs.clear();
        self.loading.clear();
    }

    /// The folder the next New File / New Folder lands in: the picked
    /// folder, the picked file's folder, or the root.
    fn target_dir(&self) -> String {
        let Some(sel) = self.tree.selected.as_deref() else {
            return String::new();
        };
        if self.dirs.contains_key(sel) || self.loading.contains(sel) {
            return sel.to_string();
        }
        parent_of(sel).to_string()
    }

    fn list_dir(&mut self, s: &Services, dir: String) {
        let Some(root) = self.root.clone() else {
            return;
        };
        if !self.loading.insert(dir.clone()) {
            return;
        }
        let slot = self.dir_slot.clone();
        s.runtime.spawn_blocking(move || {
            let r = read_listing(&root.join(&dir));
            slot.lock().push((dir, r));
        });
    }

    fn open_file(&mut self, s: &Services, path: String) {
        let Some(root) = self.root.clone() else {
            return;
        };
        let slot = self.file_slot.clone();
        let abs = root.join(&path);
        s.runtime.spawn_blocking(move || {
            let r = read_file(&abs);
            *slot.lock() = Some((path, r));
        });
    }

    /// Take what the pool left: listings, the opened file, a finished
    /// write. A write that landed means the status must be read again.
    pub fn poll(&mut self, cx: &mut ToolContext) -> Option<Event> {
        let s = cx.services;
        let done: Vec<_> = std::mem::take(&mut *self.dir_slot.lock());
        for (dir, r) in done {
            self.loading.remove(&dir);
            match r {
                Ok(list) => {
                    self.dirs.insert(dir, list);
                }
                Err(e) => self.error = Some(e),
            }
        }
        if let Some((path, r)) = self.file_slot.lock().take() {
            match r {
                Ok(loaded) => {
                    let text = match &loaded {
                        Loaded::Text(t) => t.clone(),
                        _ => String::new(),
                    };
                    let dark = Theme::from_settings(&s.settings.read().appearance).dark;
                    let font = s.settings.read().editor.font_size;
                    let mut view = EditorView::new(
                        egui::Id::new(("git-files", hs_core::new_id())),
                        languages::detect(&path),
                        dark,
                        font,
                    );
                    view.read_only = true;
                    view.right_click_copy_paste = s.settings.read().editor.right_click_copy_paste;
                    let mut buf = Buffer::from_text(&text);
                    buf.mark_saved();
                    self.file = Some(OpenFile {
                        path,
                        loaded,
                        buf,
                        cursors: Cursors::single(0),
                        view,
                        editing: false,
                    });
                }
                Err(e) => cx.toast(ToastLevel::Error, e),
            }
        }
        let wrote = self.write_slot.lock().take();
        if let Some((what, r)) = wrote {
            return match r {
                Ok(()) => {
                    cx.toast(ToastLevel::Success, what);
                    Some(Event::Changed)
                }
                Err(e) => {
                    cx.toast(ToastLevel::Error, e);
                    None
                }
            };
        }
        None
    }

    fn nodes(&self, dir: &str) -> Vec<TreeNode> {
        let Some(list) = self.dirs.get(dir) else {
            return Vec::new();
        };
        list.iter()
            .map(|(name, is_dir)| {
                let path = if dir.is_empty() {
                    name.clone()
                } else {
                    format!("{dir}/{name}")
                };
                let icon: TreeIcon = if self.material {
                    if *is_dir {
                        hs_ui::fileicons::for_folder(name).into()
                    } else {
                        hs_ui::fileicons::for_file(name).into()
                    }
                } else if *is_dir {
                    ph::FOLDER_SIMPLE.into()
                } else {
                    ph::FILE.into()
                };
                if *is_dir {
                    let mut n = TreeNode::leaf(path.clone(), name.clone(), icon);
                    n.leaf = false;
                    n.children = self.dirs.contains_key(&path).then(|| self.nodes(&path));
                    n
                } else {
                    TreeNode::leaf(path, name.clone(), icon)
                }
            })
            .collect()
    }

    /// The centre column: a toolbar row and the folder tree.
    pub fn tree_ui(&mut self, ui: &mut egui::Ui, theme: &Theme, cx: &mut ToolContext) {
        let s = cx.services;
        self.material = s.settings.read().appearance.material_icons("git");
        if self.root.is_none() {
            return;
        }
        if !self.dirs.contains_key("") {
            self.list_dir(s, String::new());
        }
        ui.add_space(form::PANE_GAP);
        ui.horizontal(|ui| {
            if ui
                .button(format!("{} {}", ph::FILE_PLUS, s.t("git-files-new-file")))
                .clicked()
            {
                self.prompt_new_file();
            }
            if ui
                .button(format!(
                    "{} {}",
                    ph::FOLDER_PLUS,
                    s.t("git-files-new-folder")
                ))
                .clicked()
            {
                self.prompt_new_folder();
            }
            if form::icon_button(ui, theme, ph::ARROW_CLOCKWISE, 14.0, false)
                .tip(s.t("git-files-refresh"))
                .clicked()
            {
                self.refresh();
            }
        });
        ui.add_space(form::PANE_GAP);
        if let Some(e) = &self.error {
            form::note(ui, theme, ph::WARNING, e);
        }
        let nodes = self.nodes("");
        let empty = self.dirs.get("").is_some_and(Vec::is_empty);
        let mut open: Option<String> = None;
        let mut load: Vec<String> = Vec::new();
        egui::ScrollArea::both()
            .id_salt(ui.id().with("git-files-tree"))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if empty {
                    form::note(ui, theme, ph::INFO, &s.t("git-files-empty-folder"));
                    return;
                }
                let r = hs_ui::tree::show(ui, theme, &mut self.tree, &nodes);
                load = r.load;
                if let Some(id) = r.clicked
                    && !self.dirs.contains_key(&id)
                    && !self.loading.contains(&id)
                    && !is_dir_in(&nodes, &id)
                {
                    open = Some(id);
                }
            });
        for d in load {
            self.list_dir(s, d);
        }
        // An open folder whose listing was forgotten (Refresh) is listed
        // again without another click.
        let want: Vec<String> = self
            .tree
            .expanded
            .iter()
            .filter(|d| !self.dirs.contains_key(*d) && !self.loading.contains(*d))
            .cloned()
            .collect();
        for d in want {
            self.list_dir(s, d);
        }
        if let Some(path) = open
            && self.file.as_ref().is_none_or(|f| f.path != path)
        {
            self.open_file(s, path);
        }
        self.prompt_ui(ui, theme, cx);
    }

    /// The right column: the file's header and actions, then its text.
    pub fn preview_ui(&mut self, ui: &mut egui::Ui, theme: &Theme, cx: &mut ToolContext) {
        let s = cx.services;
        let Some(root) = self.root.clone() else {
            return;
        };
        let Some(f) = self.file.as_mut() else {
            ui.add_space(24.0);
            ui.horizontal(|ui| {
                ui.add_space(24.0);
                form::note(ui, theme, ph::FILE_TEXT, &s.t("git-files-pick"));
            });
            return;
        };
        let dirty = f.buf.dirty();
        let mut save = false;
        let mut discard = false;
        egui::Frame::new()
            .fill(theme.surface)
            .inner_margin(egui::Margin::symmetric(12, 8))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(&f.path).strong());
                    hs_ui::toolbar::hard_right(ui, |ui| {
                        if ui
                            .button(format!(
                                "{} {}",
                                ph::ARROW_SQUARE_OUT,
                                s.t("git-files-open-editor")
                            ))
                            .clicked()
                        {
                            cx.open(
                                "editor",
                                Some(OpenRequest::File {
                                    uri: root.join(&f.path).to_string_lossy().to_string(),
                                    line: None,
                                }),
                            );
                        }
                        if matches!(f.loaded, Loaded::Text(_)) {
                            if f.editing {
                                if ui
                                    .add_enabled(dirty, egui::Button::new(s.t("git-files-discard")))
                                    .clicked()
                                {
                                    discard = true;
                                }
                                if ui
                                    .add_enabled(
                                        dirty,
                                        egui::Button::new(format!(
                                            "{} {}",
                                            ph::FLOPPY_DISK,
                                            s.t("save")
                                        )),
                                    )
                                    .clicked()
                                {
                                    save = true;
                                }
                            } else if ui
                                .button(format!("{} {}", ph::PENCIL_SIMPLE, s.t("git-files-edit")))
                                .clicked()
                            {
                                f.editing = true;
                                f.view.read_only = false;
                            }
                        }
                    });
                });
            });
        match &f.loaded {
            Loaded::Binary { size } => {
                ui.add_space(form::PANE_GAP);
                form::note(
                    ui,
                    theme,
                    ph::FILE,
                    &s.i18n
                        .t_args("git-files-binary", &[("size", &human_size(*size))]),
                );
                return;
            }
            Loaded::TooBig { size } => {
                ui.add_space(form::PANE_GAP);
                form::note(
                    ui,
                    theme,
                    ph::FILE,
                    &s.i18n
                        .t_args("git-files-too-big", &[("size", &human_size(*size))]),
                );
                return;
            }
            Loaded::Text(_) => {}
        }
        let ed_theme = EditorTheme::for_theme(theme);
        let resp = f.view.show(ui, &mut f.buf, &mut f.cursors, &ed_theme);
        if resp.save_requested && f.editing && f.buf.dirty() {
            save = true;
        }
        if discard {
            let original = match &f.loaded {
                Loaded::Text(t) => t.clone(),
                _ => String::new(),
            };
            f.buf = Buffer::from_text(&original);
            f.buf.mark_saved();
            f.cursors = Cursors::single(0);
        }
        if save {
            let text = f.buf.text();
            let path = f.path.clone();
            f.loaded = Loaded::Text(text.clone());
            f.buf.mark_saved();
            let abs = root.join(&path);
            let slot = self.write_slot.clone();
            let label = s.i18n.t_args("git-files-saved", &[("name", &path)]);
            s.runtime.spawn_blocking(move || {
                let r = std::fs::write(&abs, text.as_bytes()).map_err(|e| e.to_string());
                *slot.lock() = Some((label, r));
            });
        }
    }

    /// New File / New Folder: a name, then the write on the pool and the
    /// folder listed again.
    fn prompt_ui(&mut self, ui: &mut egui::Ui, theme: &Theme, cx: &mut ToolContext) {
        let s = cx.services;
        let Some(root) = self.root.clone() else {
            return;
        };
        let Some(prompt) = self.prompt.as_mut() else {
            return;
        };
        let (title, hint, dir, name, is_dir) = match prompt {
            Prompt::NewFile { dir, name } => (
                s.t("git-files-new-file"),
                s.t("git-files-new-file-hint"),
                dir.clone(),
                name,
                false,
            ),
            Prompt::NewFolder { dir, name } => (
                s.t("git-files-new-folder"),
                s.t("git-files-new-folder-hint"),
                dir.clone(),
                name,
                true,
            ),
        };
        let mut done = false;
        let mut cancel = false;
        let m = egui::Modal::new(ui.id().with("git-files-prompt")).show(ui.ctx(), |ui| {
            ui.set_width(380.0);
            ui.heading(&title);
            if !dir.is_empty() {
                form::hint(
                    ui,
                    theme,
                    &s.i18n.t_args("git-files-in-folder", &[("dir", &dir)]),
                );
            }
            ui.add_space(6.0);
            let r = form::text_field(ui, name, &hint);
            r.request_focus();
            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                done = true;
            }
            ui.add_space(10.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if form::primary_button(ui, theme, &s.t("create")).clicked() {
                    done = true;
                }
                if ui.button(s.t("cancel")).clicked() {
                    cancel = true;
                }
            });
        });
        if m.should_close() {
            cancel = true;
        }
        let name = name.trim().to_string();
        if cancel {
            self.prompt = None;
            return;
        }
        if !done {
            return;
        }
        if name.is_empty() || name.contains(['/', '\\']) || name == "." || name == ".." {
            cx.toast(ToastLevel::Warning, s.t("git-files-bad-name"));
            return;
        }
        self.prompt = None;
        let rel = if dir.is_empty() {
            name.clone()
        } else {
            format!("{dir}/{name}")
        };
        let abs = root.join(&rel);
        let slot = self.write_slot.clone();
        let label = s.i18n.t_args("git-files-created", &[("name", &rel)]);
        let exists = s.t("git-files-exists");
        s.runtime.spawn_blocking(move || {
            let r = if abs.exists() {
                Err(exists)
            } else if is_dir {
                std::fs::create_dir_all(&abs).map_err(|e| e.to_string())
            } else {
                std::fs::write(&abs, b"").map_err(|e| e.to_string())
            };
            *slot.lock() = Some((label, r));
        });
        // The folder is listed again once the write lands; open it now so
        // the new row is seen, and open the new file in the preview.
        self.dirs.remove(&dir);
        if !dir.is_empty() {
            self.tree.expanded.insert(dir.clone());
        }
        self.tree.selected = Some(rel.clone());
        if !is_dir {
            // The file does not exist for a few milliseconds: show it empty
            // and editable straight away rather than reading it back.
            let dark = Theme::from_settings(&s.settings.read().appearance).dark;
            let font = s.settings.read().editor.font_size;
            let mut view = EditorView::new(
                egui::Id::new(("git-files", hs_core::new_id())),
                languages::detect(&rel),
                dark,
                font,
            );
            view.right_click_copy_paste = s.settings.read().editor.right_click_copy_paste;
            let mut buf = Buffer::from_text("");
            buf.mark_saved();
            self.file = Some(OpenFile {
                path: rel,
                loaded: Loaded::Text(String::new()),
                buf,
                cursors: Cursors::single(0),
                view,
                editing: true,
            });
        }
    }
}

/// Whether `id` names a folder among `nodes` (any depth).
fn is_dir_in(nodes: &[TreeNode], id: &str) -> bool {
    nodes
        .iter()
        .any(|n| (n.id == id && !n.leaf) || n.children.as_ref().is_some_and(|c| is_dir_in(c, id)))
}

/// The folder part of a relative path: `a/b/c.txt` → `a/b`; `c.txt` → ``.
pub fn parent_of(path: &str) -> &str {
    path.rsplit_once('/').map(|(d, _)| d).unwrap_or("")
}

/// One folder's entries, folders first, each set by name without case;
/// `.git` is never listed.
pub fn read_listing(dir: &Path) -> Result<Listing, String> {
    let mut out: Listing = std::fs::read_dir(dir)
        .map_err(|e| e.to_string())?
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            if name == ".git" {
                return None;
            }
            let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
            Some((name, is_dir))
        })
        .collect();
    out.sort_by(|a, b| {
        b.1.cmp(&a.1)
            .then_with(|| a.0.to_lowercase().cmp(&b.0.to_lowercase()))
    });
    Ok(out)
}

/// A file as the preview wants it: its text, or why it has none.
pub fn read_file(path: &Path) -> Result<Loaded, String> {
    let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
    if meta.len() > MAX_TEXT {
        return Ok(Loaded::TooBig { size: meta.len() });
    }
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    Ok(classify(bytes))
}

/// Text unless the first kilobytes hold a NUL, which no text file does.
pub fn classify(bytes: Vec<u8>) -> Loaded {
    let head = &bytes[..bytes.len().min(8192)];
    if head.contains(&0) {
        return Loaded::Binary {
            size: bytes.len() as u64,
        };
    }
    Loaded::Text(String::from_utf8_lossy(&bytes).into_owned())
}

fn human_size(n: u64) -> String {
    let n = n as f64;
    if n < 1024.0 {
        format!("{n} B")
    } else if n < 1024.0 * 1024.0 {
        format!("{:.1} KB", n / 1024.0)
    } else {
        format!("{:.1} MB", n / (1024.0 * 1024.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_listing_puts_folders_first_and_hides_the_git_folder() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".git")).unwrap();
        std::fs::create_dir(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "x").unwrap();
        std::fs::write(dir.path().join("README.md"), "x").unwrap();
        let l = read_listing(dir.path()).unwrap();
        assert_eq!(
            l,
            vec![
                ("src".to_string(), true),
                ("Cargo.toml".to_string(), false),
                ("README.md".to_string(), false)
            ]
        );
    }

    #[test]
    fn a_file_is_text_unless_it_holds_a_nul_or_is_huge() {
        assert!(
            matches!(classify(b"fn main() {}".to_vec()), Loaded::Text(t) if t == "fn main() {}")
        );
        assert!(matches!(
            classify(vec![0x89, b'P', 0, b'G']),
            Loaded::Binary { size: 4 }
        ));
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("big.bin");
        std::fs::write(&p, vec![b'a'; (MAX_TEXT + 1) as usize]).unwrap();
        assert!(matches!(read_file(&p).unwrap(), Loaded::TooBig { .. }));
    }

    #[test]
    fn the_parent_of_a_relative_path() {
        assert_eq!(parent_of("a/b/c.txt"), "a/b");
        assert_eq!(parent_of("c.txt"), "");
    }
}
