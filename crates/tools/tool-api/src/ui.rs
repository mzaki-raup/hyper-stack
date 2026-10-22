//! The API client's panels: the collections sidebar, the request editor
//! (method, URL, params, headers, body, auth, scripts, docs) and the
//! response viewer (body as pretty JSON / raw / preview, headers, cookies,
//! test results, console).

use hs_core::Services;
use hs_core::tool::{ToastLevel, Tool, ToolContext};
use hs_editor::buffer::Buffer;
use hs_editor::cursor::Cursors;
use hs_editor::languages;
use hs_editor::view::{EditorTheme, EditorView};
use hs_ui::form;
use hs_ui::icons::ph;
use hs_ui::kv_editor::{KvEditor, KvRow};
use hs_ui::theme::Theme;

use crate::ApiTool;
use crate::model::{
    Auth, AuthKind, BodyKind, Collection, Folder, Item, Kv, LibraryKind, METHODS, Request,
};
use crate::send::Exchange;

pub struct SaveTo {
    pub collection: String,
    pub folder: Option<String>,
    pub name: String,
}

/// The JSON/text body editor on `hs-editor`.
pub struct BodyEditor {
    buf: Buffer,
    cursors: Cursors,
    view: EditorView,
}

impl BodyEditor {
    pub fn new(s: &Services) -> Self {
        let st = s.settings.read();
        let dark = Theme::from_settings(&st.appearance).dark;
        let mut view = EditorView::new(
            egui::Id::new(("hs-api-body", hs_core::new_id())),
            languages::by_extension("json"),
            dark,
            st.editor.font_size,
        );
        view.right_click_copy_paste = st.editor.right_click_copy_paste;
        Self {
            buf: Buffer::from_text(""),
            cursors: Cursors::default(),
            view,
        }
    }
    pub fn set_text(&mut self, text: &str, kind: BodyKind) {
        self.buf = Buffer::from_text(text);
        self.cursors = Cursors::default();
        self.view.set_language(languages::by_extension(match kind {
            BodyKind::Json => "json",
            BodyKind::Xml => "xml",
            _ => "txt",
        }));
    }
    /// Load a body and lay it out: a request that arrived on one line
    /// (an import, an assistant block, a paste) is readable at once.
    /// Typing is never reformatted under the cursor; only loading is.
    pub fn set_text_pretty(&mut self, text: &str, kind: BodyKind) {
        let pretty = match kind {
            BodyKind::Json => hs_core::fmt::pretty_json(text),
            BodyKind::Xml => hs_core::fmt::pretty_xml(text),
            _ => None,
        };
        self.set_text(pretty.as_deref().unwrap_or(text), kind);
    }

    pub fn text(&self) -> String {
        self.buf.text()
    }
    /// Bumps on every edit; cheaper than comparing the text.
    pub fn version(&self) -> u64 {
        self.buf.version()
    }
    fn show(&mut self, ui: &mut egui::Ui, theme: &Theme) {
        let ed = EditorTheme::for_theme(theme);
        self.view.show(ui, &mut self.buf, &mut self.cursors, &ed);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum BodyMode {
    #[default]
    Pretty,
    Raw,
    Preview,
}

/// The response body, highlighted and read-only.
pub struct ResponseView {
    buf: Buffer,
    cursors: Cursors,
    view: EditorView,
    mode: BodyMode,
    raw: Vec<u8>,
    pretty: String,
    sniffed: Option<hs_core::sniff::Sniffed>,
    image_uri: String,
}

impl ResponseView {
    pub fn new(s: &Services) -> Self {
        let st = s.settings.read();
        let dark = Theme::from_settings(&st.appearance).dark;
        let mut view = EditorView::new(
            egui::Id::new(("hs-api-response", hs_core::new_id())),
            languages::by_extension("json"),
            dark,
            st.editor.font_size,
        );
        view.read_only = true;
        Self {
            buf: Buffer::from_text(""),
            cursors: Cursors::default(),
            view,
            mode: BodyMode::Pretty,
            raw: Vec::new(),
            pretty: String::new(),
            sniffed: None,
            image_uri: String::new(),
        }
    }

    pub fn set(&mut self, ex: &Exchange, _s: &Services) {
        self.raw = ex.body.clone();
        let sn = hs_core::sniff::sniff(&ex.body);
        let ct = ex
            .headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("content-type"))
            .map(|(_, v)| v.to_lowercase())
            .unwrap_or_default();
        let text = String::from_utf8_lossy(&ex.body).into_owned();
        let (pretty, ext) = if ct.contains("json") || sn.ext == "json" {
            (
                hs_core::fmt::pretty_json(&text).unwrap_or_else(|| text.clone()),
                "json",
            )
        } else if ct.contains("xml") || sn.ext == "xml" {
            (
                hs_core::fmt::pretty_xml(&text).unwrap_or_else(|| text.clone()),
                "xml",
            )
        } else if ct.contains("html") {
            (
                hs_core::fmt::pretty_xml(&text).unwrap_or_else(|| text.clone()),
                "html",
            )
        } else {
            (text.clone(), "txt")
        };
        self.pretty = pretty;
        self.view.set_language(languages::by_extension(ext));
        self.buf = Buffer::from_text(&self.pretty);
        self.cursors = Cursors::default();
        self.image_uri = format!("bytes://api-response-{}.{}", hs_core::new_id(), sn.ext);
        self.mode = if sn.kind == hs_core::sniff::Kind::Image {
            BodyMode::Preview
        } else {
            BodyMode::Pretty
        };
        self.sniffed = Some(sn);
    }

    fn show(&mut self, ui: &mut egui::Ui, s: &Services, theme: &Theme) {
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.mode, BodyMode::Pretty, s.t("api-body-pretty"));
            ui.selectable_value(&mut self.mode, BodyMode::Raw, s.t("api-body-raw"));
            ui.selectable_value(&mut self.mode, BodyMode::Preview, s.t("api-body-preview"));
            ui.separator();
            if let Some(sn) = &self.sniffed {
                ui.label(
                    egui::RichText::new(format!("{} · {}", sn.label, human_size(self.raw.len())))
                        .color(theme.text_muted),
                );
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .small_button(format!("{} {}", ph::COPY, s.t("api-copy-body")))
                    .clicked()
                {
                    ui.ctx().copy_text(self.pretty.clone());
                }
                if ui
                    .small_button(format!("{} {}", ph::FLOPPY_DISK, s.t("api-save-body")))
                    .clicked()
                    && let Some(sn) = &self.sniffed
                    && let Some(p) = rfd::FileDialog::new()
                        .set_file_name(format!("response.{}", sn.ext))
                        .save_file()
                {
                    let _ = std::fs::write(p, &self.raw);
                }
            });
        });
        ui.add_space(4.0);
        match self.mode {
            BodyMode::Pretty => {
                let ed = EditorTheme::for_theme(theme);
                self.view.show(ui, &mut self.buf, &mut self.cursors, &ed);
            }
            BodyMode::Raw => {
                egui::ScrollArea::both()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let mut raw = String::from_utf8_lossy(&self.raw).into_owned();
                        ui.add(
                            egui::TextEdit::multiline(&mut raw)
                                .font(egui::TextStyle::Monospace)
                                .desired_width(f32::INFINITY),
                        );
                    });
            }
            BodyMode::Preview => {
                egui::ScrollArea::both()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        match self.sniffed.as_ref().map(|sn| sn.kind) {
                            Some(hs_core::sniff::Kind::Image | hs_core::sniff::Kind::Svg) => {
                                ui.add(
                                    egui::Image::from_bytes(
                                        self.image_uri.clone(),
                                        self.raw.clone(),
                                    )
                                    .max_size(egui::vec2(ui.available_width(), 2000.0)),
                                );
                            }
                            _ => {
                                // Sanitised text: tags stripped, no scripts, as a plain preview.
                                let text = strip_tags(&String::from_utf8_lossy(&self.raw));
                                ui.add(egui::Label::new(text).wrap());
                            }
                        }
                    });
            }
        }
    }
}

fn strip_tags(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    let mut skip_block: Option<&str> = None;
    let lower = html.to_lowercase();
    let mut i = 0;
    let bytes: Vec<char> = html.chars().collect();
    let lower_chars: Vec<char> = lower.chars().collect();
    while i < bytes.len() {
        let c = bytes[i];
        if let Some(end) = skip_block {
            let tail: String = lower_chars[i..].iter().take(end.len()).collect();
            if tail == end {
                i += end.len();
                skip_block = None;
                in_tag = false;
            } else {
                i += 1;
            }
            continue;
        }
        if c == '<' {
            let tail: String = lower_chars[i..].iter().take(8).collect();
            if tail.starts_with("<script") {
                skip_block = Some("</script>");
                i += 1;
                continue;
            }
            if tail.starts_with("<style") {
                skip_block = Some("</style>");
                i += 1;
                continue;
            }
            in_tag = true;
        } else if c == '>' && in_tag {
            in_tag = false;
            out.push(' ');
        } else if !in_tag {
            out.push(c);
        }
        i += 1;
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn human_size(n: usize) -> String {
    if n < 1024 {
        format!("{n} B")
    } else if n < 1024 * 1024 {
        format!("{:.1} KB", n as f64 / 1024.0)
    } else {
        format!("{:.1} MB", n as f64 / (1024.0 * 1024.0))
    }
}

/// The colour an HTTP method is shown in. Fixed colours rather than theme
/// roles: these are the ones every API tool uses, and a reader picks the
/// verb out by its colour before reading the word. Owner's rule,
/// 2026-10-17.
pub fn method_rgb(method: &str) -> Option<egui::Color32> {
    Some(match method {
        "GET" => egui::Color32::from_rgb(0x3F, 0xB9, 0x50),
        "POST" => egui::Color32::from_rgb(0x3B, 0x82, 0xF6),
        "PUT" => egui::Color32::from_rgb(0xF5, 0x9E, 0x0B),
        "PATCH" => egui::Color32::from_rgb(0xA8, 0x55, 0xF7),
        "DELETE" => egui::Color32::from_rgb(0xEF, 0x44, 0x44),
        // HEAD, OPTIONS and TRACE have no colour of their own: they are
        // rare enough that one would be noise.
        _ => return None,
    })
}

pub fn method_colour(theme: &Theme, method: &str) -> egui::Color32 {
    method_rgb(method).unwrap_or(theme.text_muted)
}

fn to_rows(list: &[Kv]) -> Vec<KvRow> {
    list.iter()
        .map(|k| KvRow {
            enabled: k.enabled,
            key: k.key.clone(),
            value: k.value.clone(),
            secret: false,
        })
        .collect()
}

fn from_rows(rows: &[KvRow], old: &[Kv]) -> Vec<Kv> {
    rows.iter()
        .enumerate()
        .map(|(i, r)| Kv {
            enabled: r.enabled,
            key: r.key.clone(),
            value: r.value.clone(),
            description: old
                .get(i)
                .map(|k| k.description.clone())
                .unwrap_or_default(),
        })
        .collect()
}

fn kv_table(ui: &mut egui::Ui, theme: &Theme, s: &Services, list: &mut Vec<Kv>, id: &str) {
    let mut rows = to_rows(list);
    let r = ui
        .push_id(id, |ui| {
            KvEditor {
                theme,
                key_hint: &s.t("api-kv-key"),
                value_hint: &s.t("api-kv-value"),
                add_label: &s.t("api-kv-add"),
                allow_secret: false,
            }
            .show(ui, &mut rows)
        })
        .inner;
    if r.changed || rows.len() != list.len() {
        *list = from_rows(&rows, list);
    }
}

// ---------------------------------------------------------------------------
// Sidebar

pub(crate) enum SideAction {
    Open(String, String),
    NewRequest(String, Option<String>),
    NewFolder(String, Option<String>),
    Rename(String, String, String),
    Delete(String, Option<String>),
    Duplicate(String, String),
    /// A whole collection copied beside itself, with everything in it.
    DuplicateCollection(String),
    Export(String),
    Import,
    NewCollection,
    LoadHistory(usize),
    /// Open a collection's management page in its own tab.
    ManageCollection(String),
    /// Open a package library in its own tab.
    OpenLibrary(String),
    NewLibrary,
    DeleteLibrary(String),
    /// A dragged row's new home: out of `from_folder` at `from`, into
    /// `to_folder` at the boundary `to`. The two folders are equal for a
    /// reorder within one level and differ when a row moves between
    /// levels. `collection` empty means the collections themselves.
    /// Move a request or folder: out of (`from_collection`,
    /// `from_folder`) at `from`, into (`to_collection`, `to_folder`) at
    /// `to`. Two empty collection ids mean the collections themselves,
    /// which are a flat list. `to` may be `usize::MAX` for "at the end",
    /// which is what a drop onto a folder or a collection means.
    MoveItem {
        from_collection: String,
        from_folder: Option<String>,
        from: usize,
        to_collection: String,
        to_folder: Option<String>,
        to: usize,
    },
}

pub fn sidebar(t: &mut ApiTool, ui: &mut egui::Ui, cx: &mut ToolContext, theme: &Theme) {
    let s = cx.services;
    let mut actions: Vec<SideAction> = Vec::new();
    form::header_bar(ui, theme, |ui| {
        ui.label(egui::RichText::new(s.t("tool-api")).strong().size(15.0));
    });
    ui.horizontal(|ui| {
        if ui
            .small_button(ph::FOLDER_PLUS)
            .on_hover_text(s.t("api-new-collection"))
            .clicked()
        {
            actions.push(SideAction::NewCollection);
        }
        let collection = t.state_collection();
        if ui
            .add_enabled(
                collection.is_some(),
                egui::Button::new(ph::FOLDER_PLUS).small(),
            )
            .on_hover_text(s.t("api-new-folder"))
            .clicked()
            && let Some(c) = collection
        {
            actions.push(SideAction::NewFolder(c, None));
        }
        if ui
            .small_button(ph::PLUS)
            .on_hover_text(s.t("api-new-request"))
            .clicked()
        {
            match t.home_for_new_request() {
                Some((c, folder)) => actions.push(SideAction::NewRequest(c, folder)),
                None => actions.push(SideAction::NewCollection),
            }
        }
        if ui
            .small_button(ph::DOWNLOAD_SIMPLE)
            .on_hover_text(s.t("api-import-tip"))
            .clicked()
        {
            actions.push(SideAction::Import);
        }
        if ui
            .small_button(ph::CLOCK_COUNTER_CLOCKWISE)
            .on_hover_text(s.t("api-history"))
            .clicked()
        {
            t.show_history = !t.show_history;
        }
    });
    ui.add(
        egui::TextEdit::singleline(&mut t.filter)
            .hint_text(s.t("api-filter"))
            .desired_width(f32::INFINITY),
    );
    ui.add_space(4.0);
    let filter = t.filter.trim().to_lowercase();
    let selected = t.state_request_id();
    // The tree keeps the top; the package libraries get the bottom ~30 %,
    // scrolling on their own.
    let avail = ui.available_height();
    let lib_h = (avail * 0.3).clamp(120.0, 280.0);
    let tree_h = (avail - lib_h - 10.0).max(80.0);
    egui::ScrollArea::vertical()
        .id_salt("api-sidebar")
        .auto_shrink([false, false])
        .max_height(tree_h)
        .show(ui, |ui| {
            if t.collections.is_empty() {
                form::note(ui, theme, hs_ui::icons::INFO, &s.t("api-empty-hint"));
            }
            let mut collections = hs_ui::reorder::Reorder::new("api-collections");
            for (ci, c) in t.collections.iter().enumerate() {
                let header = collections.row(ui, ci, |ui| {
                    ui.horizontal(|ui| {
                        if let Some(icon) = hs_ui::fileicons::by_name("folder-api") {
                            ui.add(
                                egui::Image::new(icon).fit_to_exact_size(egui::vec2(16.0, 16.0)),
                            );
                        }
                        egui::CollapsingHeader::new(egui::RichText::new(&c.name).strong())
                            .id_salt(("api-col", &c.id))
                            .default_open(true)
                            .show(ui, |ui| {
                                items_ui(
                                    ui,
                                    theme,
                                    s,
                                    &c.id,
                                    None,
                                    &c.items,
                                    &filter,
                                    selected.as_deref(),
                                    &mut actions,
                                );
                            })
                    })
                    .inner
                });
                // The collection itself takes a row, so a request can be
                // dragged to another collection even when that one is
                // collapsed or empty - the case with no rows to aim
                // between. Owner's report, 2026-10-18.
                let root = scope_of(&c.id, None);
                if hs_ui::reorder::dragged_scope(ui.ctx()).is_some_and(|src| src != root)
                    && let Some(d) =
                        hs_ui::reorder::drop_onto(ui, header.header_response.rect, theme.accent)
                {
                    let (from_collection, from_folder) = from_scope(&d.scope);
                    actions.push(SideAction::MoveItem {
                        from_collection,
                        from_folder,
                        from: d.index,
                        to_collection: c.id.clone(),
                        to_folder: None,
                        to: usize::MAX,
                    });
                }
                header.header_response.context_menu(|ui| {
                    if ui.button(s.t("api-new-request")).clicked() {
                        actions.push(SideAction::NewRequest(c.id.clone(), None));
                        ui.close();
                    }
                    if ui.button(s.t("api-new-folder")).clicked() {
                        actions.push(SideAction::NewFolder(c.id.clone(), None));
                        ui.close();
                    }
                    if ui.button(s.t("api-rename")).clicked() {
                        actions.push(SideAction::Rename(
                            c.id.clone(),
                            c.id.clone(),
                            c.name.clone(),
                        ));
                        ui.close();
                    }
                    if ui.button(s.t("api-duplicate")).clicked() {
                        actions.push(SideAction::DuplicateCollection(c.id.clone()));
                        ui.close();
                    }
                    if ui.button(s.t("api-export-postman")).clicked() {
                        actions.push(SideAction::Export(c.id.clone()));
                        ui.close();
                    }
                    ui.separator();
                    if ui.button(s.t("api-manage-collection")).clicked() {
                        actions.push(SideAction::ManageCollection(c.id.clone()));
                        ui.close();
                    }
                    ui.separator();
                    if ui.button(s.t("api-delete-collection")).clicked() {
                        actions.push(SideAction::Delete(c.id.clone(), None));
                        ui.close();
                    }
                });
            }
            if let Some((from, to)) = collections.finish(ui, theme.accent) {
                actions.push(SideAction::MoveItem {
                    from_collection: String::new(),
                    from_folder: None,
                    from,
                    to_collection: String::new(),
                    to_folder: None,
                    to,
                });
            }
            if t.show_history {
                ui.add_space(8.0);
                ui.separator();
                ui.label(
                    egui::RichText::new(s.t("api-history"))
                        .strong()
                        .color(theme.text_muted),
                );
                for (i, h) in t.history.iter().enumerate().rev().take(60) {
                    let col = status_colour(theme, h.status);
                    let text = format!("{} {} {}", h.status, h.method, h.url);
                    if ui
                        .add(
                            egui::Label::new(egui::RichText::new(text).small().color(col))
                                .truncate()
                                .selectable(false)
                                .sense(egui::Sense::click()),
                        )
                        .on_hover_text(format!("{} · {} ms", h.at, h.millis))
                        .clicked()
                    {
                        actions.push(SideAction::LoadHistory(i));
                    }
                }
            }
        });
    ui.add_space(4.0);
    ui.separator();
    libraries_section(t, ui, s, theme, lib_h, &mut actions);
    apply_side_actions(t, actions, cx);
}

/// The sidebar's lower section: the workspace's package libraries, one row
/// each, opening in their own tab.
fn libraries_section(
    t: &ApiTool,
    ui: &mut egui::Ui,
    s: &Services,
    theme: &Theme,
    height: f32,
    actions: &mut Vec<SideAction>,
) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(s.t("api-libraries"))
                .small()
                .strong()
                .color(theme.text_muted),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if hs_ui::form::icon_button(ui, theme, ph::PLUS, 16.0, false)
                .on_hover_text(s.t("api-new-library"))
                .clicked()
            {
                actions.push(SideAction::NewLibrary);
            }
        });
    });
    let open = match &t.state.mode {
        crate::TabMode::Library(id) => Some(id.as_str()),
        _ => None,
    };
    egui::ScrollArea::vertical()
        .id_salt("api-libraries")
        .auto_shrink([false, false])
        .max_height((height - 30.0).max(60.0))
        .show(ui, |ui| {
            if t.libraries.is_empty() {
                form::hint(ui, theme, &s.t("api-libraries-empty"));
                return;
            }
            for l in &t.libraries {
                let kind = s.t(l.kind.label_key());
                let sub = if l.enabled {
                    kind
                } else {
                    format!("{kind} · {}", s.t("disabled"))
                };
                let r = form::list_row(
                    ui,
                    theme,
                    open == Some(l.id.as_str()),
                    ph::PACKAGE,
                    &l.name,
                    Some(&sub),
                );
                if r.clicked() {
                    actions.push(SideAction::OpenLibrary(l.id.clone()));
                }
                r.context_menu(|ui| {
                    if ui.button(s.t("api-library-delete")).clicked() {
                        actions.push(SideAction::DeleteLibrary(l.id.clone()));
                        ui.close();
                    }
                });
            }
        });
}

/// Carry out what the sidebar (or the home tab) asked for. Its own
/// function so the home tab can push the same actions rather than
/// repeating what each one means.
pub(crate) fn apply_side_actions(t: &mut ApiTool, actions: Vec<SideAction>, cx: &mut ToolContext) {
    let s = cx.services;
    for a in actions {
        match a {
            SideAction::Open(c, r) => {
                // The shell decides where it lands: the tab that already
                // holds this request, an empty one, or a new one.
                cx.open(
                    "api",
                    Some(hs_core::tool::OpenRequest::File {
                        uri: format!("api://{c}/{r}"),
                        line: None,
                    }),
                );
            }
            SideAction::NewRequest(c, folder) => {
                // Numbered against the folder it lands in, so a second
                // New Request does not collide with the first.
                let taken = t.sibling_names(&c, folder.as_deref());
                let name = crate::model::free_name(&s.t("api-new-request-name"), &taken);
                t.new_request();
                t.request.name = name.clone();
                t.save_as(&c, folder.as_deref(), &name, cx);
            }
            SideAction::NewFolder(c, folder) => {
                if let Some(col) = t.collections.iter_mut().find(|x| x.id == c)
                    && let Some(items) = col.items_mut(folder.as_deref())
                {
                    let base = s.t("api-new-folder-name");
                    // Requests and folders share one namespace in a level
                    // (rename and drag already treat them so).
                    let name =
                        crate::unique_name(&base, items.iter().map(|i| i.name().to_string()));
                    items.push(Item::Folder(Folder {
                        name,
                        ..Default::default()
                    }));
                }
                t.save_collection(&c, cx);
            }
            SideAction::Rename(c, id, name) => t.rename = Some((format!("{c}/{id}"), name)),
            SideAction::ManageCollection(c) => cx.open(
                "api",
                Some(hs_core::tool::OpenRequest::File {
                    uri: format!("api://manage/{c}"),
                    line: None,
                }),
            ),
            SideAction::OpenLibrary(id) => cx.open(
                "api",
                Some(hs_core::tool::OpenRequest::File {
                    uri: format!("api://library/{id}"),
                    line: None,
                }),
            ),
            SideAction::NewLibrary => {
                let taken: Vec<String> = t.libraries.iter().map(|l| l.name.clone()).collect();
                let lib = crate::model::PackageLibrary {
                    name: crate::model::free_name(&s.t("api-new-library-name"), &taken),
                    ..Default::default()
                };
                let id = lib.id.clone();
                t.libraries.push(lib);
                t.save_library(&id, cx);
                cx.open(
                    "api",
                    Some(hs_core::tool::OpenRequest::File {
                        uri: format!("api://library/{id}"),
                        line: None,
                    }),
                );
            }
            SideAction::DeleteLibrary(id) => t.confirm_delete_library = Some(id),
            SideAction::MoveItem {
                from_collection,
                from_folder,
                from,
                to_collection,
                to_folder,
                to,
            } => t.move_item(
                &from_collection,
                from_folder.as_deref(),
                from,
                &to_collection,
                to_folder.as_deref(),
                to,
                cx,
            ),
            // Deleting a collection removes its file; deleting a folder
            // takes every request inside it. Both wait for a confirmation.
            SideAction::Delete(c, item) => t.confirm_delete = Some((c, item)),
            SideAction::Duplicate(c, id) => {
                // A request or a folder: a folder brings everything inside
                // it, with new ids throughout so the copies are their own
                // requests. Owner's requirement, 2026-10-17.
                let found = t.collections.iter().find(|x| x.id == c).and_then(|col| {
                    let parent = parent_folder(col, &id);
                    col.items(parent.as_deref())
                        .and_then(|items| items.iter().find(|i| i.id() == id))
                        .map(|item| (crate::model::deep_copy(item), parent))
                });
                if let Some((mut copy, folder)) = found {
                    if let Some(col) = t.collections.iter_mut().find(|x| x.id == c)
                        && let Some(items) = col.items_mut(folder.as_deref())
                    {
                        // Numbered against what is already beside it, so
                        // two duplicates never land on the same name.
                        let taken: Vec<String> =
                            items.iter().map(|i| i.name().to_string()).collect();
                        let free = crate::model::next_copy_name(copy.name(), &taken);
                        copy.set_name(&free);
                        items.push(copy);
                    }
                    t.save_collection(&c, cx);
                }
            }
            SideAction::DuplicateCollection(c) => {
                if let Some(src) = t.collections.iter().find(|x| x.id == c).cloned() {
                    let mut copy = src.clone();
                    copy.id = hs_core::new_id();
                    copy.items = src.items.iter().map(crate::model::deep_copy).collect();
                    let taken: Vec<String> = t.collections.iter().map(|x| x.name.clone()).collect();
                    copy.name = crate::model::next_copy_name(&src.name, &taken);
                    // At the end of the list, where a new collection goes.
                    copy.order = t.collections.len() as i32;
                    let id = copy.id.clone();
                    t.collections.push(copy);
                    t.save_collection(&id, cx);
                }
            }
            SideAction::Export(c) => t.export_postman(&c, cx),
            SideAction::Import => {
                if let Some(p) = rfd::FileDialog::new()
                    .add_filter("Collections", &["json", "yaml", "yml", "bru"])
                    .pick_file()
                {
                    t.import_postman_file(&p, cx);
                }
            }
            SideAction::NewCollection => t.new_collection = Some(String::new()),
            SideAction::LoadHistory(i) => {
                if let Some(h) = t.history.get(i).cloned() {
                    t.new_request();
                    let kind = h.request.body.kind;
                    let text = h.request.body.text.clone();
                    t.request = h.request;
                    t.request.id = hs_core::new_id();
                    t.body_view.set_text_pretty(&text, kind);
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)] // one level of the tree, one argument each.
fn items_ui(
    ui: &mut egui::Ui,
    theme: &Theme,
    s: &hs_core::Services,
    collection: &str,
    folder: Option<&str>,
    items: &[Item],
    filter: &str,
    selected: Option<&str>,
    actions: &mut Vec<SideAction>,
) {
    // Each level reorders among its own siblings: a request never leaves
    // its folder by being dragged, which is the move people actually mean.
    // Each level names itself, so a row dropped in another level says
    // where it came from and the tool can move it across — out of a
    // folder, into one, or between siblings. Owner's requirement,
    // 2026-10-17.
    let scope = scope_of(collection, folder);
    let mut reorder = hs_ui::reorder::Reorder::scoped(
        (
            "api-items",
            collection.to_string(),
            folder.unwrap_or("").to_string(),
        ),
        scope,
    );
    for (index, it) in items.iter().enumerate() {
        match it {
            Item::Folder(f) => {
                let matches = filter.is_empty() || f.name.to_lowercase().contains(filter);
                let has_match = !filter.is_empty()
                    && f.items
                        .iter()
                        .any(|i| i.name().to_lowercase().contains(filter));
                if !matches && !has_match && !filter.is_empty() {
                    // Still descend: a nested request may match.
                }
                let r = reorder.row(ui, index, |ui| {
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::Image::new(hs_ui::fileicons::for_folder(&f.name))
                                .fit_to_exact_size(egui::vec2(16.0, 16.0)),
                        );
                        egui::CollapsingHeader::new(&f.name)
                            .id_salt(("api-folder", &f.id))
                            .default_open(!filter.is_empty())
                            .show(ui, |ui| {
                                items_ui(
                                    ui,
                                    theme,
                                    s,
                                    collection,
                                    Some(&f.id),
                                    &f.items,
                                    filter,
                                    selected,
                                    actions,
                                );
                            })
                    })
                    .inner
                });
                // A folder is a drop target in its own right. Between-rows
                // drops can only reach a folder that is open and not empty,
                // which is why dragging "into a folder" looked broken: a
                // collapsed or empty one had no rows to aim between.
                // Owner's report, 2026-10-17.
                // Any level of any collection may send a row here; only
                // this folder's own rows have nowhere to go.
                // Owner's report, 2026-10-18.
                let inside = scope_of(collection, Some(&f.id));
                let from_elsewhere =
                    hs_ui::reorder::dragged_scope(ui.ctx()).is_some_and(|src| src != inside);
                if from_elsewhere {
                    let rect = r.header_response.rect;
                    if ui
                        .input(|i| i.pointer.interact_pos())
                        .is_some_and(|p| rect.contains(p))
                    {
                        // One indicator, not two: this drop has no place
                        // in the list the pointer happens to be over.
                        reorder.suppress_target();
                    }
                    if let Some(d) = hs_ui::reorder::drop_onto(ui, rect, theme.accent) {
                        let (c, from_folder) = from_scope(&d.scope);
                        actions.push(SideAction::MoveItem {
                            from_collection: c,
                            from_folder,
                            from: d.index,
                            to_collection: collection.to_string(),
                            to_folder: Some(f.id.clone()),
                            // Past the end: the handler clamps, so the row
                            // lands at the bottom of the folder it joined.
                            to: usize::MAX,
                        });
                    }
                }
                r.header_response.context_menu(|ui| {
                    if ui.button(s.t("api-new-request")).clicked() {
                        actions.push(SideAction::NewRequest(
                            collection.into(),
                            Some(f.id.clone()),
                        ));
                        ui.close();
                    }
                    if ui.button(s.t("api-new-folder")).clicked() {
                        actions.push(SideAction::NewFolder(collection.into(), Some(f.id.clone())));
                        ui.close();
                    }
                    if ui.button(s.t("api-rename")).clicked() {
                        actions.push(SideAction::Rename(
                            collection.into(),
                            f.id.clone(),
                            f.name.clone(),
                        ));
                        ui.close();
                    }
                    if ui.button(s.t("api-duplicate")).clicked() {
                        actions.push(SideAction::Duplicate(collection.into(), f.id.clone()));
                        ui.close();
                    }
                    ui.separator();
                    if ui.button(s.t("api-delete-folder")).clicked() {
                        actions.push(SideAction::Delete(collection.into(), Some(f.id.clone())));
                        ui.close();
                    }
                });
            }
            Item::Request(r) => {
                if !filter.is_empty()
                    && !r.name.to_lowercase().contains(filter)
                    && !r.url.to_lowercase().contains(filter)
                {
                    continue;
                }
                let is_sel = selected == Some(r.id.as_str());
                let (rect, resp) = reorder.row(ui, index, |ui| {
                    ui.allocate_exact_size(
                        egui::vec2(ui.available_width(), 26.0),
                        egui::Sense::click(),
                    )
                });
                if is_sel || resp.hovered() {
                    ui.painter().rect_filled(
                        rect,
                        egui::CornerRadius::same(4),
                        if is_sel {
                            theme.accent.linear_multiply(0.30)
                        } else {
                            theme.surface_hover
                        },
                    );
                }
                ui.painter().text(
                    egui::pos2(rect.left() + 8.0, rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    &r.method,
                    egui::FontId::monospace(10.5),
                    method_colour(theme, &r.method),
                );
                ui.painter().text(
                    egui::pos2(rect.left() + 64.0, rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    &r.name,
                    egui::FontId::proportional(13.0),
                    theme.text,
                );
                resp.widget_info(|| {
                    egui::WidgetInfo::labeled(
                        egui::WidgetType::Button,
                        true,
                        format!("{} {}", r.method, r.name),
                    )
                });
                if resp.clicked() {
                    actions.push(SideAction::Open(collection.into(), r.id.clone()));
                }
                resp.context_menu(|ui| {
                    if ui.button(s.t("api-rename")).clicked() {
                        actions.push(SideAction::Rename(
                            collection.into(),
                            r.id.clone(),
                            r.name.clone(),
                        ));
                        ui.close();
                    }
                    if ui.button(s.t("api-duplicate")).clicked() {
                        actions.push(SideAction::Duplicate(collection.into(), r.id.clone()));
                        ui.close();
                    }
                    ui.separator();
                    if ui.button(s.t("api-delete-request")).clicked() {
                        actions.push(SideAction::Delete(collection.into(), Some(r.id.clone())));
                        ui.close();
                    }
                });
            }
        }
    }
    if let Some(mv) = reorder.finish_move(ui, theme.accent) {
        use hs_ui::reorder::Move;
        let (from_collection, from_folder, from) = match &mv {
            Move::Within { from, .. } => {
                (collection.to_string(), folder.map(str::to_string), *from)
            }
            Move::From { scope, from, .. } => {
                let (c, f) = from_scope(scope);
                (c, f, *from)
            }
        };
        let to = match mv {
            Move::Within { to, .. } | Move::From { to, .. } => to,
        };
        actions.push(SideAction::MoveItem {
            from_collection,
            from_folder,
            from,
            to_collection: collection.to_string(),
            to_folder: folder.map(str::to_string),
            to,
        });
    }
}

/// Every request id an item holds, itself included: a folder that moves to
/// another collection takes its requests with it, and the open tab has to
/// follow whichever of them it is showing.
fn request_ids(item: &Item) -> Vec<String> {
    match item {
        Item::Request(r) => vec![r.id.clone()],
        Item::Folder(f) => f.items.iter().flat_map(request_ids).collect(),
    }
}

/// How one level of the tree names itself in a drag payload. The unit
/// separator cannot appear in an id, so the two halves always split back
/// out cleanly.
/// The request's name, edited in place. The rename reaches the collection
/// the moment the field loses focus or Enter is pressed, so the list and
/// the tab follow without a separate Save; a name already used beside it
/// is numbered, the way a copy or a dragged-in row is.
fn name_row(t: &mut ApiTool, ui: &mut egui::Ui, cx: &mut ToolContext, theme: &Theme) {
    let s = cx.services;
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(s.t("api-request-name"))
                .size(11.0)
                .color(theme.text_muted),
        );
        let r = ui.add(
            egui::TextEdit::singleline(&mut t.request.name)
                .hint_text(s.t("api-request-name-hint"))
                .desired_width(f32::INFINITY),
        );
        // Enter counts only while this field has the focus: a tab-wide
        // Enter check wrote the collection file on every newline typed in
        // the body editor.
        let done =
            r.lost_focus() || (r.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
        if !(r.changed() || done) {
            return;
        }
        if !done {
            // Still typing: the tab title follows live, the file waits.
            return;
        }
        let Some((collection, id)) = t.state_collection().zip(t.state_request_id()) else {
            // An unsaved draft has no row to rename; the name travels with
            // it when it is saved.
            return;
        };
        let typed = t.request.name.trim().to_string();
        if typed.is_empty() {
            // Blank is not a name. Put back whatever the file still says.
            if let Some(saved) = t.saved.as_ref().map(|r| r.name.clone()) {
                t.request.name = saved;
            }
            return;
        }
        let taken: Vec<String> = t
            .sibling_names_excluding(&collection, &id)
            .into_iter()
            .collect();
        let free = crate::model::free_name(&typed, &taken);
        t.request.name = free.clone();
        if let Some(col) = t.collections.iter_mut().find(|c| c.id == collection)
            && let Some(req) = col.find_mut(&id)
        {
            req.name = free;
        }
        t.save_collection(&collection, cx);
        if let Some(saved) = t.saved.as_mut() {
            saved.name = t.request.name.clone();
        }
    });
}

/// The confirmed Delete: a request or folder (`Some(id)`) or the whole
/// collection (`None`).
fn delete_now(t: &mut ApiTool, c: &str, item: Option<&str>, cx: &mut ToolContext) {
    match item {
        Some(id) => {
            if let Some(col) = t.collections.iter_mut().find(|x| x.id == c) {
                col.remove(id);
            }
            t.save_collection(c, cx);
            // The open request may have been the item itself or anything
            // inside a deleted folder: either way its row is gone, and the
            // next Ctrl+S must not say "Saved".
            let open_gone = t.state_request_id().is_some_and(|open| {
                open == id
                    || t.collections
                        .iter()
                        .find(|x| x.id == c)
                        .is_some_and(|col| col.find(&open).is_none())
            });
            if open_gone {
                t.new_request();
            }
        }
        None => {
            let _ = std::fs::remove_file(Collection::path(&t.collections_dir(), c));
            t.collections.retain(|x| x.id != c);
            if t.state_collection().as_deref() == Some(c) {
                t.new_request();
            }
            // A deleted collection has no file left to save, so the other
            // tabs have to be told separately.
            t.announce(cx);
        }
    }
}

/// Which folder of `col` holds `id` (`None` = the collection's root).
fn parent_folder(col: &Collection, id: &str) -> Option<String> {
    col.parent_folder_of(id).flatten()
}

fn scope_of(collection: &str, folder: Option<&str>) -> String {
    format!("{collection}\u{1}{}", folder.unwrap_or(""))
}

fn from_scope(scope: &str) -> (String, Option<String>) {
    match scope.split_once('\u{1}') {
        Some((c, "")) => (c.to_string(), None),
        Some((c, f)) => (c.to_string(), Some(f.to_string())),
        None => (scope.to_string(), None),
    }
}

fn status_colour(theme: &Theme, status: u16) -> egui::Color32 {
    match status {
        200..=299 => theme.success,
        300..=399 => theme.secondary,
        400..=499 => theme.warning,
        500..=599 => theme.error,
        _ => theme.text_muted,
    }
}

// ---------------------------------------------------------------------------
// Main area

/// The API client's home tab: whose workspace this is, what it holds, and
/// what to do next. With no collections yet it is only the last of those,
/// because a summary of nothing is not worth reading.
/// Owner's requirement, 2026-10-17.
pub fn home(t: &mut ApiTool, ui: &mut egui::Ui, cx: &mut ToolContext, theme: &Theme) {
    let s = cx.services;
    let (name, root) = {
        let w = s.workspace.read();
        (w.meta.name.clone(), w.root().to_path_buf())
    };
    let requests: usize = t.collections.iter().map(|c| count_requests(&c.items)).sum();
    let folders: usize = t.collections.iter().map(|c| count_folders(&c.items)).sum();
    let tabs = s.open_tabs.of("api");
    // The workspace's own logo beside its name, so the tab says whose
    // requests these are rather than only naming the tool.
    ui.horizontal(|ui| {
        match hs_core::workspace::workspace_icon_file(&root).and_then(|f| std::fs::read(f).ok()) {
            Some(bytes) => {
                let uri = format!("bytes://api-home-logo/{name}");
                ui.ctx().include_bytes(uri.clone(), bytes);
                ui.add(egui::Image::from_uri(uri).fit_to_exact_size(egui::Vec2::splat(28.0)));
            }
            None => {
                ui.label(
                    egui::RichText::new(hs_ui::icons::WORKSPACE)
                        .size(24.0)
                        .color(theme.text_muted),
                );
            }
        }
        ui.add_space(8.0);
        ui.vertical(|ui| {
            ui.label(egui::RichText::new(&name).strong().size(20.0));
            ui.label(
                egui::RichText::new(s.t("api-dashboard-sub"))
                    .size(12.0)
                    .color(theme.text_muted),
            );
        });
    });
    ui.add_space(10.0);
    let empty = t.collections.is_empty();
    if !empty {
        ui.horizontal(|ui| {
            hs_ui::dashboard::stat(
                ui,
                theme,
                &t.collections.len().to_string(),
                &s.t("dash-collections"),
            );
            hs_ui::dashboard::stat(ui, theme, &folders.to_string(), &s.t("api-folders"));
            hs_ui::dashboard::stat(ui, theme, &requests.to_string(), &s.t("dash-requests"));
            hs_ui::dashboard::stat(ui, theme, &tabs.len().to_string(), &s.t("dash-open-tabs"));
        });
        ui.add_space(4.0);
    }
    // What to do next. With nothing saved these are the whole page; with
    // collections they sit under the summary.
    let mut action: Option<SideAction> = None;
    hs_ui::form::section(ui, theme, &s.t("api-home-next"), |ui| {
        ui.horizontal_wrapped(|ui| {
            if ui
                .button(format!("{} {}", ph::FOLDER_PLUS, s.t("api-new-collection")))
                .clicked()
            {
                action = Some(SideAction::NewCollection);
            }
            if !empty
                && ui
                    .button(format!("{} {}", ph::PLUS, s.t("api-new-request")))
                    .clicked()
                && let Some((c, folder)) = t.home_for_new_request()
            {
                action = Some(SideAction::NewRequest(c, folder));
            }
            if ui
                .button(format!("{} {}", ph::DOWNLOAD_SIMPLE, s.t("api-import-tip")))
                .clicked()
            {
                action = Some(SideAction::Import);
            }
        });
        if empty {
            hs_ui::form::hint(ui, theme, &s.t("api-home-empty-hint"));
        }
    });
    if let Some(a) = action {
        apply_side_actions(t, vec![a], cx);
    }
    let labels = hs_ui::dashboard::DashboardLabels {
        open_heading: &s.t("dash-open"),
        nothing_open: &s.t("dash-nothing-open"),
        no_preview: &s.t("dash-no-preview"),
    };
    egui::ScrollArea::vertical()
        .id_salt("api-home")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            if let Some(id) = hs_ui::dashboard::open_tabs(ui, theme, &tabs, &labels) {
                cx.actions
                    .push(hs_core::tool::Action::FocusTab { tab_id: id });
            }
        });
    // The dashboard has its own New Collection button and the sidebar's
    // folder-plus works here too, so the dialog has to be drawn on this tab.
    // `Tool::ui` returns right after `home`, and the request view is the only
    // other caller of `dialogs`, which left the button looking dead.
    dialogs(t, ui, cx, theme);
}

/// How many requests a tree holds, at any depth.
fn count_requests(items: &[Item]) -> usize {
    items
        .iter()
        .map(|i| match i {
            Item::Request(_) => 1,
            Item::Folder(f) => count_requests(&f.items),
        })
        .sum()
}

/// How many folders a tree holds, at any depth.
fn count_folders(items: &[Item]) -> usize {
    items
        .iter()
        .map(|i| match i {
            Item::Request(_) => 0,
            Item::Folder(f) => 1 + count_folders(&f.items),
        })
        .sum()
}

pub fn main(t: &mut ApiTool, ui: &mut egui::Ui, cx: &mut ToolContext, theme: &Theme) {
    let s = cx.services;
    dialogs(t, ui, cx, theme);
    if let Some(text) = t.copy_pending.take() {
        ui.ctx().copy_text(text);
    }
    if let Some((level, text)) = t.copy_pending_toast.take() {
        cx.toast(level, text);
    }
    // URL row.
    let vars: Vec<(String, String)> = {
        let mut v = s.environments.read().available_vars(None);
        v.extend(
            t.globals
                .values
                .iter()
                .filter(|g| g.enabled)
                .map(|g| (g.key.clone(), g.value.clone())),
        );
        v
    };
    let mut send = false;
    let mut save = false;
    // The request's name, above the action bar and editable in place. It
    // used to be reachable only through the list's Rename dialog, which is
    // a long way to go for the one piece of text that names the tab, the
    // row in the list and the thing being sent. Owner's requirement,
    // 2026-10-17.
    name_row(t, ui, cx, theme);
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        // The picker is sized to the URL field beside it: a ComboBox is
        // shorter than a text field by default, and the two frames then
        // sit at different heights however the row is aligned. Owner's
        // report, 2026-10-17.
        form::field_row(ui, |ui| {
            egui::ComboBox::from_id_salt("api-method")
                .width(96.0)
                .height(260.0)
                .selected_text(
                    egui::RichText::new(&t.request.method)
                        .strong()
                        .color(method_colour(theme, &t.request.method)),
                )
                .show_ui(ui, |ui| {
                    for m in METHODS {
                        if ui.selectable_label(t.request.method == m, m).clicked() {
                            t.request.method = m.into();
                        }
                    }
                });
        });
        let w = (ui.available_width() - 262.0).max(200.0);
        let out = hs_ui::varfield::var_field_width(
            ui,
            theme,
            "api-url",
            &mut t.request.url,
            &s.t("api-url-hint"),
            &vars,
            w,
        );
        hs_ui::varpeek::var_peek(ui, theme, s, "api-url-peek", &t.request.url);
        if let Some((k, v)) = out.add_var
            && let Err(e) = s.environments.write().set_var(None, &k, &v)
        {
            cx.toast(ToastLevel::Error, e.to_string());
        }
        if out.response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            send = true;
        }
        if t.pending.is_some() {
            ui.spinner();
            if ui.button(s.t("api-cancel")).clicked() {
                t.cancel();
            }
        } else if form::primary_button(
            ui,
            theme,
            &format!("{} {}", ph::PAPER_PLANE_RIGHT, s.t("api-send")),
        )
        .clicked()
        {
            send = true;
        }
        let dirty = t.is_dirty();
        if ui
            .button(format!(
                "{} {}{}",
                ph::FLOPPY_DISK,
                s.t("save"),
                if dirty { " •" } else { "" }
            ))
            .clicked()
        {
            save = true;
        }
        ui.menu_button("⋯", |ui| {
            if ui.button(s.t("api-copy-curl")).clicked() {
                cx.actions.push(hs_core::tool::Action::RunCommand {
                    id: "api.copy_curl".into(),
                });
                ui.close();
            }
            if ui.button(s.t("api-import-curl")).clicked() {
                t.curl_import = Some(String::new());
                ui.close();
            }
            if ui.button(s.t("api-save-as")).clicked() {
                if t.collections.is_empty() {
                    t.new_collection = Some(String::new());
                } else {
                    t.save_to = Some(SaveTo {
                        collection: t.collections[0].id.clone(),
                        folder: None,
                        name: t.request.name.clone(),
                    });
                }
                ui.close();
            }
        });
    });
    ui.add_space(4.0);
    // Request editor above, response below (resizable).
    let total = ui.available_height();
    let resp_h = (total * 0.45).max(120.0);
    egui::Panel::bottom(ui.id().with("api-response"))
        .resizable(true)
        .default_size(resp_h)
        .size_range(80.0..=(total - 120.0).max(80.0))
        .show(ui, |ui| {
            response_panel(t, ui, s, theme);
        });
    request_editor(t, ui, cx, theme);
    if send {
        t.request_send(cx);
    }
    if save {
        t.save(cx);
    }
}

fn request_editor(t: &mut ApiTool, ui: &mut egui::Ui, cx: &mut ToolContext, theme: &Theme) {
    let s = cx.services;
    let counts = [
        t.request
            .params
            .iter()
            .filter(|p| p.enabled && !p.key.is_empty())
            .count(),
        t.request
            .headers
            .iter()
            .filter(|p| p.enabled && !p.key.is_empty())
            .count(),
        usize::from(t.request.body.kind != BodyKind::None),
        usize::from(t.request.auth.kind != AuthKind::Inherit),
        usize::from(
            !t.request.pre_script.trim().is_empty() || !t.request.post_script.trim().is_empty(),
        ),
        usize::from(!t.request.docs.trim().is_empty()),
    ];
    let labels = [
        "api-tab-params",
        "api-tab-headers",
        "api-tab-body",
        "api-tab-auth",
        "api-tab-scripts",
        "api-tab-docs",
    ];
    ui.horizontal(|ui| {
        for (i, key) in labels.iter().enumerate() {
            let text = if counts[i] > 0 && i < 2 {
                format!("{} ({})", s.t(key), counts[i])
            } else if counts[i] > 0 {
                format!("{} •", s.t(key))
            } else {
                s.t(key)
            };
            if ui.selectable_label(t.req_tab() == i, text).clicked() {
                t.set_req_tab(i);
            }
        }
    });
    ui.separator();
    egui::ScrollArea::vertical()
        .id_salt("api-request-body")
        .auto_shrink([false, false])
        .show(ui, |ui| match t.req_tab() {
            0 => kv_table(ui, theme, s, &mut t.request.params, "params"),
            1 => kv_table(ui, theme, s, &mut t.request.headers, "headers"),
            2 => body_tab(t, ui, s, theme),
            3 => auth_tab(t, ui, s, theme),
            4 => scripts_tab(t, ui, s, theme),
            _ => {
                ui.add(
                    egui::TextEdit::multiline(&mut t.request.docs)
                        .hint_text(s.t("api-docs-hint"))
                        .desired_width(f32::INFINITY)
                        .desired_rows(10),
                );
            }
        });
}

fn body_tab(t: &mut ApiTool, ui: &mut egui::Ui, s: &Services, theme: &Theme) {
    ui.horizontal(|ui| {
        for k in BodyKind::ALL {
            if ui
                .selectable_label(t.request.body.kind == k, k.label())
                .clicked()
                && t.request.body.kind != k
            {
                t.request.body.kind = k;
                let text = t.body_view.text();
                // Switching to JSON or XML lays out what is there.
                t.body_view.set_text_pretty(&text, k);
            }
        }
        if !matches!(t.request.body.kind, BodyKind::None | BodyKind::Binary)
            && ui
                .small_button(format!("{} {}", ph::DOWNLOAD_SIMPLE, s.t("download")))
                .on_hover_text(s.t("api-body-download-tip"))
                .clicked()
        {
            let ext = match t.request.body.kind {
                BodyKind::Json => "json",
                BodyKind::Xml => "xml",
                _ => "txt",
            };
            let name = format!("{}.{ext}", hs_core::download::safe_name(&t.request.name));
            let text = t.body_view.text();
            match hs_core::download::save_bytes(&name, text.as_bytes()) {
                Ok(Some(p)) => {
                    t.copy_pending_toast = Some((
                        ToastLevel::Success,
                        s.i18n
                            .t_args("download-saved", &[("path", &p.display().to_string())]),
                    ))
                }
                Ok(None) => {}
                Err(e) => t.copy_pending_toast = Some((ToastLevel::Error, e)),
            }
        }
        if matches!(t.request.body.kind, BodyKind::Json | BodyKind::Xml)
            && ui
                .small_button(s.t("api-body-format"))
                .on_hover_text(s.t("api-body-format-tip"))
                .clicked()
        {
            let text = t.body_view.text();
            let kind = t.request.body.kind;
            let pretty = match kind {
                BodyKind::Xml => hs_core::fmt::pretty_xml(&text),
                _ => hs_core::fmt::pretty_json(&text),
            };
            match pretty {
                Some(p) => t.body_view.set_text(&p, kind),
                // Malformed, or already laid out: say so rather than
                // silently doing nothing.
                None => {
                    t.copy_pending_toast = Some((ToastLevel::Info, s.t("api-body-format-failed")));
                }
            }
        }
    });
    ui.add_space(4.0);
    match t.request.body.kind {
        BodyKind::None => {
            form::hint(ui, theme, &s.t("api-body-none"));
        }
        BodyKind::Json | BodyKind::Text | BodyKind::Xml => {
            let h = ui.available_height().max(160.0);
            ui.allocate_ui(egui::vec2(ui.available_width(), h), |ui| {
                t.body_view.show(ui, theme);
            });
            t.request.body.text = t.body_view.text();
        }
        BodyKind::Form => kv_table(ui, theme, s, &mut t.request.body.form, "form"),
        BodyKind::Multipart => {
            kv_table(ui, theme, s, &mut t.request.body.form, "multipart");
            if ui
                .button(format!("{} {}", ph::FOLDER_OPEN, s.t("api-add-file-part")))
                .clicked()
                && let Some(p) = rfd::FileDialog::new().pick_file()
            {
                t.request.body.form.push(Kv {
                    key: "file".into(),
                    value: p.display().to_string(),
                    description: "file:".into(),
                    enabled: true,
                });
            }
            form::hint(ui, theme, &s.t("api-multipart-hint"));
        }
        BodyKind::Binary => {
            ui.horizontal(|ui| {
                ui.label(&t.request.body.file);
                if ui.button(s.t("api-choose-file")).clicked()
                    && let Some(p) = rfd::FileDialog::new().pick_file()
                {
                    t.request.body.file = p.display().to_string();
                }
            });
        }
    }
}

fn auth_tab(t: &mut ApiTool, ui: &mut egui::Ui, s: &Services, theme: &Theme) {
    auth_editor(&mut t.request.auth, ui, s, theme, "api-auth");
}

/// The authentication form, for a request and for a collection.
fn auth_editor(a: &mut Auth, ui: &mut egui::Ui, s: &Services, theme: &Theme, id: &str) {
    ui.horizontal(|ui| {
        ui.label(s.t("api-auth-type"));
        egui::ComboBox::from_id_salt(id)
            .selected_text(a.kind.label())
            .show_ui(ui, |ui| {
                for k in AuthKind::ALL {
                    ui.selectable_value(&mut a.kind, k, k.label());
                }
            });
    });
    ui.add_space(6.0);
    form::field_grid(ui, (id, "grid"), |ui| match a.kind {
        AuthKind::Basic => {
            form::label(ui, &s.t("api-auth-user"));
            form::text_field(ui, &mut a.username, "");
            ui.end_row();
            form::label(ui, &s.t("api-auth-pass"));
            form::secret_field(ui, &mut a.password, "{{password}}", 360.0);
            ui.end_row();
        }
        AuthKind::Bearer => {
            form::label(ui, &s.t("api-auth-token"));
            form::text_field(ui, &mut a.token, "{{token}}");
            ui.end_row();
        }
        AuthKind::ApiKey => {
            form::label(ui, &s.t("api-auth-key"));
            form::text_field(ui, &mut a.key, "X-API-Key");
            ui.end_row();
            form::label(ui, &s.t("api-auth-value"));
            form::text_field(ui, &mut a.value, "{{api_key}}");
            ui.end_row();
            form::label(ui, &s.t("api-auth-add-to"));
            ui.horizontal(|ui| {
                if ui
                    .selectable_label(a.add_to != "query", s.t("api-auth-header"))
                    .clicked()
                {
                    a.add_to = "header".into();
                }
                if ui
                    .selectable_label(a.add_to == "query", s.t("api-auth-query"))
                    .clicked()
                {
                    a.add_to = "query".into();
                }
            });
            ui.end_row();
        }
        AuthKind::OAuth2 => {
            form::label(ui, &s.t("api-auth-token-url"));
            form::text_field(ui, &mut a.token_url, "https://auth.example.com/oauth/token");
            ui.end_row();
            form::label(ui, &s.t("api-auth-grant"));
            if a.grant.is_empty() {
                a.grant = "client_credentials".into();
            }
            egui::ComboBox::from_id_salt((id, "grant"))
                .selected_text(s.t(match a.grant.as_str() {
                    "password" => "api-auth-grant-password",
                    "refresh_token" => "api-auth-grant-refresh",
                    _ => "api-auth-grant-client",
                }))
                .show_ui(ui, |ui| {
                    for (value, key) in [
                        ("client_credentials", "api-auth-grant-client"),
                        ("password", "api-auth-grant-password"),
                        ("refresh_token", "api-auth-grant-refresh"),
                    ] {
                        ui.selectable_value(&mut a.grant, value.to_string(), s.t(key));
                    }
                });
            ui.end_row();
            form::label(ui, &s.t("api-auth-client-id"));
            form::text_field(ui, &mut a.client_id, "{{client_id}}");
            ui.end_row();
            form::label(ui, &s.t("api-auth-client-secret"));
            form::secret_field(ui, &mut a.client_secret, "{{client_secret}}", 360.0);
            ui.end_row();
            if a.grant == "password" {
                form::label(ui, &s.t("api-auth-user"));
                form::text_field(ui, &mut a.username, "");
                ui.end_row();
                form::label(ui, &s.t("api-auth-pass"));
                form::secret_field(ui, &mut a.password, "{{password}}", 360.0);
                ui.end_row();
            }
            if a.grant == "refresh_token" {
                form::label(ui, &s.t("api-auth-refresh-token"));
                form::secret_field(ui, &mut a.token, "{{refresh_token}}", 360.0);
                ui.end_row();
            }
            form::label(ui, &s.t("api-auth-scope"));
            form::text_field(ui, &mut a.scope, "read write");
            ui.end_row();
            form::label(ui, &s.t("api-auth-audience"));
            form::text_field(ui, &mut a.audience, "");
            ui.end_row();
            form::label(ui, "");
            if ui.button(s.t("api-auth-refresh-now")).clicked() {
                crate::send::forget_tokens();
            }
            ui.end_row();
        }
        AuthKind::AwsSigV4 => {
            form::label(ui, &s.t("api-auth-access-key"));
            form::text_field(ui, &mut a.access_key, "{{aws_access_key_id}}");
            ui.end_row();
            form::label(ui, &s.t("api-auth-secret-key"));
            form::secret_field(ui, &mut a.secret_key, "{{aws_secret_access_key}}", 360.0);
            ui.end_row();
            form::label(ui, &s.t("api-auth-session-token"));
            form::secret_field(ui, &mut a.session_token, "", 360.0);
            ui.end_row();
            form::label(ui, &s.t("api-auth-region"));
            form::text_field(ui, &mut a.region, "us-east-1");
            ui.end_row();
            form::label(ui, &s.t("api-auth-service"));
            form::text_field(ui, &mut a.service, "execute-api");
            ui.end_row();
        }
        _ => {}
    });
    form::hint(
        ui,
        theme,
        &s.t(match a.kind {
            AuthKind::OAuth2 => "api-auth-hint-oauth2",
            AuthKind::AwsSigV4 => "api-auth-hint-sigv4",
            _ => "api-auth-hint",
        }),
    );
}

const SNIPPETS: [(&str, &str); 7] = [
    (
        "Status is 200",
        "pm.test(\"status is 200\", () => pm.response.to.have.status(200));",
    ),
    (
        "Response is JSON",
        "pm.test(\"json body\", () => pm.response.to.have.jsonBody());",
    ),
    (
        "Save a value to the environment",
        "const data = pm.response.json();\npm.environment.set(\"token\", data.token);",
    ),
    (
        "Save a value to globals",
        "pm.globals.set(\"id\", pm.response.json().id);",
    ),
    (
        "Check a JSON field",
        "pm.test(\"has id\", () => pm.expect(pm.response.json().id).to.exist);",
    ),
    (
        "Response time under 500 ms",
        "pm.test(\"fast\", () => pm.expect(pm.response.responseTime).to.be.below(500));",
    ),
    (
        "Set a header before sending",
        "pm.request.headers.add({ key: \"X-Trace\", value: pm.variables.get(\"trace\") || \"1\" });",
    ),
];

fn scripts_tab(t: &mut ApiTool, ui: &mut egui::Ui, s: &Services, theme: &Theme) {
    form::hint(ui, theme, &s.t("api-scripts-hint"));
    // The two scripts get the same height whatever is in them: a multiline
    // field grows with its content, so the one that had been written in
    // squeezed the empty one down to nothing. Owner's requirement,
    // 2026-10-17.
    const ROW_CHROME: f32 = 34.0; // the label row and the gap under it
    let each = ((ui.available_height() - ROW_CHROME * 2.0) / 2.0).max(90.0);
    let libraries: Vec<String> = t
        .libraries
        .iter()
        .filter(|l| l.enabled)
        .map(|l| l.name.clone())
        .collect();
    for (label, field, key) in [
        ("api-pre-script", 0usize, "api-pre-script-hint"),
        ("api-post-script", 1usize, "api-post-script-hint"),
    ] {
        let target = if field == 0 {
            &mut t.request.pre_script
        } else {
            &mut t.request.post_script
        };
        script_editor(
            ui,
            s,
            label,
            key,
            ("api-script", field),
            each,
            target,
            &libraries,
        );
    }
}

/// One script field with its Snippets menu (snippets, then the package
/// libraries, which insert the `// @library <name>` line that pulls them
/// in). Shared by the request tab and the collection's Manage page.
#[allow(clippy::too_many_arguments)]
fn script_editor(
    ui: &mut egui::Ui,
    s: &Services,
    label_key: &str,
    hint_key: &str,
    id: impl std::hash::Hash + std::fmt::Debug,
    height: f32,
    target: &mut String,
    libraries: &[String],
) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(s.t(label_key)).strong());
        ui.menu_button(format!("{} {}", ph::CODE, s.t("api-snippets")), |ui| {
            for (name, code) in SNIPPETS {
                if ui.button(name).clicked() {
                    if !target.is_empty() && !target.ends_with('\n') {
                        target.push('\n');
                    }
                    target.push_str(code);
                    target.push('\n');
                    ui.close();
                }
            }
            if !libraries.is_empty() {
                ui.separator();
                ui.menu_button(s.t("api-insert-library"), |ui| {
                    for name in libraries {
                        if ui.button(name).clicked() {
                            insert_library_directive(target, name);
                            ui.close();
                        }
                    }
                });
            }
        });
    });
    // Rows rather than a bare height, so the field itself fills its
    // half instead of sitting at the top of an empty box.
    let line = ui.text_style_height(&egui::TextStyle::Monospace);
    let rows = ((height / line).floor() as usize).max(4);
    egui::ScrollArea::vertical()
        .id_salt(id)
        .max_height(height)
        .show(ui, |ui| {
            ui.add(
                egui::TextEdit::multiline(target)
                    .hint_text(s.t(hint_key))
                    .font(egui::TextStyle::Monospace)
                    .desired_width(f32::INFINITY)
                    .desired_rows(rows),
            );
        });
    ui.add_space(6.0);
}

/// Put `// @library <name>` at the top of a script, once.
fn insert_library_directive(target: &mut String, name: &str) {
    let line = format!("{}{name}", crate::model::LIBRARY_DIRECTIVE);
    if target.lines().any(|l| l.trim() == line) {
        return;
    }
    let rest = std::mem::take(target);
    *target = if rest.is_empty() {
        format!("{line}\n")
    } else {
        format!("{line}\n{rest}")
    };
}

fn response_panel(t: &mut ApiTool, ui: &mut egui::Ui, s: &Services, theme: &Theme) {
    let resp_tab = t.state.resp_tab;
    let mut new_tab = None;
    let Some(last) = &t.last else {
        if t.pending.is_some() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(s.t("api-sending"));
            });
        } else {
            form::hint(ui, theme, &s.t("api-response-empty"));
        }
        return;
    };
    match &last.exchange {
        Err(e) => {
            ui.colored_label(theme.error, format!("{} {e}", hs_ui::icons::ERROR));
            if let Some(err) = &last.script_error {
                ui.colored_label(theme.error, format!("{} {err}", s.t("api-script-error")));
            }
            if !last.logs.is_empty() {
                console(ui, theme, &last.logs);
            }
        }
        Ok(ex) => {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(format!("{} {}", ex.status, ex.status_text))
                        .strong()
                        .color(status_colour(theme, ex.status)),
                );
                ui.label(
                    egui::RichText::new(format!(
                        "{} ms · {} · {}{}",
                        ex.millis,
                        human_size(ex.body.len()),
                        ex.version,
                        if ex.remote.is_empty() {
                            String::new()
                        } else {
                            format!(" · {}", ex.remote)
                        }
                    ))
                    .color(theme.text_muted),
                );
                let passed = last.tests.iter().filter(|x| x.passed).count();
                if !last.tests.is_empty() {
                    let all = passed == last.tests.len();
                    ui.label(
                        egui::RichText::new(format!(
                            "{} {passed}/{}",
                            if all {
                                hs_ui::icons::CHECK
                            } else {
                                hs_ui::icons::ERROR
                            },
                            last.tests.len()
                        ))
                        .color(if all {
                            theme.success
                        } else {
                            theme.error
                        }),
                    );
                }
                if last.script_error.is_some() {
                    ui.colored_label(theme.error, s.t("api-script-error"));
                }
            });
            let cookies = ex
                .headers
                .iter()
                .filter(|(k, _)| k.eq_ignore_ascii_case("set-cookie"))
                .count();
            ui.horizontal(|ui| {
                for (i, (key, n)) in [
                    ("api-resp-body", 0usize),
                    ("api-resp-headers", ex.headers.len()),
                    ("api-resp-cookies", cookies),
                    ("api-resp-tests", last.tests.len()),
                    ("api-resp-console", last.logs.len()),
                ]
                .iter()
                .enumerate()
                {
                    let text = if *n > 0 && i > 0 {
                        format!("{} ({n})", s.t(key))
                    } else {
                        s.t(key)
                    };
                    if ui.selectable_label(resp_tab == i, text).clicked() {
                        new_tab = Some(i);
                    }
                }
            });
            ui.separator();
            match resp_tab {
                0 => t.resp_view.show(ui, s, theme),
                1 => {
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            egui::Grid::new("api-resp-headers")
                                .striped(true)
                                .num_columns(2)
                                .show(ui, |ui| {
                                    for (k, v) in &ex.headers {
                                        ui.label(egui::RichText::new(k).monospace().strong());
                                        ui.add(
                                            egui::Label::new(egui::RichText::new(v).monospace())
                                                .wrap(),
                                        );
                                        ui.end_row();
                                    }
                                });
                        });
                }
                2 => {
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            if cookies == 0 {
                                form::hint(ui, theme, &s.t("api-no-cookies"));
                            }
                            for (_, v) in ex
                                .headers
                                .iter()
                                .filter(|(k, _)| k.eq_ignore_ascii_case("set-cookie"))
                            {
                                ui.add(egui::Label::new(egui::RichText::new(v).monospace()).wrap());
                            }
                        });
                }
                3 => {
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            if let Some(err) = &last.script_error {
                                ui.colored_label(theme.error, err);
                            }
                            if last.tests.is_empty() {
                                form::hint(ui, theme, &s.t("api-no-tests"));
                            }
                            for tr in &last.tests {
                                ui.horizontal(|ui| {
                                    ui.colored_label(
                                        if tr.passed {
                                            theme.success
                                        } else {
                                            theme.error
                                        },
                                        if tr.passed {
                                            hs_ui::icons::CHECK
                                        } else {
                                            hs_ui::icons::ERROR
                                        },
                                    );
                                    ui.label(&tr.name);
                                    if !tr.passed {
                                        ui.colored_label(theme.text_muted, &tr.message);
                                    }
                                });
                            }
                        });
                }
                _ => console(ui, theme, &last.logs),
            }
        }
    }
    if let Some(i) = new_tab {
        t.state.resp_tab = i;
    }
}

fn console(ui: &mut egui::Ui, theme: &Theme, logs: &[String]) {
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            if logs.is_empty() {
                ui.colored_label(theme.text_muted, "—");
            }
            for l in logs {
                ui.add(egui::Label::new(egui::RichText::new(l).monospace()).wrap());
            }
        });
}

fn dialogs(t: &mut ApiTool, ui: &mut egui::Ui, cx: &mut ToolContext, theme: &Theme) {
    let s = cx.services;
    let ctx = ui.ctx().clone();
    // Protected environment confirmation.
    if let Some(env) = t.confirm_send.clone() {
        match hs_ui::dialog::confirm(
            &ctx,
            theme,
            "api-protected",
            &s.t("api-protected-title"),
            &s.i18n.t_args(
                "api-protected-body",
                &[
                    ("env", &env),
                    ("method", &t.request.method),
                    ("url", &t.request.url),
                ],
            ),
            &s.t("api-send"),
            &s.t("cancel"),
            true,
        ) {
            hs_ui::dialog::DialogResult::Ok => {
                t.confirm_send = None;
                t.send(cx);
            }
            hs_ui::dialog::DialogResult::Cancel => t.confirm_send = None,
            hs_ui::dialog::DialogResult::Open => {}
        }
    }
    // Delete confirmation (collection, folder or request).
    if let Some((c, item)) = t.confirm_delete.clone() {
        let (title, body) = match &item {
            None => (
                s.t("api-delete-collection"),
                s.t("api-delete-body-collection"),
            ),
            Some(id) => {
                let is_folder = t
                    .collections
                    .iter()
                    .find(|x| x.id == c)
                    .is_some_and(|col| col.find(id).is_none());
                if is_folder {
                    (s.t("api-delete-folder"), s.t("api-delete-body-folder"))
                } else {
                    (s.t("api-delete-request"), s.t("api-delete-body-request"))
                }
            }
        };
        match hs_ui::dialog::confirm(
            &ctx,
            theme,
            "api-delete",
            &title,
            &body,
            &s.t("api-delete"),
            &s.t("cancel"),
            true,
        ) {
            hs_ui::dialog::DialogResult::Ok => {
                t.confirm_delete = None;
                delete_now(t, &c, item.as_deref(), cx);
            }
            hs_ui::dialog::DialogResult::Cancel => t.confirm_delete = None,
            hs_ui::dialog::DialogResult::Open => {}
        }
    }
    // Delete a package library.
    if let Some(id) = t.confirm_delete_library.clone() {
        match hs_ui::dialog::confirm(
            &ctx,
            theme,
            "api-delete-library",
            &s.t("api-library-delete"),
            &s.t("api-library-delete-body"),
            &s.t("api-delete"),
            &s.t("cancel"),
            true,
        ) {
            hs_ui::dialog::DialogResult::Ok => {
                t.confirm_delete_library = None;
                t.delete_library(&id, cx);
                for c in 0..t.collections.len() {
                    let cid = t.collections[c].id.clone();
                    t.save_collection(&cid, cx);
                }
            }
            hs_ui::dialog::DialogResult::Cancel => t.confirm_delete_library = None,
            hs_ui::dialog::DialogResult::Open => {}
        }
    }
    // New collection.
    if let Some(name) = &mut t.new_collection {
        let mut done: Option<bool> = None;
        let modal = egui::Modal::new(egui::Id::new("api-new-collection")).show(&ctx, |ui| {
            ui.set_width(380.0);
            ui.heading(s.t("api-new-collection"));
            ui.add_space(6.0);
            let r = form::text_field(ui, name, &s.t("api-collection-name"));
            r.request_focus();
            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                done = Some(true);
            }
            ui.add_space(10.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if form::primary_button(ui, theme, &s.t("create")).clicked() {
                    done = Some(true);
                }
                if ui.button(s.t("cancel")).clicked() {
                    done = Some(false);
                }
            });
        });
        if modal.should_close() && done.is_none() {
            done = Some(false);
        }
        if let Some(ok) = done {
            let name = t.new_collection.take().unwrap_or_default();
            if ok && !name.trim().is_empty() {
                let id = t.add_collection(&name, cx);
                if t.state_request_id().is_none() && !t.request.url.is_empty() {
                    t.save_to = Some(SaveTo {
                        collection: id,
                        folder: None,
                        name: t.request.name.clone(),
                    });
                }
            }
        }
    }
    // Save to a collection / folder.
    if let Some(st) = &mut t.save_to {
        let mut done: Option<bool> = None;
        let cols: Vec<(String, String)> = t
            .collections
            .iter()
            .map(|c| (c.id.clone(), c.name.clone()))
            .collect();
        let folders: Vec<(String, String)> = t
            .collections
            .iter()
            .find(|c| c.id == st.collection)
            .map(|c| {
                fn walk(items: &[Item], prefix: &str, out: &mut Vec<(String, String)>) {
                    for it in items {
                        if let Item::Folder(f) = it {
                            let name = format!("{prefix}{}", f.name);
                            out.push((f.id.clone(), name.clone()));
                            walk(&f.items, &format!("{name} / "), out);
                        }
                    }
                }
                let mut out = Vec::new();
                walk(&c.items, "", &mut out);
                out
            })
            .unwrap_or_default();
        let modal = egui::Modal::new(egui::Id::new("api-save-to")).show(&ctx, |ui| {
            ui.set_width(420.0);
            ui.heading(s.t("api-save-as"));
            ui.add_space(6.0);
            form::field_grid(ui, "api-save-grid", |ui| {
                form::label(ui, &s.t("api-request-name"));
                form::text_field(ui, &mut st.name, "");
                ui.end_row();
                form::label(ui, &s.t("api-collection"));
                egui::ComboBox::from_id_salt("api-save-col")
                    .selected_text(
                        cols.iter()
                            .find(|c| c.0 == st.collection)
                            .map(|c| c.1.clone())
                            .unwrap_or_default(),
                    )
                    .show_ui(ui, |ui| {
                        for (id, name) in &cols {
                            if ui.selectable_label(&st.collection == id, name).clicked() {
                                st.collection = id.clone();
                                st.folder = None;
                            }
                        }
                    });
                ui.end_row();
                form::label(ui, &s.t("api-folder"));
                egui::ComboBox::from_id_salt("api-save-folder")
                    .selected_text(
                        st.folder
                            .as_ref()
                            .and_then(|f| folders.iter().find(|x| &x.0 == f).map(|x| x.1.clone()))
                            .unwrap_or_else(|| s.t("api-folder-root")),
                    )
                    .show_ui(ui, |ui| {
                        if ui
                            .selectable_label(st.folder.is_none(), s.t("api-folder-root"))
                            .clicked()
                        {
                            st.folder = None;
                        }
                        for (id, name) in &folders {
                            if ui
                                .selectable_label(st.folder.as_ref() == Some(id), name)
                                .clicked()
                            {
                                st.folder = Some(id.clone());
                            }
                        }
                    });
                ui.end_row();
            });
            ui.add_space(10.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if form::primary_button(ui, theme, &s.t("save")).clicked() {
                    done = Some(true);
                }
                if ui.button(s.t("cancel")).clicked() {
                    done = Some(false);
                }
            });
        });
        if modal.should_close() && done.is_none() {
            done = Some(false);
        }
        if let Some(ok) = done {
            let st = t.save_to.take().unwrap();
            if ok {
                let name = if st.name.trim().is_empty() {
                    s.t("api-new-request-name")
                } else {
                    st.name.trim().to_string()
                };
                t.save_as(&st.collection, st.folder.as_deref(), &name, cx);
            }
        }
    }
    // Rename.
    if let Some((_target, name)) = &mut t.rename {
        let mut done: Option<bool> = None;
        let modal = egui::Modal::new(egui::Id::new("api-rename")).show(&ctx, |ui| {
            ui.set_width(380.0);
            ui.heading(s.t("api-rename"));
            ui.add_space(6.0);
            let r = form::text_field(ui, name, "");
            r.request_focus();
            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                done = Some(true);
            }
            ui.add_space(10.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if form::primary_button(ui, theme, &s.t("save")).clicked() {
                    done = Some(true);
                }
                if ui.button(s.t("cancel")).clicked() {
                    done = Some(false);
                }
            });
        });
        if modal.should_close() && done.is_none() {
            done = Some(false);
        }
        if let Some(ok) = done {
            let (target, name) = t.rename.take().unwrap();
            if ok
                && !name.trim().is_empty()
                && let Some((cid, id)) = target.split_once('/')
            {
                let (cid, id) = (cid.to_string(), id.to_string());
                let typed = name.trim().to_string();
                // Every rename (collection, folder or request) is checked
                // against its own siblings and renumbered " 001", " 002"...
                // on a clash, the same rule New Collection/New Folder/New
                // Request already apply when creating one.
                let mut applied = typed.clone();
                if cid == id {
                    // Renaming the collection itself: dedupe against every
                    // other collection's name.
                    let siblings = t
                        .collections
                        .iter()
                        .filter(|c| c.id != id)
                        .map(|c| c.name.clone());
                    applied = crate::unique_name(&typed, siblings);
                    if let Some(c) = t.collections.iter_mut().find(|c| c.id == cid) {
                        c.name = applied.clone();
                    }
                } else if let Some(c) = t.collections.iter_mut().find(|c| c.id == cid) {
                    // A request or folder anywhere in the tree: dedupe
                    // against its own siblings, not the whole collection, so
                    // renaming "Foo" back to "Foo" is never flagged as a
                    // clash with itself and a name used in a different
                    // folder never gets suffixed needlessly.
                    let parent = c.parent_folder_of(&id).flatten();
                    let siblings = c
                        .items(parent.as_deref())
                        .map(|items| {
                            items
                                .iter()
                                .filter(|i| i.id() != id)
                                .map(|i| i.name().to_string())
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    applied = crate::unique_name(&typed, siblings.into_iter());
                    if let Some(r) = c.find_mut(&id) {
                        r.name = applied.clone();
                    } else {
                        fn rename_folder(items: &mut [Item], id: &str, name: &str) {
                            for it in items {
                                if let Item::Folder(f) = it {
                                    if f.id == id {
                                        f.name = name.to_string();
                                    }
                                    rename_folder(&mut f.items, id, name);
                                }
                            }
                        }
                        rename_folder(&mut c.items, &id, &applied);
                    }
                }
                t.save_collection(&cid, cx);
                if t.state_request_id().as_deref() == Some(&id) {
                    t.request.name = applied.clone();
                    if let Some(saved) = &mut t.saved {
                        saved.name = applied.clone();
                    }
                }
            }
        }
    }
    // Import curl.
    if let Some(text) = &mut t.curl_import {
        let mut done: Option<bool> = None;
        let modal = egui::Modal::new(egui::Id::new("api-curl")).show(&ctx, |ui| {
            ui.set_width(560.0);
            ui.heading(s.t("api-import-curl"));
            ui.add_space(6.0);
            ui.add(
                egui::TextEdit::multiline(text)
                    .hint_text("curl -X POST https://… -H 'Authorization: Bearer …' -d '{…}'")
                    .font(egui::TextStyle::Monospace)
                    .desired_width(f32::INFINITY)
                    .desired_rows(6),
            );
            ui.add_space(10.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if form::primary_button(ui, theme, &s.t("api-import")).clicked() {
                    done = Some(true);
                }
                if ui.button(s.t("cancel")).clicked() {
                    done = Some(false);
                }
            });
        });
        if modal.should_close() && done.is_none() {
            done = Some(false);
        }
        if let Some(ok) = done {
            let text = t.curl_import.take().unwrap_or_default();
            if ok {
                let handled = t.open(
                    hs_core::tool::OpenRequest::Text {
                        mime: "text/x-curl".into(),
                        body: text,
                    },
                    cx,
                );
                if !handled {
                    cx.toast(ToastLevel::Error, s.t("api-curl-invalid"));
                }
            }
        }
    }
    let _ = Request::default;
}

impl ApiTool {
    pub(crate) fn req_tab(&self) -> usize {
        self.state.req_tab
    }
    pub(crate) fn set_req_tab(&mut self, i: usize) {
        self.state.req_tab = i;
    }
    /// Move a request or folder from one place in the sidebar to another:
    /// the same list, another folder, another collection's root, or a
    /// folder of another collection. `to == usize::MAX` means "at the
    /// end", which is what a drop onto a folder or a collection says.
    ///
    /// Two empty collection ids are the collections themselves, which are
    /// a flat list of their own. Owner's report, 2026-10-18.
    #[allow(clippy::too_many_arguments)]
    pub fn move_item(
        &mut self,
        from_collection: &str,
        from_folder: Option<&str>,
        from: usize,
        to_collection: &str,
        to_folder: Option<&str>,
        to: usize,
        cx: &mut ToolContext,
    ) {
        if from_collection.is_empty() && to_collection.is_empty() {
            hs_ui::reorder::move_item(&mut self.collections, from, to);
            self.save_order(cx);
            return;
        }
        // Inside one collection and one level: a plain reorder, which is
        // the only move where the boundary index means what it says.
        if from_collection == to_collection && from_folder == to_folder {
            if let Some(col) = self
                .collections
                .iter_mut()
                .find(|x| x.id == from_collection)
                && let Some(items) = col.items_mut(to_folder)
            {
                // A drop on the level's own header means "at the end".
                let to = to.min(items.len());
                hs_ui::reorder::move_item(items, from, to);
                self.save_collection(from_collection, cx);
            }
            return;
        }
        // Take it out first, so the insert index is not disturbed by the
        // removal - and so a failed insert can put it back.
        let Some(src) = self
            .collections
            .iter_mut()
            .find(|x| x.id == from_collection)
        else {
            return;
        };
        let Some(mut item) = src
            .items_mut(from_folder)
            .filter(|items| from < items.len())
            .map(|items| items.remove(from))
        else {
            return;
        };
        let moved_ids = request_ids(&item);
        // `None` means it landed; `Some(item)` hands it back untouched.
        let orphan = {
            let dst = self
                .collections
                .iter_mut()
                .find(|x| x.id == to_collection)
                .and_then(|c| c.items_mut(to_folder));
            match dst {
                Some(items) => {
                    // A name already in use where it lands gets the same
                    // numbering a copy would.
                    let names: Vec<String> = items.iter().map(|i| i.name().to_string()).collect();
                    let free = crate::model::free_name(item.name(), &names);
                    if free != item.name() {
                        item.set_name(&free);
                    }
                    let at = to.min(items.len());
                    items.insert(at, item);
                    None
                }
                None => Some(item),
            }
        };
        if let Some(item) = orphan {
            // The target went with it: dropping a folder inside itself
            // takes the whole branch out of the tree, so put it back
            // rather than losing it.
            if let Some(src) = self
                .collections
                .iter_mut()
                .find(|x| x.id == from_collection)
                && let Some(items) = src.items_mut(from_folder)
            {
                let at = from.min(items.len());
                items.insert(at, item);
            }
            return;
        }
        self.save_collection(from_collection, cx);
        if to_collection != from_collection {
            self.save_collection(to_collection, cx);
            // The open request may have just changed collection; the tab
            // saves into the file its state names, so that has to follow.
            if let Some(open) = self.state.request_id.clone()
                && moved_ids.contains(&open)
            {
                self.state.collection = Some(to_collection.to_string());
            }
        }
    }

    pub(crate) fn state_request_id(&self) -> Option<String> {
        self.state.request_id.clone()
    }
    pub(crate) fn state_collection(&self) -> Option<String> {
        self.state.collection.clone()
    }
}

// ---------------------------------------------------------------------------
// Package library tab

/// A package library in its own tab: name, where it came from, the code,
/// and the ways to fill it (type it, read a file, fetch a URL).
pub(crate) fn library_tab(
    t: &mut ApiTool,
    id: &str,
    ui: &mut egui::Ui,
    cx: &mut ToolContext,
    theme: &Theme,
) {
    let s = cx.services;
    let Some(index) = t.libraries.iter().position(|l| l.id == id) else {
        form::page_title(ui, theme, &s.t("api-library"), None);
        form::hint(ui, theme, &s.t("api-libraries-empty"));
        return;
    };
    let fetching = t.library_fetch.as_ref().is_some_and(|(f, _)| f == id);
    let mut save = false;
    let mut import_file = false;
    let mut fetch_url: Option<String> = None;
    let mut delete = false;
    egui::ScrollArea::vertical()
        .id_salt(("api-library-tab", id))
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let lib = &mut t.libraries[index];
            form::page_title(
                ui,
                theme,
                &s.t("api-library"),
                Some(&s.t("api-libraries-hint")),
            );
            form::section(ui, theme, &s.t("api-manage-general"), |ui| {
                form::field_grid(ui, ("api-library-general", id), |ui| {
                    form::label(ui, &s.t("api-library-name"));
                    form::text_field(ui, &mut lib.name, "");
                    ui.end_row();
                    form::label(ui, &s.t("api-library-kind"));
                    ui.label(s.t(lib.kind.label_key()));
                    ui.end_row();
                    if !lib.source.is_empty() {
                        form::label(ui, &s.t("api-library-source"));
                        ui.add(egui::Label::new(&lib.source).truncate())
                            .on_hover_text(&lib.source);
                        ui.end_row();
                    }
                    form::label(ui, &s.t("api-library-enabled"));
                    ui.checkbox(&mut lib.enabled, "");
                    ui.end_row();
                });
                form::hint(
                    ui,
                    theme,
                    &s.i18n.t_args("api-library-usage", &[("name", &lib.name)]),
                );
            });
            form::section(ui, theme, &s.t("api-library-code"), |ui| {
                ui.horizontal_wrapped(|ui| {
                    if ui.button(s.t("api-library-import-file")).clicked() {
                        import_file = true;
                    }
                    ui.menu_button(s.t("api-library-import-url"), |ui| {
                        ui.set_min_width(320.0);
                        let url_id = ui.id().with("url");
                        let mut url: String = ui.data(|d| d.get_temp(url_id)).unwrap_or_default();
                        let r = ui.add(
                            egui::TextEdit::singleline(&mut url)
                                .hint_text(s.t("api-library-url-hint"))
                                .desired_width(f32::INFINITY),
                        );
                        ui.data_mut(|d| d.insert_temp(url_id, url.clone()));
                        let go = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                        if (ui
                            .add_enabled(!fetching, egui::Button::new(s.t("api-library-fetch")))
                            .clicked()
                            || go)
                            && !url.trim().is_empty()
                        {
                            fetch_url = Some(url.trim().to_string());
                            ui.close();
                        }
                    });
                    if fetching {
                        ui.spinner();
                    }
                });
                form::hint(ui, theme, &s.t("api-library-code-hint"));
                let rows = ((ui.available_height() - 120.0)
                    / ui.text_style_height(&egui::TextStyle::Monospace))
                .floor()
                .clamp(12.0, 60.0) as usize;
                ui.add(
                    egui::TextEdit::multiline(&mut lib.code)
                        .font(egui::TextStyle::Monospace)
                        .desired_width(f32::INFINITY)
                        .desired_rows(rows),
                );
            });
            ui.horizontal(|ui| {
                if form::primary_button(ui, theme, &s.t("api-library-save")).clicked() {
                    save = true;
                }
                if form::danger_button(ui, theme, &s.t("api-library-delete")).clicked() {
                    delete = true;
                }
            });
        });
    if import_file
        && let Some(p) = rfd::FileDialog::new()
            .add_filter("JavaScript", &["js", "mjs", "txt"])
            .pick_file()
    {
        match std::fs::read_to_string(&p) {
            Ok(text) => {
                let lib = &mut t.libraries[index];
                lib.code = text;
                lib.kind = LibraryKind::Imported;
                lib.source = p.display().to_string();
                save = true;
                cx.toast(ToastLevel::Success, s.t("api-library-imported"));
            }
            Err(e) => cx.toast(ToastLevel::Error, e.to_string()),
        }
    }
    if let Some(url) = fetch_url {
        // The fetch runs on the runtime; `poll` keeps the text when it lands.
        let slot: std::sync::Arc<parking_lot::Mutex<Option<Result<String, String>>>> =
            std::sync::Arc::new(parking_lot::Mutex::new(None));
        let out = slot.clone();
        let waker = s.runtime.waker();
        let target = url.clone();
        s.runtime.spawn(async move {
            let result = async {
                let resp = reqwest::get(&target).await.map_err(|e| e.to_string())?;
                if !resp.status().is_success() {
                    return Err(format!("{} {}", resp.status().as_u16(), target));
                }
                resp.text().await.map_err(|e| e.to_string())
            }
            .await;
            *out.lock() = Some(result);
            waker.request_repaint();
        });
        t.libraries[index].source = url;
        t.library_fetch = Some((id.to_string(), slot));
    }
    if save {
        t.save_library(id, cx);
        cx.toast(ToastLevel::Success, s.t("api-library-saved"));
    }
    if delete {
        t.confirm_delete_library = Some(id.to_string());
    }
}

// ---------------------------------------------------------------------------
// Manage collection tab

/// A collection's own page: name, the variables its scripts saved, the
/// authentication and scripts every request under it inherits, the
/// package libraries it uses, and its documentation.
/// The categories of a Manage Collection tab (owner's request,
/// 2026-10-22: one page per category rather than one long page).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ManagePage {
    #[default]
    General,
    Variables,
    Globals,
    Auth,
    Scripts,
    Libraries,
    Docs,
}

impl ManagePage {
    const ALL: [ManagePage; 7] = [
        ManagePage::General,
        ManagePage::Variables,
        ManagePage::Globals,
        ManagePage::Auth,
        ManagePage::Scripts,
        ManagePage::Libraries,
        ManagePage::Docs,
    ];

    fn label_key(self) -> &'static str {
        match self {
            ManagePage::General => "api-manage-general",
            ManagePage::Variables => "api-manage-variables",
            ManagePage::Globals => "api-manage-globals",
            ManagePage::Auth => "api-manage-auth",
            ManagePage::Scripts => "api-manage-scripts",
            ManagePage::Libraries => "api-manage-libraries",
            ManagePage::Docs => "api-manage-docs",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            ManagePage::General => ph::SLIDERS,
            ManagePage::Variables => ph::TAG,
            ManagePage::Globals => ph::GLOBE,
            ManagePage::Auth => ph::KEY,
            ManagePage::Scripts => ph::CODE,
            ManagePage::Libraries => ph::PACKAGE,
            ManagePage::Docs => ph::BOOK_OPEN,
        }
    }
}

pub(crate) fn manage_tab(
    t: &mut ApiTool,
    cid: &str,
    ui: &mut egui::Ui,
    cx: &mut ToolContext,
    theme: &Theme,
) {
    let s = cx.services;
    let Some(index) = t.collections.iter().position(|c| c.id == cid) else {
        form::page_title(ui, theme, &s.t("api-manage-title"), None);
        form::hint(ui, theme, &s.t("api-manage-missing"));
        return;
    };
    let libraries: Vec<(String, String, bool)> = t
        .libraries
        .iter()
        .map(|l| (l.id.clone(), l.name.clone(), l.enabled))
        .collect();
    let library_names: Vec<String> = libraries
        .iter()
        .filter(|(_, _, enabled)| *enabled)
        .map(|(_, n, _)| n.clone())
        .collect();
    let mut save = false;
    let mut globals_changed = false;
    let mut variables_changed = false;
    let title = t.collections[index].name.clone();
    form::page_title(
        ui,
        theme,
        &format!("{} - {title}", s.t("api-manage-title")),
        Some(&s.t("api-manage-hint")),
    );
    // One row of category tabs, like a request's Params / Headers / Body.
    ui.horizontal_wrapped(|ui| {
        for page in ManagePage::ALL {
            let text = format!("{} {}", page.icon(), s.t(page.label_key()));
            if ui.selectable_label(t.manage_page == page, text).clicked() {
                t.manage_page = page;
            }
        }
    });
    ui.separator();
    let page = t.manage_page;
    egui::ScrollArea::vertical()
        .id_salt(("api-manage-tab", cid))
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.add_space(6.0);
            match page {
                ManagePage::General => {
                    let c = &mut t.collections[index];
                    form::section(ui, theme, &s.t("api-manage-general"), |ui| {
                        form::field_grid(ui, ("api-manage-general", cid), |ui| {
                            form::label(ui, &s.t("api-library-name"));
                            form::text_field(ui, &mut c.name, "");
                            ui.end_row();
                        });
                    });
                }
                ManagePage::Variables => {
                    let c = &mut t.collections[index];
                    form::section(ui, theme, &s.t("api-manage-variables"), |ui| {
                        form::hint(ui, theme, &s.t("api-manage-variables-hint"));
                        let before = c.variables.clone();
                        kv_table(ui, theme, s, &mut c.variables, "api-manage-vars");
                        if c.variables != before {
                            variables_changed = true;
                        }
                    });
                }
                ManagePage::Globals => {
                    form::section(ui, theme, &s.t("api-manage-globals"), |ui| {
                        form::hint(ui, theme, &s.t("api-manage-globals-hint"));
                        let before = t.globals.values.clone();
                        kv_table(ui, theme, s, &mut t.globals.values, "api-manage-globals");
                        if t.globals.values != before {
                            globals_changed = true;
                        }
                    });
                }
                ManagePage::Auth => {
                    let c = &mut t.collections[index];
                    form::section(ui, theme, &s.t("api-manage-auth"), |ui| {
                        form::hint(ui, theme, &s.t("api-manage-auth-hint"));
                        auth_editor(&mut c.auth, ui, s, theme, "api-manage-auth");
                    });
                }
                ManagePage::Scripts => {
                    let c = &mut t.collections[index];
                    form::section(ui, theme, &s.t("api-manage-scripts"), |ui| {
                        form::hint(ui, theme, &s.t("api-manage-scripts-hint"));
                        script_editor(
                            ui,
                            s,
                            "api-pre-script",
                            "api-pre-script-hint",
                            ("api-manage-pre", cid),
                            200.0,
                            &mut c.pre_script,
                            &library_names,
                        );
                        script_editor(
                            ui,
                            s,
                            "api-post-script",
                            "api-post-script-hint",
                            ("api-manage-post", cid),
                            200.0,
                            &mut c.post_script,
                            &library_names,
                        );
                    });
                }
                ManagePage::Libraries => {
                    let c = &mut t.collections[index];
                    form::section(ui, theme, &s.t("api-manage-libraries"), |ui| {
                        form::hint(ui, theme, &s.t("api-manage-libraries-hint"));
                        if libraries.is_empty() {
                            form::hint(ui, theme, &s.t("api-libraries-empty"));
                        }
                        for (id, name, enabled) in &libraries {
                            let mut on = c.library_refs.contains(id);
                            let label = if *enabled {
                                name.clone()
                            } else {
                                format!("{name} ({})", s.t("disabled"))
                            };
                            if ui.checkbox(&mut on, label).changed() {
                                if on {
                                    c.library_refs.push(id.clone());
                                } else {
                                    c.library_refs.retain(|r| r != id);
                                }
                            }
                        }
                    });
                }
                ManagePage::Docs => {
                    let c = &mut t.collections[index];
                    form::section(ui, theme, &s.t("api-manage-docs"), |ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut c.docs)
                                .desired_width(f32::INFINITY)
                                .desired_rows(12),
                        );
                    });
                }
            }
            if page != ManagePage::Globals && page != ManagePage::Variables {
                if form::primary_button(ui, theme, &s.t("api-manage-save")).clicked() {
                    save = true;
                }
            } else {
                form::hint(ui, theme, &s.t("api-manage-autosave-hint"));
            }
            ui.add_space(12.0);
        });
    if globals_changed && let Err(e) = t.globals.save(&t.globals_path()) {
        cx.toast(ToastLevel::Error, e.to_string());
    }
    // Variables are written as they are typed (owner's request): the
    // collection file is small and the other tabs learn of it at once.
    if variables_changed {
        t.save_collection(cid, cx);
    }
    if save {
        t.save_collection(cid, cx);
        cx.toast(ToastLevel::Success, s.t("api-manage-saved"));
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_strips_tags_and_scripts() {
        let t = strip_tags(
            "<html><head><style>b{}</style><script>alert(1)</script></head><body><h1>Hi</h1> <p>there</p></body></html>",
        );
        assert_eq!(t, "Hi there");
    }

    #[test]
    fn sizes_and_colours() {
        assert_eq!(human_size(10), "10 B");
        assert_eq!(human_size(2048), "2.0 KB");
        let th = Theme::dark();
        assert_eq!(status_colour(&th, 404), th.warning);
    }

    #[test]
    fn a_method_has_its_own_colour_and_the_rest_are_muted() {
        // The five that carry meaning are fixed colours, the same in both
        // themes, so a reader picks the verb out before reading the word.
        let th = Theme::dark();
        for m in ["GET", "POST", "PUT", "PATCH", "DELETE"] {
            let c = method_rgb(m).unwrap_or_else(|| panic!("{m} has a colour"));
            assert_eq!(method_colour(&th, m), c, "{m}");
            assert_ne!(c, th.text_muted, "{m} is not the muted fallback");
        }
        // All five differ from one another.
        let mut seen = std::collections::HashSet::new();
        for m in ["GET", "POST", "PUT", "PATCH", "DELETE"] {
            assert!(seen.insert(method_rgb(m).unwrap()), "{m} repeats a colour");
        }
        for m in ["HEAD", "OPTIONS", "TRACE", "WHAT"] {
            assert_eq!(method_rgb(m), None, "{m}");
            assert_eq!(method_colour(&th, m), th.text_muted, "{m}");
        }
    }
}
