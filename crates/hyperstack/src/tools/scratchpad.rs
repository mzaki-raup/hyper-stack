//! Scratchpad: a collection of Markdown notes per workspace. Every note is a
//! file under `<workspace>/notes/<id>.md`; the sidebar lists them, one tab
//! shows one note (`note://<id>`) with the source on the left and the
//! rendered preview on the right, and edits are saved a moment after they
//! stop (Ctrl+S saves at once).

use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use hs_core::Services;
use hs_core::tool::{
    Accept, CommandDescriptor, OpenRequest, Printable, QuickOpenItem, Speakable, ToastLevel, Tool,
    ToolCategory, ToolContext, ToolDescriptor, ToolFactory,
};
use hs_ui::form;
use hs_ui::icons::ph;
use hs_ui::theme::Theme;
use hs_ui::tip::TipExt;
use serde::{Deserialize, Serialize};

pub static DESCRIPTOR: ToolDescriptor = ToolDescriptor {
    id: "scratchpad",
    title_key: "tool-scratchpad",
    icon: hs_ui::icons::NOTE,
    category: ToolCategory::Utilities,
    accepts: &[Accept::Text, Accept::LocalFile],
};

/// Idle time after the last keystroke before a note is written.
const AUTOSAVE_AFTER: Duration = Duration::from_millis(1500);
/// How often the notes folder is re-read while the sidebar is shown.
const RESCAN_EVERY: Duration = Duration::from_secs(10);

pub struct ScratchpadFactory;

impl ToolFactory for ScratchpadFactory {
    fn descriptor(&self) -> &ToolDescriptor {
        &DESCRIPTOR
    }
    fn create(&self, services: &Services, restore: Option<serde_json::Value>) -> Box<dyn Tool> {
        let mut state: State = restore
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default();
        // A saved note is read back from its file - wherever it now sits -
        // and the layout only carries the draft of a note never saved.
        let mut path = None;
        if let Some(id) = state.id.clone() {
            match find_note(&notes_dir(services), &id)
                .and_then(|p| std::fs::read_to_string(&p).ok().map(|t| (p, t)))
            {
                Some((p, text)) => {
                    state.text = text;
                    path = Some(p);
                }
                None => state.id = None,
            }
        }
        Box::new(Scratchpad {
            saved: state.text.clone(),
            state,
            created: None,
            modified: None,
            stamped: None,
            home_title: None,
            notes: Vec::new(),
            pins: Vec::new(),
            order: Vec::new(),
            folders: Vec::new(),
            orders: Default::default(),
            collapsed: Default::default(),
            path,
            untitled: services.t("scratchpad-untitled"),
            tool_name: services.t("tool-scratchpad"),
            renaming: None,
            rename_focus: false,
            confirm_delete_folder: None,
            assistant: hs_ui::ai_assistant::AssistantPanel::default(),
            insert_pending: None,
            scanned: None,
            filter: String::new(),
            last_edit: None,
            confirm_delete: None,
        })
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
enum View {
    Edit,
    #[default]
    Split,
    Preview,
}

#[derive(Default, Serialize, Deserialize, Clone)]
#[serde(default)]
struct State {
    /// The note's file id; `None` for a note that was never saved.
    id: Option<String>,
    text: String,
    view: View,
    environment: Option<String>,
}

/// A note as listed in the sidebar.
#[derive(Clone)]
struct NoteMeta {
    id: String,
    title: String,
    modified: SystemTime,
    pinned: bool,
    /// The folder the note sits in, relative to `notes/` with `/` between
    /// levels; empty at the top level.
    folder: String,
}

pub struct Scratchpad {
    state: State,
    /// When this note's file was created and last written, for the
    /// header band. Re-read on a timer: `header()` runs every frame and
    /// has no business touching the disk that often.
    created: Option<std::time::SystemTime>,
    modified: Option<std::time::SystemTime>,
    stamped: Option<(String, std::time::Instant)>,
    /// Set when this tab is the activity's home tab: its name, which is the
    /// activity's rather than a note's. Owner's report, 2026-10-17.
    home_title: Option<String>,
    saved: String,
    notes: Vec<NoteMeta>,
    pins: Vec<String>,
    /// Saved order of unpinned notes; ids missing here sort newest first.
    order: Vec<String>,
    /// Every folder under `notes/`, as paths relative to it, sorted.
    folders: Vec<String>,
    /// Each level's saved order, by folder path (empty for the top level),
    /// read on every rescan rather than on every frame.
    orders: std::collections::HashMap<String, Vec<String>>,
    /// Folders the user collapsed (`notes/collapsed.json`).
    collapsed: std::collections::HashSet<String>,
    /// Where this tab's note was last found. Only a hint for the header's
    /// timestamps: a save looks the note up again, because another tab may
    /// have moved it into a folder in the meantime.
    path: Option<PathBuf>,
    /// "Untitled" and the activity's name in the user's language, kept for
    /// `quick_open_items`, which is handed no services.
    untitled: String,
    tool_name: String,
    /// The folder being renamed and the name typed so far.
    renaming: Option<(String, String)>,
    /// Put the cursor in the rename field on the next frame, once.
    rename_focus: bool,
    confirm_delete_folder: Option<String>,
    assistant: hs_ui::ai_assistant::AssistantPanel,
    /// Text to put into the note at the cursor on the next frame.
    insert_pending: Option<String>,
    scanned: Option<Instant>,
    filter: String,
    last_edit: Option<Instant>,
    confirm_delete: Option<String>,
}

fn notes_dir(s: &Services) -> PathBuf {
    s.workspace.read().root().join("notes")
}

fn note_path(dir: &Path, id: &str) -> PathBuf {
    dir.join(format!("{id}.md"))
}

/// The first heading, or the first non-empty line, trimmed to 60 chars.
fn title_of(text: &str) -> Option<String> {
    text.lines()
        .map(|l| l.trim().trim_start_matches('#').trim())
        .find(|l| !l.is_empty())
        .map(|l| l.chars().take(60).collect())
}

/// Read the notes folder and every folder under it: id, title (from the
/// first lines), mtime and folder, newest first. Only the first 4 KB of
/// each file is read for the title.
fn list_notes(dir: &Path) -> Vec<NoteMeta> {
    let mut out = Vec::new();
    collect_notes(dir, "", &mut out);
    out.sort_by_key(|a| std::cmp::Reverse(a.modified));
    out
}

fn collect_notes(dir: &Path, folder: &str, out: &mut Vec<NoteMeta>) {
    use std::io::Read;
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let path = e.path();
        if path.is_dir() {
            if let Some(name) = folder_of_dir(&path) {
                collect_notes(&path, &join_folder(folder, &name), out);
            }
            continue;
        }
        if path.extension().and_then(|x| x.to_str()) != Some("md") {
            continue;
        }
        let Some(id) = path.file_stem().and_then(|x| x.to_str()) else {
            continue;
        };
        let modified = e
            .metadata()
            .and_then(|m| m.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        let mut head = Vec::with_capacity(4096);
        if let Ok(f) = std::fs::File::open(&path) {
            let _ = f.take(4096).read_to_end(&mut head);
        }
        let title = title_of(&String::from_utf8_lossy(&head)).unwrap_or_default();
        out.push(NoteMeta {
            id: id.to_string(),
            title,
            modified,
            pinned: false,
            folder: folder.to_string(),
        });
    }
}

/// Every folder under `dir`, as paths relative to it, sorted.
fn list_folders(dir: &Path) -> Vec<String> {
    fn walk(dir: &Path, folder: &str, out: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for e in entries.flatten() {
            let path = e.path();
            if path.is_dir()
                && let Some(name) = folder_of_dir(&path)
            {
                let here = join_folder(folder, &name);
                walk(&path, &here, out);
                out.push(here);
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, "", &mut out);
    out.sort_by_key(|f| f.to_lowercase());
    out
}

/// A directory's name, when it is a folder of notes. A dot-directory is the
/// app's or someone else's housekeeping, not a folder the user made.
fn folder_of_dir(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_str()?;
    (!name.starts_with('.')).then(|| name.to_string())
}

/// `parent/name`, or just `name` at the top level.
fn join_folder(parent: &str, name: &str) -> String {
    if parent.is_empty() {
        name.to_string()
    } else {
        format!("{parent}/{name}")
    }
}

/// The folder a folder sits in; empty for one at the top level.
fn parent_of(folder: &str) -> &str {
    folder.rsplit_once('/').map(|(p, _)| p).unwrap_or("")
}

/// The last part of a folder path, which is what the sidebar shows.
fn leaf_of(folder: &str) -> &str {
    folder.rsplit_once('/').map(|(_, n)| n).unwrap_or(folder)
}

/// Whether `inner` is `outer` or anywhere inside it. Compared without
/// regard to case, as the file system compares them.
fn is_within(outer: &str, inner: &str) -> bool {
    let (o, i) = (outer.to_lowercase(), inner.to_lowercase());
    i == o || i.starts_with(&format!("{o}/"))
}

/// The directory a folder path names under `root`.
fn folder_dir(root: &Path, folder: &str) -> PathBuf {
    folder
        .split('/')
        .filter(|p| !p.is_empty())
        .fold(root.to_path_buf(), |d, p| d.join(p))
}

/// Where note `id` lives now: at the top level, or in whichever folder it
/// has been moved to. Ids are unique across the whole tree.
fn find_note(root: &Path, id: &str) -> Option<PathBuf> {
    let top = note_path(root, id);
    if top.is_file() {
        return Some(top);
    }
    list_folders(root)
        .iter()
        .map(|f| note_path(&folder_dir(root, f), id))
        .find(|p| p.is_file())
}

/// A name a folder can have on every platform: no separators or other
/// characters a file name refuses, not `.` or `..`, and no trailing dot or
/// space - Windows strips those, so `Work.` would land on top of `Work`.
fn folder_name(wanted: &str) -> Option<String> {
    // Checked first: `safe_name` turns an empty name into "download".
    if wanted.trim().is_empty() {
        return None;
    }
    let cleaned = hs_core::download::safe_name(wanted);
    // Trimming trailing dots also disposes of `.` and `..`.
    let cleaned = cleaned.trim_end_matches(['.', ' ']).trim().to_string();
    (!cleaned.is_empty()).then_some(cleaned)
}

/// Saved order of the unpinned notes, in `notes/order.json`.
fn load_order(dir: &Path) -> Vec<String> {
    std::fs::read_to_string(dir.join("order.json"))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn save_order(dir: &Path, order: &[String]) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    std::fs::write(
        dir.join("order.json"),
        serde_json::to_string_pretty(order).unwrap_or_default(),
    )
}

/// Ids of the notes pinned to the top, in `notes/pins.json`.
fn load_pins(dir: &Path) -> Vec<String> {
    std::fs::read_to_string(dir.join("pins.json"))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn save_pins(dir: &Path, pins: &[String]) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    std::fs::write(
        dir.join("pins.json"),
        serde_json::to_string_pretty(pins).unwrap_or_default(),
    )
}

/// Pinned notes first (in pin order), then the rest newest first.
fn order_notes(mut notes: Vec<NoteMeta>, pins: &[String], order: &[String]) -> Vec<NoteMeta> {
    for n in &mut notes {
        n.pinned = pins.contains(&n.id);
    }
    let rank = |n: &NoteMeta| pins.iter().position(|p| *p == n.id).unwrap_or(usize::MAX);
    notes.sort_by(|a, b| match (a.pinned, b.pinned) {
        (true, true) => rank(a).cmp(&rank(b)),
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        (false, false) => {
            let rank = |n: &NoteMeta| {
                order
                    .iter()
                    .position(|id| id == &n.id)
                    .unwrap_or(usize::MAX)
            };
            match (rank(a) == usize::MAX, rank(b) == usize::MAX) {
                (true, true) => b.modified.cmp(&a.modified),
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => rank(a).cmp(&rank(b)),
            }
        }
    });
    notes
}

/// One row of a level of the tree.
#[derive(Clone)]
enum Entry {
    Folder(String),
    Note(NoteMeta),
}

impl Entry {
    /// How the row is named in its level's `order.json`: a note by its id,
    /// a folder by `/` and its name. A slash can appear in neither a
    /// folder's name nor a note's id, so the two can never be confused, and
    /// an `order.json` written before folders existed - ids only - still
    /// reads correctly.
    fn key(&self) -> String {
        match self {
            Entry::Folder(f) => format!("/{}", leaf_of(f)),
            Entry::Note(n) => n.id.clone(),
        }
    }
}

/// The rows of one level: its folders and its unpinned notes, in the
/// level's saved order. Rows the order does not mention come first -
/// folders by name, then notes newest first - which is where a new note
/// has always appeared. Pinned notes are not in any level: they sit in
/// their own group at the top.
fn level_entries(
    folder: &str,
    notes: &[NoteMeta],
    folders: &[String],
    order: &[String],
) -> Vec<Entry> {
    use std::cmp::Ordering;
    let mut entries: Vec<Entry> = folders
        .iter()
        .filter(|f| parent_of(f) == folder)
        .map(|f| Entry::Folder(f.clone()))
        .chain(
            notes
                .iter()
                .filter(|n| !n.pinned && n.folder == folder)
                .map(|n| Entry::Note(n.clone())),
        )
        .collect();
    let rank = |e: &Entry| order.iter().position(|k| *k == e.key());
    entries.sort_by(|a, b| match (rank(a), rank(b)) {
        (Some(x), Some(y)) => x.cmp(&y),
        (None, Some(_)) => Ordering::Less,
        (Some(_), None) => Ordering::Greater,
        (None, None) => match (a, b) {
            (Entry::Folder(x), Entry::Folder(y)) => x.to_lowercase().cmp(&y.to_lowercase()),
            (Entry::Folder(_), Entry::Note(_)) => Ordering::Less,
            (Entry::Note(_), Entry::Folder(_)) => Ordering::Greater,
            (Entry::Note(x), Entry::Note(y)) => y.modified.cmp(&x.modified),
        },
    });
    entries
}

/// Folders the user collapsed, in `notes/collapsed.json`.
fn load_collapsed(dir: &Path) -> std::collections::HashSet<String> {
    std::fs::read_to_string(dir.join("collapsed.json"))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn save_collapsed(dir: &Path, set: &std::collections::HashSet<String>) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let mut list: Vec<&String> = set.iter().collect();
    list.sort();
    std::fs::write(
        dir.join("collapsed.json"),
        serde_json::to_string_pretty(&list).unwrap_or_default(),
    )
}

/// The names already used in a directory - folders and files alike, since
/// a folder cannot take a name a file in the same place already has.
fn names_in(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .map(|it| {
            it.flatten()
                .filter_map(|e| e.file_name().to_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn invalid(message: &str) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidInput, message.to_string())
}

/// Make a folder inside `parent`, called `wanted`, or `wanted 001` and so
/// on when that name is taken. Returns the new folder's path.
fn create_folder(root: &Path, parent: &str, wanted: &str) -> std::io::Result<String> {
    let name = folder_name(wanted).ok_or_else(|| invalid("a folder needs a name"))?;
    let here = folder_dir(root, parent);
    std::fs::create_dir_all(&here)?;
    let taken = names_in(&here);
    let name = hs_core::storage::unique_name(&name, taken.iter().map(String::as_str));
    std::fs::create_dir(here.join(&name))?;
    Ok(join_folder(parent, &name))
}

/// Rename a folder where it stands. Returns its new path; a name another
/// folder already has gets the usual ` 001` suffix.
fn rename_folder(root: &Path, folder: &str, wanted: &str) -> std::io::Result<String> {
    let name = folder_name(wanted).ok_or_else(|| invalid("a folder needs a name"))?;
    let own = leaf_of(folder);
    if name == own {
        return Ok(folder.to_string());
    }
    let parent = parent_of(folder);
    let here = folder_dir(root, parent);
    // The folder's own name is not a clash: renaming `work` to `Work` is a
    // change of case, not a collision with itself.
    let taken: Vec<String> = names_in(&here)
        .into_iter()
        .filter(|n| !n.eq_ignore_ascii_case(own))
        .collect();
    let name = hs_core::storage::unique_name(&name, taken.iter().map(String::as_str));
    std::fs::rename(folder_dir(root, folder), here.join(&name))?;
    Ok(join_folder(parent, &name))
}

/// Move a note into `folder` (empty for the top level). Notes are named by
/// id, so a move can never land on another note.
fn move_note(root: &Path, id: &str, folder: &str) -> std::io::Result<()> {
    let from = find_note(root, id).ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::NotFound, "the note is no longer there")
    })?;
    let to_dir = folder_dir(root, folder);
    std::fs::create_dir_all(&to_dir)?;
    let to = note_path(&to_dir, id);
    if from != to {
        std::fs::rename(&from, &to)?;
    }
    Ok(())
}

/// Move a folder, with everything in it, into `into` (empty for the top
/// level). Returns its new path. It keeps its name unless the destination
/// already has one, when it gets the usual ` 001` suffix.
fn move_folder(root: &Path, folder: &str, into: &str) -> std::io::Result<String> {
    if is_within(folder, into) {
        return Err(invalid("a folder cannot go inside itself"));
    }
    if parent_of(folder) == into {
        return Ok(folder.to_string());
    }
    let dest = folder_dir(root, into);
    std::fs::create_dir_all(&dest)?;
    let taken = names_in(&dest);
    let name = hs_core::storage::unique_name(leaf_of(folder), taken.iter().map(String::as_str));
    std::fs::rename(folder_dir(root, folder), dest.join(&name))?;
    Ok(join_folder(into, &name))
}

/// The same set with every path at or under `old` moved to `new`, after a
/// folder was renamed or moved, so what was collapsed stays collapsed.
fn remap_folders(
    set: &std::collections::HashSet<String>,
    old: &str,
    new: &str,
) -> std::collections::HashSet<String> {
    set.iter()
        .map(|f| {
            if is_within(old, f) {
                format!("{new}{}", &f[old.len()..])
            } else {
                f.clone()
            }
        })
        .collect()
}

fn when(t: SystemTime) -> String {
    let dt: chrono::DateTime<chrono::Local> = t.into();
    let now = chrono::Local::now();
    if dt.date_naive() == now.date_naive() {
        dt.format("%H:%M").to_string()
    } else {
        dt.format("%Y-%m-%d").to_string()
    }
}

/// Something the sidebar asked for while it was being drawn. Collected and
/// carried out afterwards, because the tree is drawn by a recursive function
/// that only reads the tool.
enum SideAct {
    Open(String),
    TogglePin(String),
    ConfirmNote(String),
    ConfirmFolder(String),
    /// Flip whether a folder is collapsed.
    Collapse(String),
    /// Make a folder inside this one (empty for the top level).
    NewFolder(String),
    Rename(String),
    /// Row `index` of level `from` was dropped at boundary `to` of `into`.
    Drop {
        from: String,
        index: usize,
        into: String,
        to: usize,
    },
}

/// What drawing the tree needs to read.
struct Tree<'a> {
    notes: &'a [NoteMeta],
    folders: &'a [String],
    orders: &'a std::collections::HashMap<String, Vec<String>>,
    collapsed: &'a std::collections::HashSet<String>,
    selected: Option<&'a str>,
    untitled: &'a str,
    theme: &'a Theme,
    s: &'a Services,
}

impl Tree<'_> {
    fn title_of<'n>(&'n self, n: &'n NoteMeta) -> &'n str {
        if n.title.is_empty() {
            self.untitled
        } else {
            &n.title
        }
    }
}

/// Where a note is, for a row shown outside its folder (a pinned note, a
/// search hit): the folder's path, or the time for a note at the top level.
fn note_place(n: &NoteMeta) -> String {
    if n.folder.is_empty() {
        when(n.modified)
    } else {
        n.folder.replace('/', " / ")
    }
}

/// A note's right-click menu and click, the same wherever its row is drawn.
fn note_actions(r: &egui::Response, t: &Tree, n: &NoteMeta, acts: &mut Vec<SideAct>) {
    if r.clicked() && t.selected != Some(n.id.as_str()) {
        acts.push(SideAct::Open(n.id.clone()));
    }
    r.context_menu(|ui| {
        let (icon, key) = if n.pinned {
            (ph::PUSH_PIN_SLASH, "scratchpad-unpin")
        } else {
            (ph::PUSH_PIN, "scratchpad-pin")
        };
        if ui.button(format!("{icon} {}", t.s.t(key))).clicked() {
            acts.push(SideAct::TogglePin(n.id.clone()));
            ui.close();
        }
        if ui
            .button(format!("{} {}", hs_ui::icons::TRASH, t.s.t("delete")))
            .clicked()
        {
            acts.push(SideAct::ConfirmNote(n.id.clone()));
            ui.close();
        }
    });
}

/// A note row with no grip: a pinned note, or a search hit. The slot a grip
/// takes on a draggable row is kept, so every row lines up, and a pinned
/// note shows its pin there - a state, not a replacement for the icon.
fn plain_note_row(ui: &mut egui::Ui, t: &Tree, n: &NoteMeta, acts: &mut Vec<SideAct>) {
    let selected = t.selected == Some(n.id.as_str());
    let place = note_place(n);
    let r = ui
        .horizontal(|ui| {
            let (slot, _) = ui.allocate_exact_size(
                egui::vec2(hs_ui::reorder::GRIP_WIDTH, 22.0),
                egui::Sense::hover(),
            );
            let r = form::list_row(ui, t.theme, selected, ph::NOTE, t.title_of(n), Some(&place));
            if n.pinned {
                let body = ui.min_rect();
                ui.painter().text(
                    egui::pos2(slot.center().x, body.center().y),
                    egui::Align2::CENTER_CENTER,
                    ph::PUSH_PIN,
                    egui::FontId::proportional(11.0),
                    t.theme.text_muted,
                );
            }
            r
        })
        .inner;
    note_actions(&r, t, n, acts);
}

/// One level of the tree: its folders and unpinned notes in their saved
/// order, each draggable. A folder is a collapsing header with its own level
/// inside; a row can be dropped between two others - in this level or
/// another - or onto a folder's header to go into it. The same shape as the
/// Browser's favourites (`tool-browser::side`), so the gesture is one the
/// app already teaches.
fn draw_level(ui: &mut egui::Ui, t: &Tree, folder: &str, acts: &mut Vec<SideAct>) {
    use hs_ui::reorder::{Move, Reorder};
    let order = t.orders.get(folder).map(Vec::as_slice).unwrap_or(&[]);
    let entries = level_entries(folder, t.notes, t.folders, order);
    let mut reorder = Reorder::scoped(("scratchpad-level", folder.to_string()), folder);
    for (index, entry) in entries.iter().enumerate() {
        match entry {
            Entry::Folder(f) => {
                let r = reorder.row(ui, index, |ui| {
                    egui::CollapsingHeader::new(format!("{} {}", ph::FOLDER, leaf_of(f)))
                        .id_salt(("scratchpad-folder", f))
                        .default_open(!t.collapsed.contains(f))
                        .show(ui, |ui| draw_level(ui, t, f, acts))
                });
                if r.header_response.clicked() {
                    acts.push(SideAct::Collapse(f.clone()));
                }
                // A folder takes a drop of its own, so a collapsed or empty
                // one can still be where a note goes.
                let rect = r.header_response.rect;
                if hs_ui::reorder::dragged_scope(ui.ctx()).is_some() {
                    if ui
                        .input(|i| i.pointer.interact_pos())
                        .is_some_and(|p| rect.contains(p))
                    {
                        reorder.suppress_target();
                    }
                    if let Some(d) = hs_ui::reorder::drop_onto(ui, rect, t.theme.accent) {
                        acts.push(SideAct::Drop {
                            from: d.scope,
                            index: d.index,
                            into: f.clone(),
                            to: usize::MAX,
                        });
                    }
                }
                r.header_response.context_menu(|ui| {
                    if ui
                        .button(format!(
                            "{} {}",
                            ph::FOLDER_PLUS,
                            t.s.t("scratchpad-new-folder")
                        ))
                        .clicked()
                    {
                        acts.push(SideAct::NewFolder(f.clone()));
                        ui.close();
                    }
                    if ui
                        .button(format!(
                            "{} {}",
                            ph::PENCIL_SIMPLE,
                            t.s.t("scratchpad-rename")
                        ))
                        .clicked()
                    {
                        acts.push(SideAct::Rename(f.clone()));
                        ui.close();
                    }
                    ui.separator();
                    if ui
                        .button(format!("{} {}", hs_ui::icons::TRASH, t.s.t("delete")))
                        .clicked()
                    {
                        acts.push(SideAct::ConfirmFolder(f.clone()));
                        ui.close();
                    }
                });
            }
            Entry::Note(n) => {
                let selected = t.selected == Some(n.id.as_str());
                let sub = when(n.modified);
                let r = reorder.row(ui, index, |ui| {
                    form::list_row(ui, t.theme, selected, ph::NOTE, t.title_of(n), Some(&sub))
                });
                note_actions(&r, t, n, acts);
            }
        }
    }
    if let Some(mv) = reorder.finish_move(ui, t.theme.accent) {
        let (from, index, to) = match mv {
            Move::Within { from, to } => (folder.to_string(), from, to),
            Move::From { scope, from, to } => (scope, from, to),
        };
        acts.push(SideAct::Drop {
            from,
            index,
            into: folder.to_string(),
            to,
        });
    }
}

impl Scratchpad {
    /// The home tab: what this activity has open, with the last picture of
    /// each pane. Owner's requirement, 2026-10-17.
    /// The activity's home page: a band, the figures, and then one
    /// section per thing a reader might want to do here - start a note,
    /// open a saved one, or go back to a note already open (owner's
    /// requirement, 2026-10-27).
    fn dashboard(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        let s = cx.services;
        let theme = hs_ui::theme::Theme::from_settings(&s.settings.read().appearance);
        // The dashboard is the one surface that shows the whole list, so
        // it has to be looking at a current one.
        if self.scanned.is_none_or(|t| t.elapsed() > RESCAN_EVERY) {
            self.rescan(s);
        }
        let notes = self.notes.clone();
        let tabs = s.open_tabs.of("scratchpad");
        let labels = hs_ui::dashboard::DashboardLabels {
            open_heading: &s.t("dash-open"),
            nothing_open: &s.t("dash-nothing-open"),
            no_preview: &s.t("dash-no-preview"),
        };
        let untitled = s.t("scratchpad-untitled");
        let mut new_note = false;
        let mut open: Option<String> = None;
        let mut focus: Option<u64> = None;
        egui::ScrollArea::vertical()
            .id_salt("scratchpad-dashboard")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                hs_ui::dashboard::head(
                    ui,
                    &theme,
                    ph::NOTE_PENCIL,
                    &s.t("tool-scratchpad"),
                    &s.t("scratchpad-dashboard-sub"),
                );
                hs_ui::dashboard::rule(ui, &theme);

                hs_ui::dashboard::stats(
                    ui,
                    &theme,
                    &[
                        hs_ui::dashboard::Stat {
                            icon: ph::NOTE,
                            value: &notes.len().to_string(),
                            label: &s.t("dash-notes"),
                        },
                        hs_ui::dashboard::Stat {
                            icon: ph::COLUMNS,
                            value: &tabs.len().to_string(),
                            label: &s.t("dash-open-tabs"),
                        },
                    ],
                );
                hs_ui::dashboard::rule(ui, &theme);

                hs_ui::dashboard::section(ui, &theme, &s.t("dash-start-here"));
                ui.horizontal(|ui| {
                    ui.add_space(8.0);
                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new(format!(
                                    "{} {}",
                                    ph::PLUS,
                                    s.t("scratchpad-new")
                                ))
                                .size(14.0),
                            )
                            .min_size(egui::vec2(170.0, 32.0))
                            .fill(theme.accent),
                        )
                        .tip(s.t("scratchpad-new-tip"))
                        .clicked()
                    {
                        new_note = true;
                    }
                });
                hs_ui::dashboard::rule(ui, &theme);

                hs_ui::dashboard::section(ui, &theme, &s.t("dash-saved-notes"));
                if notes.is_empty() {
                    ui.horizontal(|ui| {
                        ui.add_space(8.0);
                        form::hint(ui, &theme, &s.t("scratchpad-empty"));
                    });
                } else {
                    // A grid of equal buttons rather than a column of
                    // rows: the names are short and a single column
                    // would leave most of a wide pane empty.
                    const BUTTON: f32 = 230.0;
                    let spacing = ui.spacing().item_spacing.x;
                    let avail = (ui.available_width() - 32.0).max(BUTTON);
                    let columns =
                        (((avail + spacing) / (BUTTON + spacing)).floor() as usize).max(1);
                    ui.horizontal(|ui| {
                        ui.add_space(8.0);
                        egui::Grid::new("scratchpad-saved-notes")
                            .num_columns(columns)
                            .spacing([spacing, spacing])
                            .show(ui, |ui| {
                                for (i, n) in notes.iter().enumerate() {
                                    let title = if n.title.is_empty() {
                                        untitled.as_str()
                                    } else {
                                        n.title.as_str()
                                    };
                                    let icon = if n.pinned { ph::PUSH_PIN } else { ph::NOTE };
                                    if ui
                                        .add(
                                            egui::Button::new(format!("{icon}  {title}"))
                                                .min_size(egui::vec2(BUTTON, 30.0))
                                                .wrap_mode(egui::TextWrapMode::Truncate),
                                        )
                                        .tip(when(n.modified))
                                        .clicked()
                                    {
                                        open = Some(n.id.clone());
                                    }
                                    if (i + 1) % columns == 0 {
                                        ui.end_row();
                                    }
                                }
                            });
                    });
                }
                hs_ui::dashboard::rule(ui, &theme);

                if let Some(id) = hs_ui::dashboard::open_tabs_section(ui, &theme, &tabs, &labels) {
                    focus = Some(id);
                }
                ui.add_space(8.0);
            });
        if new_note {
            cx.open_new_tab("scratchpad", None);
        }
        if let Some(id) = open {
            cx.open(
                "scratchpad",
                Some(OpenRequest::File {
                    uri: format!("note://{id}"),
                    line: None,
                }),
            );
        }
        if let Some(tab_id) = focus {
            cx.actions.push(hs_core::tool::Action::FocusTab { tab_id });
        }
    }

    fn rescan(&mut self, s: &Services) {
        let dir = notes_dir(s);
        self.pins = load_pins(&dir);
        self.order = load_order(&dir);
        self.folders = list_folders(&dir);
        self.orders = std::iter::once(String::new())
            .chain(self.folders.iter().cloned())
            .map(|f| {
                let order = load_order(&folder_dir(&dir, &f));
                (f, order)
            })
            .collect();
        self.collapsed = load_collapsed(&dir);
        self.notes = order_notes(list_notes(&dir), &self.pins, &self.order);
        self.untitled = s.t("scratchpad-untitled");
        self.tool_name = s.t("tool-scratchpad");
        self.scanned = Some(Instant::now());
    }

    /// The keys of level `folder` in the order it is drawn.
    fn level_keys(&self, folder: &str) -> Vec<String> {
        let order = self.orders.get(folder).map(Vec::as_slice).unwrap_or(&[]);
        level_entries(folder, &self.notes, &self.folders, order)
            .iter()
            .map(Entry::key)
            .collect()
    }

    /// Row `index` of level `folder`, as the drag that started on it saw it.
    fn entry_at(&self, folder: &str, index: usize) -> Option<Entry> {
        let order = self.orders.get(folder).map(Vec::as_slice).unwrap_or(&[]);
        level_entries(folder, &self.notes, &self.folders, order)
            .into_iter()
            .nth(index)
    }

    /// A row was dropped: row `index` of level `from`, at boundary `to` of
    /// level `into` (`usize::MAX` for "onto the folder", meaning its end).
    /// Within one level that is a reorder; across levels it moves the note
    /// or the folder on disk and writes both levels' orders.
    fn drop_entry(
        &mut self,
        from: &str,
        index: usize,
        into: &str,
        to: usize,
        cx: &mut ToolContext,
    ) {
        let root = notes_dir(cx.services);
        let Some(entry) = self.entry_at(from, index) else {
            return;
        };
        let old_key = entry.key();
        let result = if from == into {
            let mut keys = self.level_keys(into);
            let to = to.min(keys.len());
            hs_ui::reorder::move_item(&mut keys, index, to);
            save_order(&folder_dir(&root, into), &keys)
        } else {
            let moved = match &entry {
                Entry::Note(n) => move_note(&root, &n.id, into).map(|()| old_key.clone()),
                Entry::Folder(f) => {
                    // Onto itself or into its own subfolder: a gesture with
                    // nowhere to go, not a mistake worth a toast.
                    if is_within(f, into) {
                        return;
                    }
                    move_folder(&root, f, into).map(|new| {
                        self.collapsed = remap_folders(&self.collapsed, f, &new);
                        let _ = save_collapsed(&root, &self.collapsed);
                        format!("/{}", leaf_of(&new))
                    })
                }
            };
            moved.and_then(|new_key| {
                self.forget(&root, from, &old_key);
                let mut keys = self.level_keys(into);
                keys.retain(|k| *k != new_key);
                keys.insert(to.min(keys.len()), new_key);
                save_order(&folder_dir(&root, into), &keys)
            })
        };
        if let Err(e) = result {
            cx.toast(ToastLevel::Error, e.to_string());
        }
        self.rescan(cx.services);
    }

    /// Take `key` out of level `folder`'s saved order, after its row left.
    fn forget(&self, root: &Path, folder: &str, key: &str) {
        let mut order = self.orders.get(folder).cloned().unwrap_or_default();
        let before = order.len();
        order.retain(|k| k != key);
        if order.len() != before {
            let _ = save_order(&folder_dir(root, folder), &order);
        }
    }

    /// Make a folder inside `parent` and put its name up for editing.
    fn new_folder(&mut self, parent: &str, cx: &mut ToolContext) {
        let root = notes_dir(cx.services);
        match create_folder(&root, parent, &cx.services.t("scratchpad-new-folder")) {
            Ok(path) => {
                // The parent opens, so the new folder can be seen.
                if self.collapsed.remove(parent) {
                    let _ = save_collapsed(&root, &self.collapsed);
                }
                self.renaming = Some((path.clone(), leaf_of(&path).to_string()));
                self.rename_focus = true;
                self.rescan(cx.services);
            }
            Err(e) => cx.toast(ToastLevel::Error, e.to_string()),
        }
    }

    fn rename(&mut self, folder: &str, wanted: &str, cx: &mut ToolContext) {
        let root = notes_dir(cx.services);
        match rename_folder(&root, folder, wanted) {
            Ok(new) => {
                if new != folder {
                    self.collapsed = remap_folders(&self.collapsed, folder, &new);
                    let _ = save_collapsed(&root, &self.collapsed);
                    // The folder keeps its place in its parent's order.
                    let parent = parent_of(folder).to_string();
                    let (old_key, new_key) = (
                        format!("/{}", leaf_of(folder)),
                        format!("/{}", leaf_of(&new)),
                    );
                    let mut order = self.orders.get(&parent).cloned().unwrap_or_default();
                    if let Some(k) = order.iter_mut().find(|k| **k == old_key) {
                        *k = new_key;
                        let _ = save_order(&folder_dir(&root, &parent), &order);
                    }
                }
                self.rescan(cx.services);
            }
            Err(e) => cx.toast(ToastLevel::Error, e.to_string()),
        }
    }

    /// Carry out what the sidebar asked for while it was being drawn.
    fn apply(&mut self, act: SideAct, cx: &mut ToolContext) {
        match act {
            SideAct::Open(id) => cx.open(
                "scratchpad",
                Some(OpenRequest::File {
                    uri: format!("note://{id}"),
                    line: None,
                }),
            ),
            SideAct::TogglePin(id) => self.toggle_pin(&id, cx),
            SideAct::ConfirmNote(id) => self.confirm_delete = Some(id),
            SideAct::ConfirmFolder(folder) => self.confirm_delete_folder = Some(folder),
            SideAct::Collapse(folder) => {
                if !self.collapsed.remove(&folder) {
                    self.collapsed.insert(folder);
                }
                if let Err(e) = save_collapsed(&notes_dir(cx.services), &self.collapsed) {
                    cx.toast(ToastLevel::Warning, e.to_string());
                }
            }
            SideAct::NewFolder(parent) => self.new_folder(&parent, cx),
            SideAct::Rename(folder) => {
                self.renaming = Some((folder.clone(), leaf_of(&folder).to_string()));
                self.rename_focus = true;
            }
            SideAct::Drop {
                from,
                index,
                into,
                to,
            } => self.drop_entry(&from, index, &into, to, cx),
        }
    }

    /// Delete a folder and every note in it, once the user confirmed.
    fn delete_folder(&mut self, folder: &str, cx: &mut ToolContext) {
        let root = notes_dir(cx.services);
        let inside: Vec<String> = self
            .notes
            .iter()
            .filter(|n| is_within(folder, &n.folder))
            .map(|n| n.id.clone())
            .collect();
        if let Err(e) = std::fs::remove_dir_all(folder_dir(&root, folder)) {
            cx.toast(ToastLevel::Error, e.to_string());
            return;
        }
        // A pin on a note that no longer exists would only take up a slot.
        let pins = self.pins.len();
        self.pins.retain(|p| !inside.contains(p));
        if self.pins.len() != pins {
            let _ = save_pins(&root, &self.pins);
        }
        self.forget(&root, parent_of(folder), &format!("/{}", leaf_of(folder)));
        self.collapsed.retain(|c| !is_within(folder, c));
        let _ = save_collapsed(&root, &self.collapsed);
        if self.state.id.as_ref().is_some_and(|id| inside.contains(id)) {
            self.state.id = None;
            self.state.text.clear();
            self.saved.clear();
            self.path = None;
        }
        self.rescan(cx.services);
        cx.toast(ToastLevel::Info, cx.services.t("scratchpad-folder-deleted"));
    }

    /// Pin a note to the top of the list, or unpin it.
    fn toggle_pin(&mut self, id: &str, cx: &mut ToolContext) {
        tracing::debug!(id, pins = ?self.pins, "scratchpad: toggle pin");
        match self.pins.iter().position(|p| p == id) {
            Some(i) => {
                self.pins.remove(i);
            }
            None => self.pins.push(id.to_string()),
        }
        if let Err(e) = save_pins(&notes_dir(cx.services), &self.pins) {
            cx.toast(ToastLevel::Error, e.to_string());
        }
        self.rescan(cx.services);
    }

    /// Write the note (giving it an id on the first save).
    fn save(&mut self, cx: &mut ToolContext) -> bool {
        let dir = notes_dir(cx.services);
        if let Err(e) = std::fs::create_dir_all(&dir) {
            cx.toast(ToastLevel::Error, e.to_string());
            return false;
        }
        let id = self.state.id.get_or_insert_with(hs_core::new_id).clone();
        // Wherever the note now is - another tab may have moved it into a
        // folder - and at the top level for one that was never written.
        // Writing to a remembered path would leave a stale copy behind.
        let path = find_note(&dir, &id).unwrap_or_else(|| note_path(&dir, &id));
        match std::fs::write(&path, &self.state.text) {
            Ok(()) => {
                self.path = Some(path);
                self.saved = self.state.text.clone();
                self.last_edit = None;
                self.rescan(cx.services);
                true
            }
            Err(e) => {
                cx.toast(ToastLevel::Error, e.to_string());
                false
            }
        }
    }

    fn delete(&mut self, id: &str, cx: &mut ToolContext) {
        let dir = notes_dir(cx.services);
        let path = find_note(&dir, id).unwrap_or_else(|| note_path(&dir, id));
        if let Err(e) = std::fs::remove_file(&path) {
            cx.toast(ToastLevel::Error, e.to_string());
            return;
        }
        if self.state.id.as_deref() == Some(id) {
            self.state.id = None;
            self.state.text.clear();
            self.saved.clear();
        }
        if self.pins.iter().any(|p| p == id) {
            self.pins.retain(|p| p != id);
            let _ = save_pins(&notes_dir(cx.services), &self.pins);
        }
        self.rescan(cx.services);
        cx.toast(ToastLevel::Info, cx.services.t("scratchpad-deleted"));
    }

    fn editor(&mut self, ui: &mut egui::Ui) {
        let id = ui.id().with("scratchpad-text");
        if let Some(text) = self.insert_pending.take() {
            // At the cursor when the editor has one, else at the end, as
            // its own paragraph.
            let at = egui::TextEdit::load_state(ui.ctx(), id)
                .and_then(|st| st.cursor.char_range())
                .map(|r| r.primary.index.0)
                .unwrap_or(self.state.text.chars().count());
            let byte = self
                .state
                .text
                .char_indices()
                .nth(at)
                .map(|(b, _)| b)
                .unwrap_or(self.state.text.len());
            let before = &self.state.text[..byte];
            let mut piece = String::new();
            if !before.is_empty() && !before.ends_with("\n\n") {
                piece.push_str(if before.ends_with('\n') { "\n" } else { "\n\n" });
            }
            piece.push_str(text.trim_end());
            piece.push('\n');
            self.state.text.insert_str(byte, &piece);
            self.last_edit = Some(Instant::now());
        }
        egui::ScrollArea::vertical()
            .id_salt("scratchpad-edit")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let r = ui.add_sized(
                    [ui.available_width(), ui.available_height().max(200.0)],
                    egui::TextEdit::multiline(&mut self.state.text)
                        .id(id)
                        .code_editor()
                        .frame(egui::Frame::NONE),
                );
                if r.changed() {
                    self.last_edit = Some(Instant::now());
                }
            });
    }

    /// The rendered note. `{{name}}` is resolved through the active
    /// environment and the globals before rendering - what a reader
    /// sees is the value, not the placeholder - and an unknown name is
    /// left as written rather than refused. A link the reader clicks
    /// is returned, to be opened as a page in the Browser activity
    /// (owner's requirement, 2026-10-29).
    fn preview(&self, ui: &mut egui::Ui, theme: &Theme, s: &Services) -> Option<String> {
        let style = hs_editor::markdown::MarkdownStyle {
            dark: theme.dark,
            text: theme.text,
            muted: theme.text_muted,
            link: theme.secondary,
            code_bg: theme.surface,
            quote_bar: theme.border,
            base_size: 14.0,
        };
        // Resolved once per frame; the note is small and the lookup is
        // a map walk, so this is cheaper than caching it against edits.
        let text = s
            .environments
            .read()
            .substitute_lenient(&self.state.text, None);
        egui::ScrollArea::vertical()
            .id_salt("scratchpad-preview")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                egui::Frame::new()
                    .inner_margin(egui::Margin::same(16))
                    .show(ui, |ui| {
                        if text.trim().is_empty() {
                            ui.label(egui::RichText::new("…").color(theme.text_muted));
                            None
                        } else {
                            hs_editor::markdown::render(ui, &text, &style).link_clicked
                        }
                    })
                    .inner
            })
            .inner
    }
}

impl Scratchpad {
    /// Re-read the note file's created and modified times for the header
    /// band - at most twice a second, and only when the note changed.
    ///
    /// `header()` is asked every frame and is handed no services, so the
    /// answer has to be sitting on the tool; statting the file sixty
    /// times a second to keep it there would be worse than the problem.
    fn refresh_stamps(&mut self, s: &Services) {
        let Some(id) = self.state.id.clone() else {
            self.created = None;
            self.modified = None;
            self.stamped = None;
            return;
        };
        let fresh = matches!(
            &self.stamped,
            Some((seen, at)) if *seen == id && at.elapsed() < std::time::Duration::from_millis(500)
        );
        if fresh {
            return;
        }
        // The remembered path first; only a note that has moved since is
        // looked for, which keeps this to one stat in the usual case.
        let path = self
            .path
            .clone()
            .filter(|p| p.is_file())
            .or_else(|| find_note(&notes_dir(s), &id));
        let meta = path.as_ref().and_then(|p| std::fs::metadata(p).ok());
        self.path = path;
        self.created = meta.as_ref().and_then(|m| m.created().ok());
        self.modified = meta.as_ref().and_then(|m| m.modified().ok());
        self.stamped = Some((id, std::time::Instant::now()));
    }
}

impl Tool for Scratchpad {
    fn wants_home_tab(&self) -> bool {
        true
    }

    fn home_is_dashboard(&self) -> bool {
        true
    }

    fn descriptor(&self) -> &ToolDescriptor {
        &DESCRIPTOR
    }
    fn set_home_tab(&mut self, home: bool, services: &Services) {
        self.home_title = home.then(|| services.t("tool-scratchpad"));
    }

    fn title(&self) -> Cow<'_, str> {
        // The home tab is the activity's; it holds no note.
        if let Some(name) = &self.home_title {
            return Cow::Borrowed(name);
        }
        match title_of(&self.state.text) {
            Some(t) => Cow::Owned(t),
            None => Cow::Borrowed("New Note"),
        }
    }
    /// Note name, when it was written and last changed, and whether
    /// what is on screen has been saved.
    fn header(&self) -> Option<hs_core::tool::ToolHeader> {
        // The dashboard is looking at the activity, not at a note.
        if self.home_title.is_some() {
            return None;
        }
        let status = if self.dirty() {
            "Draft"
        } else if self.state.id.is_some() {
            "Saved"
        } else {
            "Draft"
        };
        Some(
            hs_core::tool::ToolHeader::new(ph::NOTE_PENCIL, self.title().to_string())
                .fact("Date Created", hs_core::tool::stamp(self.created))
                .fact("Date Modified", hs_core::tool::stamp(self.modified))
                .fact("Note Status", status),
        )
    }

    fn dirty(&self) -> bool {
        self.state.text != self.saved
    }
    fn sidebar(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) -> bool {
        let s = cx.services;
        let theme = Theme::from_settings(&s.settings.read().appearance);
        if self.scanned.is_none_or(|t| t.elapsed() > RESCAN_EVERY) {
            self.rescan(s);
        }
        let mut new_folder = false;
        form::header_bar(ui, &theme, |ui| {
            ui.label(
                egui::RichText::new(s.t("scratchpad-notes"))
                    .strong()
                    .size(15.0),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .small_button(format!("{} {}", ph::PLUS, s.t("scratchpad-new")))
                    .tip(s.t("scratchpad-new-tip"))
                    .clicked()
                {
                    cx.open_new_tab("scratchpad", None);
                }
                // The icon alone, like the other sidebar headers' add
                // buttons: two labelled buttons do not fit beside the
                // title in a sidebar column.
                if ui
                    .small_button(ph::FOLDER_PLUS)
                    .tip(s.t("scratchpad-new-folder-tip"))
                    .clicked()
                {
                    new_folder = true;
                }
            });
        });
        if new_folder {
            self.new_folder("", cx);
        }
        ui.add(
            egui::TextEdit::singleline(&mut self.filter)
                .hint_text(s.t("scratchpad-search"))
                .desired_width(f32::INFINITY),
        );
        ui.add_space(4.0);
        if let Some(id) = self.confirm_delete.clone() {
            let title = self
                .notes
                .iter()
                .find(|n| n.id == id)
                .map(|n| n.title.clone())
                .filter(|t| !t.is_empty())
                .unwrap_or_else(|| s.t("scratchpad-untitled"));
            form::note(
                ui,
                &theme,
                hs_ui::icons::ph::WARNING,
                &s.i18n
                    .t_args("scratchpad-delete-confirm", &[("title", &title)]),
            );
            ui.horizontal(|ui| {
                if form::danger_button(ui, &theme, &s.t("delete")).clicked() {
                    self.confirm_delete = None;
                    self.delete(&id, cx);
                }
                if ui.button(s.t("cancel")).clicked() {
                    self.confirm_delete = None;
                }
            });
            ui.add_space(4.0);
        }
        if let Some(folder) = self.confirm_delete_folder.clone() {
            let count = self
                .notes
                .iter()
                .filter(|n| is_within(&folder, &n.folder))
                .count();
            form::note(
                ui,
                &theme,
                hs_ui::icons::ph::WARNING,
                &s.i18n.t_args(
                    "scratchpad-folder-delete-confirm",
                    &[("name", leaf_of(&folder)), ("count", &count.to_string())],
                ),
            );
            ui.horizontal(|ui| {
                if form::danger_button(ui, &theme, &s.t("delete")).clicked() {
                    self.confirm_delete_folder = None;
                    self.delete_folder(&folder, cx);
                }
                if ui.button(s.t("cancel")).clicked() {
                    self.confirm_delete_folder = None;
                }
            });
            ui.add_space(4.0);
        }
        // Renaming a folder gets a line of its own, like the confirmations
        // above: in place of the header's text it would be cramped in a
        // sidebar column.
        if let Some((folder, mut name)) = self.renaming.clone() {
            form::hint(ui, &theme, &s.t("scratchpad-rename-folder"));
            let r = ui.add(egui::TextEdit::singleline(&mut name).desired_width(f32::INFINITY));
            if std::mem::take(&mut self.rename_focus) {
                r.request_focus();
            }
            let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let escape = ui.input(|i| i.key_pressed(egui::Key::Escape));
            let mut done = escape;
            ui.horizontal(|ui| {
                if form::primary_button(ui, &theme, &s.t("scratchpad-rename")).clicked() || enter {
                    self.rename(&folder, &name, cx);
                    done = true;
                }
                if ui.button(s.t("cancel")).clicked() {
                    done = true;
                }
            });
            self.renaming = (!done).then_some((folder, name));
            ui.add_space(4.0);
        }
        let filter = self.filter.trim().to_lowercase();
        if self.notes.is_empty() && self.folders.is_empty() {
            form::hint(ui, &theme, &s.t("scratchpad-empty"));
        }
        let mut acts: Vec<SideAct> = Vec::new();
        egui::ScrollArea::vertical()
            .id_salt("scratchpad-notes")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let tree = Tree {
                    notes: &self.notes,
                    folders: &self.folders,
                    orders: &self.orders,
                    collapsed: &self.collapsed,
                    selected: self.state.id.as_deref(),
                    untitled: &self.untitled,
                    theme: &theme,
                    s,
                };
                if filter.is_empty() {
                    // Pinned notes keep their place at the top, whichever
                    // folder they live in, and cannot be dragged: unpin one
                    // to move it.
                    for n in self.notes.iter().filter(|n| n.pinned) {
                        plain_note_row(ui, &tree, n, &mut acts);
                    }
                    draw_level(ui, &tree, "", &mut acts);
                } else {
                    // A search lists its matches flat, each with the folder
                    // it sits in: a tree with most branches pruned away is
                    // harder to read than a list.
                    let hits: Vec<&NoteMeta> = self
                        .notes
                        .iter()
                        .filter(|n| {
                            n.title.to_lowercase().contains(&filter)
                                || n.folder.to_lowercase().contains(&filter)
                        })
                        .collect();
                    if hits.is_empty() {
                        form::hint(ui, &theme, &s.t("scratchpad-no-match"));
                    }
                    for n in hits {
                        plain_note_row(ui, &tree, n, &mut acts);
                    }
                }
            });
        for act in acts {
            self.apply(act, cx);
        }
        true
    }

    fn toolbar(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        let s = cx.services;
        for (view, icon, key) in [
            (View::Edit, ph::PENCIL_SIMPLE, "scratchpad-view-edit"),
            (View::Split, ph::COLUMNS, "scratchpad-view-split"),
            (View::Preview, ph::EYE, "scratchpad-view-preview"),
        ] {
            if ui
                .selectable_label(self.state.view == view, format!("{} {}", icon, s.t(key)))
                .clicked()
            {
                self.state.view = view;
            }
        }
        ui.separator();
        if ui
            .button(format!("{} {}", hs_ui::icons::CHECK, s.t("save")))
            .clicked()
            && self.save(cx)
        {
            cx.toast(ToastLevel::Success, s.t("toast-saved"));
        }
        if let Some(id) = self.state.id.clone() {
            let pinned = self.pins.contains(&id);
            let (icon, key) = if pinned {
                (ph::PUSH_PIN_SLASH, "scratchpad-unpin")
            } else {
                (ph::PUSH_PIN, "scratchpad-pin")
            };
            if ui
                .selectable_label(pinned, format!("{} {}", icon, s.t(key)))
                .clicked()
            {
                self.toggle_pin(&id, cx);
            }
        }
        if ui
            .button(format!("{} {}", ph::DOWNLOAD_SIMPLE, s.t("download")))
            .tip(s.t("scratchpad-download-tip"))
            .clicked()
        {
            let name = format!(
                "{}.md",
                hs_core::download::safe_name(
                    &title_of(&self.state.text).unwrap_or_else(|| "note".into())
                )
            );
            match hs_core::download::save_bytes(&name, self.state.text.as_bytes()) {
                Ok(Some(p)) => cx.toast(
                    ToastLevel::Success,
                    s.i18n
                        .t_args("download-saved", &[("path", &p.display().to_string())]),
                ),
                Ok(None) => {}
                Err(e) => cx.toast(ToastLevel::Error, e),
            }
        }
        hs_ui::toolbar::hard_right(ui, |ui| {
            // The assistant is the same control in every tool, so it sits in
            // the same place: hard right of the toolbar.
            if ui
                .selectable_label(
                    self.assistant.open,
                    format!("{} {}", ph::SPARKLE, s.t("ai-assistant")),
                )
                .tip(s.t("scratchpad-ai-tip"))
                .clicked()
            {
                self.assistant.toggle("Scratchpad");
            }
        });
        if self.state.environment.is_none()
            && let Some(env) = cx.env_id
        {
            self.state.environment = Some(env.to_string());
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        if cx.is_home {
            self.dashboard(ui, cx);
            return;
        }
        self.refresh_stamps(cx.services);
        // Autosave a moment after typing stops. A brand-new empty note is
        // not written: it only becomes a file once it has content.
        if self.dirty()
            && !self.state.text.trim().is_empty()
            && self.last_edit.is_some_and(|t| t.elapsed() > AUTOSAVE_AFTER)
        {
            self.save(cx);
        }
        let theme = Theme::from_settings(&cx.services.settings.read().appearance);
        let s = cx.services;
        // The same split every other tool uses: the column shrinks to keep
        // room for the note and steps aside in a narrow window, instead of a
        // fixed 390 px cut that left the column looking like it floated.
        let (mut main, panel) = hs_ui::ai_assistant::split_for_assistant(ui, self.assistant.open);
        let mut link: Option<String> = None;
        match self.state.view {
            View::Edit => self.editor(&mut main),
            View::Preview => link = self.preview(&mut main, &theme, s),
            View::Split => {
                let half = (main.available_width() * 0.5).round();
                egui::Panel::left(main.id().with("scratchpad-split"))
                    .resizable(true)
                    .default_size(half)
                    .size_range(160.0..=f32::INFINITY)
                    .show(&mut main, |ui| self.editor(ui));
                link = egui::CentralPanel::default()
                    .frame(egui::Frame::new().fill(theme.panel_bg))
                    .show(&mut main, |ui| self.preview(ui, &theme, s))
                    .inner;
            }
        }
        // A web address opens as a page in the Browser activity rather
        // than in the system browser: the note is part of the work, and
        // so is the page it points at. Only the preview's links do this;
        // the source pane is for editing.
        if let Some(url) = link
            && (url.starts_with("http://") || url.starts_with("https://"))
        {
            cx.open("browser", Some(OpenRequest::Url { url }));
        }
        if let Some(panel) = panel {
            // The note is the context; the system prompt asks for Markdown
            // pieces in fences so Insert can place them at the cursor.
            self.assistant
                .set_context("the open note", self.state.text.clone());
            self.assistant.extra_system = s.t("scratchpad-ai-system");
            self.assistant.can_insert = true;
            let mut child = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(panel)
                    .layout(egui::Layout::top_down(egui::Align::Min)),
            );
            child.set_clip_rect(panel.intersect(ui.clip_rect()));
            egui::Frame::new()
                .fill(theme.panel_bg)
                .stroke(egui::Stroke::new(1.0, theme.border))
                .show(&mut child, |ui| {
                    ui.set_min_width(370.0);
                    let out = self.assistant.ui(ui, s, &theme, "Scratchpad");
                    if let Some(block) = out.use_block {
                        // Markdown goes in as it is; any other language is
                        // kept as a fenced block of that language.
                        let lang = out.use_block_lang.trim().to_ascii_lowercase();
                        let text = if lang.is_empty() || lang == "markdown" || lang == "md" {
                            block
                        } else {
                            format!("```{lang}\n{block}\n```")
                        };
                        self.insert_pending = Some(text);
                    }
                });
        }
    }

    fn commands(&self) -> Vec<CommandDescriptor> {
        vec![
            CommandDescriptor {
                id: "scratchpad.save".into(),
                title: "Scratchpad: Save Note".into(),
                shortcut_action: Some("file.save".into()),
            },
            CommandDescriptor {
                id: "scratchpad.new".into(),
                title: "Scratchpad: New Note".into(),
                shortcut_action: None,
            },
            CommandDescriptor {
                id: "scratchpad.new_folder".into(),
                title: "Scratchpad: New Folder".into(),
                shortcut_action: None,
            },
            CommandDescriptor {
                id: "scratchpad.clear".into(),
                title: "Scratchpad: Clear Note".into(),
                shortcut_action: None,
            },
            CommandDescriptor {
                id: "scratchpad.ai".into(),
                title: "Scratchpad: Toggle AI Assistant".into(),
                shortcut_action: None,
            },
        ]
    }

    fn run_command(&mut self, id: &str, cx: &mut ToolContext) -> bool {
        match id {
            "scratchpad.save" => {
                if self.save(cx) {
                    cx.toast(ToastLevel::Success, cx.services.t("toast-saved"));
                }
                true
            }
            "scratchpad.new" => {
                cx.open_new_tab("scratchpad", None);
                true
            }
            "scratchpad.new_folder" => {
                self.new_folder("", cx);
                true
            }
            "scratchpad.clear" => {
                self.state.text.clear();
                self.last_edit = Some(Instant::now());
                true
            }
            "scratchpad.ai" => {
                self.assistant.toggle("Scratchpad");
                true
            }
            _ => false,
        }
    }

    fn save_state(&self) -> serde_json::Value {
        let mut st = self.state.clone();
        if st.id.is_some() {
            // The file is the source of truth for a saved note.
            st.text.clear();
        }
        serde_json::to_value(&st).unwrap_or_default()
    }

    fn current_uri(&self) -> Option<String> {
        self.state.id.as_ref().map(|id| format!("note://{id}"))
    }

    /// Every note, for Ctrl+P, with the folder it sits in as the subtitle.
    /// Each Scratchpad tab offers the same list; the palette keeps one copy
    /// of each, and `open` sends a note to its own tab whichever tab is
    /// handed the choice.
    fn quick_open_items(&self) -> Vec<QuickOpenItem> {
        self.notes
            .iter()
            .map(|n| QuickOpenItem {
                id: format!("note://{}", n.id),
                title: if n.title.is_empty() {
                    self.untitled.clone()
                } else {
                    n.title.clone()
                },
                subtitle: if n.folder.is_empty() {
                    self.tool_name.clone()
                } else {
                    format!("{} / {}", self.tool_name, n.folder.replace('/', " / "))
                },
            })
            .collect()
    }

    /// The tab showing a note takes requests for it; an empty new tab takes
    /// any note; a tab with another note never does.
    fn uri_affinity(&self, uri: &str) -> Option<u8> {
        let id = uri.strip_prefix("note://")?;
        match &self.state.id {
            Some(mine) if mine == id => Some(0),
            None if self.state.text.trim().is_empty() => Some(200),
            _ => None,
        }
    }

    fn open(&mut self, item: OpenRequest, cx: &mut ToolContext) -> bool {
        match item {
            OpenRequest::Text { body, .. } => {
                self.state.text = body;
                self.last_edit = Some(Instant::now());
                true
            }
            OpenRequest::File { uri, .. } => {
                tracing::debug!(%uri, current = ?self.state.id, "scratchpad: open");
                if let Some(id) = uri.strip_prefix("note://") {
                    // Quick-open hands an item to the tab that listed it,
                    // not to the tab that should show it. A tab holding
                    // another note - or the dashboard - passes the request
                    // to the shell, which focuses the note's own tab or opens
                    // one: one tab per note, and nothing typed here lost.
                    // The shell never routes a note to this tab in return
                    // (`uri_affinity` is `None` for another note, and the
                    // dashboard is never a candidate), so this cannot loop.
                    let mine = self.state.id.as_deref() == Some(id);
                    let empty = self.state.id.is_none() && self.state.text.trim().is_empty();
                    if self.home_title.is_some() || !(mine || empty) {
                        cx.open(
                            "scratchpad",
                            Some(OpenRequest::File {
                                uri: uri.clone(),
                                line: None,
                            }),
                        );
                        return true;
                    }
                    let Some(path) = find_note(&notes_dir(cx.services), id) else {
                        return false;
                    };
                    return match std::fs::read_to_string(&path) {
                        Ok(text) => {
                            self.state.id = Some(id.to_string());
                            self.saved = text.clone();
                            self.state.text = text;
                            self.last_edit = None;
                            self.path = Some(path);
                            true
                        }
                        Err(_) => false,
                    };
                }
                // A Markdown file from elsewhere starts a new note with its
                // content (the original file is not touched).
                let path = uri.strip_prefix("file://").unwrap_or(&uri);
                match std::fs::read_to_string(path) {
                    Ok(t) => {
                        self.state.id = None;
                        self.saved.clear();
                        self.state.text = t;
                        self.last_edit = Some(Instant::now());
                        true
                    }
                    Err(_) => false,
                }
            }
            _ => false,
        }
    }

    fn speakable(&self) -> Vec<Speakable> {
        let mut out = Vec::new();
        if !self.state.text.trim().is_empty() {
            out.push(Speakable::new("Note", self.state.text.clone()));
        }
        if let Some(a) = self.assistant.last_answer() {
            out.push(Speakable::new("Assistant's last answer", a));
        }
        out
    }

    fn printable(&self) -> Option<Printable> {
        (!self.state.text.trim().is_empty()).then(|| Printable {
            title: self.title().to_string(),
            subtitle: "Scratchpad".into(),
            html: hs_editor::markdown::to_html(&self.state.text),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_comes_from_the_first_heading_or_line() {
        assert_eq!(title_of("# Shopping\n- milk").as_deref(), Some("Shopping"));
        assert_eq!(
            title_of("\n\n  plain first line\n").as_deref(),
            Some("plain first line")
        );
        assert_eq!(title_of("   \n"), None);
        assert_eq!(title_of("###   \nnext").as_deref(), Some("next"));
    }

    #[test]
    fn saved_order_positions_unpinned_notes() {
        let mk = |id: &str, secs: u64| NoteMeta {
            id: id.into(),
            title: id.into(),
            modified: SystemTime::UNIX_EPOCH + Duration::from_secs(secs),
            pinned: false,
            folder: String::new(),
        };
        let notes = vec![mk("a", 1), mk("b", 2), mk("c", 3)];
        let out = order_notes(notes, &["b".into()], &["c".into(), "a".into()]);
        let ids: Vec<&str> = out.iter().map(|n| n.id.as_str()).collect();
        assert_eq!(ids, ["b", "c", "a"]);
    }

    #[test]
    fn pinned_notes_come_first_in_pin_order() {
        let mk = |id: &str, secs: u64| NoteMeta {
            id: id.into(),
            title: id.into(),
            modified: SystemTime::UNIX_EPOCH + Duration::from_secs(secs),
            pinned: false,
            folder: String::new(),
        };
        let notes = vec![
            mk("old", 1),
            mk("new", 3),
            mk("mid", 2),
            mk("pinned-late", 0),
        ];
        let out = order_notes(notes, &["mid".into(), "pinned-late".into()], &[]);
        let ids: Vec<&str> = out.iter().map(|n| n.id.as_str()).collect();
        assert_eq!(ids, ["mid", "pinned-late", "new", "old"]);
        assert!(out[0].pinned && out[1].pinned && !out[2].pinned);
    }

    #[test]
    fn notes_are_listed_newest_first_with_titles() {
        let dir = std::env::temp_dir().join(format!("hs-notes-{}", hs_core::new_id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.md"), "# First\nbody").unwrap();
        std::thread::sleep(Duration::from_millis(20));
        std::fs::write(dir.join("b.md"), "second line only").unwrap();
        std::fs::write(dir.join("ignored.txt"), "no").unwrap();
        let notes = list_notes(&dir);
        assert_eq!(notes.len(), 2);
        assert_eq!(notes[0].id, "b");
        assert_eq!(notes[0].title, "second line only");
        assert_eq!(notes[1].title, "First");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ---- folders (depth programme phase O) -------------------------------

    /// `notes/` with a note at the top, one in `Work` and one in
    /// `Work/Sub`, plus a dot-directory that is not a folder of notes.
    fn tree() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("Work").join("Sub")).unwrap();
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::write(root.join("top.md"), "# Top").unwrap();
        std::fs::write(root.join("Work").join("w.md"), "# In work").unwrap();
        std::fs::write(root.join("Work").join("Sub").join("s.md"), "# Deep").unwrap();
        std::fs::write(root.join(".git").join("x.md"), "not a note").unwrap();
        dir
    }

    fn note(id: &str, folder: &str, secs: u64) -> NoteMeta {
        NoteMeta {
            id: id.into(),
            title: id.into(),
            modified: SystemTime::UNIX_EPOCH + Duration::from_secs(secs),
            pinned: false,
            folder: folder.into(),
        }
    }

    fn keys(entries: &[Entry]) -> Vec<String> {
        entries.iter().map(Entry::key).collect()
    }

    #[test]
    fn notes_and_folders_are_read_through_the_whole_tree() {
        let dir = tree();
        let mut notes: Vec<(String, String)> = list_notes(dir.path())
            .into_iter()
            .map(|n| (n.id, n.folder))
            .collect();
        notes.sort();
        assert_eq!(
            notes,
            [
                ("s".to_string(), "Work/Sub".to_string()),
                ("top".to_string(), String::new()),
                ("w".to_string(), "Work".to_string()),
            ],
            "every note, with the folder it sits in; nothing from .git"
        );
        assert_eq!(list_folders(dir.path()), ["Work", "Work/Sub"]);
    }

    #[test]
    fn a_note_is_found_wherever_it_was_moved() {
        let dir = tree();
        let root = dir.path();
        assert_eq!(find_note(root, "top"), Some(root.join("top.md")));
        assert_eq!(
            find_note(root, "s"),
            Some(root.join("Work").join("Sub").join("s.md"))
        );
        assert_eq!(find_note(root, "missing"), None);
    }

    #[test]
    fn a_moved_note_keeps_its_id_and_its_content() {
        let dir = tree();
        let root = dir.path();
        move_note(root, "top", "Work/Sub").unwrap();
        assert!(!root.join("top.md").exists(), "the old file is gone");
        let moved = root.join("Work").join("Sub").join("top.md");
        assert_eq!(std::fs::read_to_string(moved).unwrap(), "# Top");
        // Back to the top level.
        move_note(root, "top", "").unwrap();
        assert_eq!(find_note(root, "top"), Some(root.join("top.md")));
    }

    #[test]
    fn a_level_holds_its_own_folders_and_unpinned_notes() {
        let folders = vec!["B".to_string(), "a".into(), "B/inner".into()];
        let mut pinned = note("pinned", "", 9);
        pinned.pinned = true;
        let notes = vec![
            note("old", "", 1),
            note("new", "", 3),
            pinned,
            note("elsewhere", "B", 5),
        ];
        // Nothing saved: folders by name (case ignored), then notes newest
        // first. Pinned notes and other levels' rows are not here.
        let top = level_entries("", &notes, &folders, &[]);
        assert_eq!(keys(&top), ["/a", "/B", "new", "old"]);
        let inside = level_entries("B", &notes, &folders, &[]);
        assert_eq!(keys(&inside), ["/inner", "elsewhere"]);
    }

    #[test]
    fn a_saved_order_is_kept_and_new_rows_come_first() {
        let folders = vec!["F".to_string()];
        let notes = vec![note("a", "", 1), note("b", "", 2), note("fresh", "", 9)];
        // `fresh` was made after the order was saved, so it is not in it.
        let order = vec!["b".to_string(), "/F".into(), "a".into()];
        let level = level_entries("", &notes, &folders, &order);
        assert_eq!(keys(&level), ["fresh", "b", "/F", "a"]);
    }

    #[test]
    fn an_order_saved_before_folders_existed_still_reads() {
        // The old `order.json` held note ids and nothing else.
        let notes = vec![note("a", "", 1), note("b", "", 2)];
        let level = level_entries("", &notes, &[], &["a".to_string(), "b".into()]);
        assert_eq!(keys(&level), ["a", "b"]);
    }

    #[test]
    fn a_new_folder_whose_name_is_taken_gets_the_usual_suffix() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        assert_eq!(create_folder(root, "", "Work").unwrap(), "Work");
        assert_eq!(create_folder(root, "", "Work").unwrap(), "Work 001");
        assert_eq!(create_folder(root, "Work", "Inner").unwrap(), "Work/Inner");
        // `work` and `Work` are one folder on Windows and macOS.
        assert_eq!(create_folder(root, "", "work").unwrap(), "work 002");
        assert!(
            create_folder(root, "", "   ").is_err(),
            "a folder needs a name"
        );
    }

    #[test]
    fn renaming_a_folder_only_in_case_is_not_a_clash() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        create_folder(root, "", "work").unwrap();
        assert_eq!(rename_folder(root, "work", "Work").unwrap(), "Work");
        create_folder(root, "", "Home").unwrap();
        assert_eq!(rename_folder(root, "Home", "WORK").unwrap(), "WORK 001");
    }

    #[test]
    fn a_folder_moves_with_everything_in_it_but_never_into_itself() {
        let dir = tree();
        let root = dir.path();
        assert!(move_folder(root, "Work", "Work").is_err());
        assert!(move_folder(root, "Work", "Work/Sub").is_err());

        create_folder(root, "", "Archive").unwrap();
        assert_eq!(
            move_folder(root, "Work", "Archive").unwrap(),
            "Archive/Work"
        );
        assert_eq!(
            find_note(root, "s"),
            Some(root.join("Archive").join("Work").join("Sub").join("s.md")),
            "the folder took its notes with it"
        );
        // A destination that already has a folder of that name.
        create_folder(root, "", "Work").unwrap();
        assert_eq!(
            move_folder(root, "Work", "Archive").unwrap(),
            "Archive/Work 001"
        );
    }

    #[test]
    fn collapsed_folders_follow_a_rename() {
        let set: std::collections::HashSet<String> =
            ["Work".to_string(), "Work/Sub".into(), "Workshop".into()]
                .into_iter()
                .collect();
        let moved = remap_folders(&set, "Work", "Archive/Work");
        assert!(moved.contains("Archive/Work") && moved.contains("Archive/Work/Sub"));
        // A sibling whose name merely starts the same way is left alone.
        assert!(moved.contains("Workshop"));
        assert!(!moved.contains("Work"));
    }

    #[test]
    fn a_folder_name_is_one_every_platform_accepts() {
        assert_eq!(folder_name("Work").as_deref(), Some("Work"));
        assert_eq!(folder_name("a/b\\c").as_deref(), Some("a_b_c"));
        assert_eq!(folder_name("Work. ").as_deref(), Some("Work"));
        assert_eq!(folder_name(""), None);
        assert_eq!(folder_name("."), None);
        assert_eq!(folder_name(".."), None);
    }

    #[test]
    fn a_folder_holds_only_what_is_inside_it() {
        assert!(is_within("Work", "Work"));
        assert!(is_within("Work", "Work/Sub"));
        assert!(is_within("work", "Work/Sub"), "case is ignored, as on disk");
        assert!(!is_within("Work", "Workshop"));
        assert!(!is_within("Work/Sub", "Work"));
    }
}
