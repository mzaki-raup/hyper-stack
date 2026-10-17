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
use crate::model::{AuthKind, BodyKind, Collection, Folder, Item, Kv, METHODS, Request};
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

pub fn method_colour(theme: &Theme, method: &str) -> egui::Color32 {
    match method {
        "GET" => theme.success,
        "POST" => theme.secondary,
        "PUT" | "PATCH" => theme.warning,
        "DELETE" => theme.error,
        _ => theme.text_muted,
    }
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

enum SideAction {
    Open(String, String),
    NewRequest(String, Option<String>),
    NewFolder(String, Option<String>),
    Rename(String, String, String),
    Delete(String, Option<String>),
    Duplicate(String, String),
    Export(String),
    Import,
    NewCollection,
    LoadHistory(usize),
    /// Drag-reorder inside one level: the collection, the folder it is in
    /// (`None` = the collection's own items), and the move. Collections
    /// themselves reorder with `collection` empty.
    Reorder {
        collection: String,
        folder: Option<String>,
        from: usize,
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
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .small_button(ph::CLOCK_COUNTER_CLOCKWISE)
                .on_hover_text(s.t("api-history"))
                .clicked()
            {
                t.show_history = !t.show_history;
            }
            if ui
                .small_button(ph::DOWNLOAD_SIMPLE)
                .on_hover_text(s.t("api-import-tip"))
                .clicked()
            {
                actions.push(SideAction::Import);
            }
            if ui
                .small_button(ph::FOLDER_PLUS)
                .on_hover_text(s.t("api-new-collection"))
                .clicked()
            {
                actions.push(SideAction::NewCollection);
            }
            if ui
                .small_button(ph::PLUS)
                .on_hover_text(s.t("api-new-request"))
                .clicked()
            {
                t.new_request();
            }
        });
    });
    ui.add(
        egui::TextEdit::singleline(&mut t.filter)
            .hint_text(s.t("api-filter"))
            .desired_width(f32::INFINITY),
    );
    ui.add_space(4.0);
    let filter = t.filter.trim().to_lowercase();
    let selected = t.state_request_id();
    egui::ScrollArea::vertical()
        .id_salt("api-sidebar")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            if t.collections.is_empty() {
                form::note(ui, theme, hs_ui::icons::INFO, &s.t("api-empty-hint"));
            }
            let mut collections = hs_ui::reorder::Reorder::new("api-collections");
            for (ci, c) in t.collections.iter().enumerate() {
                let header = collections.row(ui, ci, |ui| {
                    egui::CollapsingHeader::new(
                        egui::RichText::new(format!("{} {}", ph::FOLDER, c.name)).strong(),
                    )
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
                });
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
                    if ui.button(s.t("api-export-postman")).clicked() {
                        actions.push(SideAction::Export(c.id.clone()));
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
                actions.push(SideAction::Reorder {
                    collection: String::new(),
                    folder: None,
                    from,
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
    for a in actions {
        match a {
            SideAction::Open(c, r) => t.open_request(&c, &r),
            SideAction::NewRequest(c, folder) => {
                let name = s.t("api-new-request-name");
                t.new_request();
                t.request.name = name.clone();
                t.save_as(&c, folder.as_deref(), &name, cx);
            }
            SideAction::NewFolder(c, folder) => {
                if let Some(col) = t.collections.iter_mut().find(|x| x.id == c)
                    && let Some(items) = col.items_mut(folder.as_deref())
                {
                    items.push(Item::Folder(Folder {
                        name: s.t("api-new-folder-name"),
                        ..Default::default()
                    }));
                }
                t.save_collection(&c, cx);
            }
            SideAction::Rename(c, id, name) => t.rename = Some((format!("{c}/{id}"), name)),
            SideAction::Reorder {
                collection,
                folder,
                from,
                to,
            } => {
                // An empty collection id means the collections themselves.
                if collection.is_empty() {
                    hs_ui::reorder::move_item(&mut t.collections, from, to);
                    t.save_order(cx);
                } else if let Some(col) = t.collections.iter_mut().find(|x| x.id == collection)
                    && let Some(items) = col.items_mut(folder.as_deref())
                {
                    hs_ui::reorder::move_item(items, from, to);
                    t.save_collection(&collection, cx);
                }
            }
            SideAction::Delete(c, item) => match item {
                Some(id) => {
                    if let Some(col) = t.collections.iter_mut().find(|x| x.id == c) {
                        col.remove(&id);
                    }
                    t.save_collection(&c, cx);
                    if t.state_request_id().as_deref() == Some(&id) {
                        t.new_request();
                    }
                }
                None => {
                    let _ = std::fs::remove_file(Collection::path(&t.collections_dir(), &c));
                    t.collections.retain(|x| x.id != c);
                    if t.state_collection().as_deref() == Some(&c) {
                        t.new_request();
                    }
                }
            },
            SideAction::Duplicate(c, id) => {
                let copy = t.collections.iter().find(|x| x.id == c).and_then(|x| {
                    x.find(&id)
                        .map(|(r, folders)| (r.clone(), folders.last().map(|f| f.id.clone())))
                });
                if let Some((mut r, folder)) = copy {
                    r.id = hs_core::new_id();
                    r.name = format!("{} (copy)", r.name);
                    if let Some(col) = t.collections.iter_mut().find(|x| x.id == c)
                        && let Some(items) = col.items_mut(folder.as_deref())
                    {
                        items.push(Item::Request(r));
                    }
                    t.save_collection(&c, cx);
                }
            }
            SideAction::Export(c) => t.export_postman(&c, cx),
            SideAction::Import => {
                if let Some(p) = rfd::FileDialog::new()
                    .add_filter("Postman JSON", &["json"])
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
    let mut reorder = hs_ui::reorder::Reorder::new((
        "api-items",
        collection.to_string(),
        folder.unwrap_or("").to_string(),
    ));
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
                    egui::CollapsingHeader::new(format!("{} {}", ph::FOLDER, f.name))
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
                });
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
    if let Some((from, to)) = reorder.finish(ui, theme.accent) {
        actions.push(SideAction::Reorder {
            collection: collection.to_string(),
            folder: folder.map(str::to_string),
            from,
            to,
        });
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
    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt("api-method")
            .width(96.0)
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
    let a = &mut t.request.auth;
    ui.horizontal(|ui| {
        ui.label(s.t("api-auth-type"));
        egui::ComboBox::from_id_salt("api-auth")
            .selected_text(a.kind.label())
            .show_ui(ui, |ui| {
                for k in AuthKind::ALL {
                    ui.selectable_value(&mut a.kind, k, k.label());
                }
            });
    });
    ui.add_space(6.0);
    form::field_grid(ui, "api-auth-grid", |ui| match a.kind {
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
        _ => {}
    });
    form::hint(ui, theme, &s.t("api-auth-hint"));
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
    for (label, field, key) in [
        ("api-pre-script", 0usize, "api-pre-script-hint"),
        ("api-post-script", 1usize, "api-post-script-hint"),
    ] {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(s.t(label)).strong());
            ui.menu_button(format!("{} {}", ph::CODE, s.t("api-snippets")), |ui| {
                for (name, code) in SNIPPETS {
                    if ui.button(name).clicked() {
                        let target = if field == 0 {
                            &mut t.request.pre_script
                        } else {
                            &mut t.request.post_script
                        };
                        if !target.is_empty() && !target.ends_with('\n') {
                            target.push('\n');
                        }
                        target.push_str(code);
                        target.push('\n');
                        ui.close();
                    }
                }
            });
        });
        let target = if field == 0 {
            &mut t.request.pre_script
        } else {
            &mut t.request.post_script
        };
        let width = ui.available_width();
        ui.allocate_ui(egui::vec2(width, each), |ui| {
            egui::ScrollArea::vertical()
                .id_salt(("api-script", field))
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::multiline(target)
                            .hint_text(s.t(key))
                            .font(egui::TextStyle::Monospace)
                            .desired_width(f32::INFINITY)
                            .desired_rows(6),
                    );
                });
        });
        ui.add_space(6.0);
    }
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
                    "Request".into()
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
                if let Some(c) = t.collections.iter_mut().find(|c| c.id == cid) {
                    if c.id == id {
                        c.name = name.trim().to_string();
                    } else if let Some(r) = c.find_mut(&id) {
                        r.name = name.trim().to_string();
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
                        rename_folder(&mut c.items, &id, name.trim());
                    }
                }
                t.save_collection(&cid, cx);
                if t.state_request_id().as_deref() == Some(&id) {
                    t.request.name = name.trim().to_string();
                    if let Some(saved) = &mut t.saved {
                        saved.name = name.trim().to_string();
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
    pub(crate) fn state_request_id(&self) -> Option<String> {
        self.state.request_id.clone()
    }
    pub(crate) fn state_collection(&self) -> Option<String> {
        self.state.collection.clone()
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
        assert_eq!(method_colour(&th, "GET"), th.success);
        assert_eq!(status_colour(&th, 404), th.warning);
    }
}
