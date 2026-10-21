//! The browser's sidebar: **Favourites** and **Passwords**, one set per
//! workspace.
//!
//! The list lives in the sidebar and the detail pane opens beside the page,
//! not over it: the browser reports where its web view should sit each
//! frame, so narrowing that rectangle gives the editor room the same way
//! the AI assistant takes a column in the other tools. A native web view
//! paints above anything egui draws, so a pane drawn *over* the page would
//! be invisible.
//!
//! Secrets are never held in these files. A login keeps a `secret:<ulid>`
//! reference and the value lives in the vault, so revealing or filling one
//! needs the vault unlocked (`docs/06-security.md`).

use std::collections::HashMap;

use hs_core::tool::{ToastLevel, ToolContext};
use hs_ui::form;
use hs_ui::icons::ph;
use hs_ui::theme::Theme;

use crate::BrowserTool;
use crate::store::{FavFolder, FavItem, Favorite, Login};

/// Which half of the sidebar is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SideTab {
    #[default]
    Favorites,
    Passwords,
}

/// What the detail pane is editing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Editing {
    Favorite(String),
    Login(String),
}

/// A delete waiting to be confirmed. Deleting a folder takes everything
/// inside it, so the question says how much is going.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Confirm {
    pub id: String,
    pub name: String,
    pub children: usize,
    pub login: bool,
}

/// Everything the sidebar remembers between frames.
#[derive(Default)]
pub struct SideState {
    pub tab: SideTab,
    pub open_folders: std::collections::HashSet<String>,
    pub editing: Option<Editing>,
    pub confirm: Option<Confirm>,
    pub filter: String,
    /// Typed replacements for the three secret fields, and whether each was
    /// revealed from the vault. Empty means "leave what is stored alone".
    pub secrets: [String; 3],
    pub revealed: [bool; 3],
    /// A detail form changed; the file is written once typing pauses (no
    /// text field has the keyboard), not on every keystroke.
    pub favorites_dirty: bool,
    pub passwords_dirty: bool,
}

/// Write back what the detail forms changed, once the keyboard is free.
fn flush_dirty(t: &mut BrowserTool, ui: &egui::Ui, cx: &mut ToolContext) {
    if ui.ctx().egui_wants_keyboard_input() {
        return;
    }
    if std::mem::take(&mut t.side.favorites_dirty) {
        save_favorites(t, cx);
    }
    if std::mem::take(&mut t.side.passwords_dirty) {
        save_passwords(t, cx);
    }
}

/// Indexes into [`SideState::secrets`], so the three fields and their
/// buffers cannot drift apart.
const PASSWORD: usize = 0;
const PASSKEY: usize = 1;
const API_KEY: usize = 2;

/// What the sidebar wants the tool to do.
#[derive(Default)]
pub struct SideOut {
    /// Open this URL in a page.
    pub open: Option<String>,
}

/// Draw the sidebar. Returns what the user asked for.
pub fn sidebar(
    t: &mut BrowserTool,
    ui: &mut egui::Ui,
    cx: &mut ToolContext,
    theme: &Theme,
    icons: &HashMap<String, Option<String>>,
) -> SideOut {
    let mut out = SideOut::default();
    let s = cx.services;
    flush_dirty(t, ui, cx);
    form::header_bar(ui, theme, |ui| {
        ui.label(egui::RichText::new(s.t("tool-browser")).strong().size(15.0));
    });
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        for (tab, key) in [
            (SideTab::Favorites, "browser-favorites"),
            (SideTab::Passwords, "browser-passwords"),
        ] {
            if ui
                .add(egui::Button::selectable(t.side.tab == tab, s.t(key)))
                .clicked()
            {
                t.side.tab = tab;
                // The panes edit different things; carrying a selection
                // across would open the wrong editor.
                t.side.editing = None;
            }
        }
    });
    ui.add_space(4.0);
    ui.add(
        egui::TextEdit::singleline(&mut t.side.filter)
            .hint_text(s.t("browser-side-filter"))
            .desired_width(f32::INFINITY),
    );
    ui.add_space(4.0);
    match t.side.tab {
        SideTab::Favorites => favorites(t, ui, cx, theme, icons, &mut out),
        SideTab::Passwords => passwords(t, ui, cx, theme, icons),
    }
    confirm_dialog(t, ui, cx, theme);
    out
}

// --------------------------------------------------------------------- //
// Favourites
// --------------------------------------------------------------------- //

/// What a pass over the tree decided, applied after the walk so the tree is
/// not being changed while it is read.
#[derive(Default)]
struct FavActions {
    open: Option<String>,
    edit: Option<String>,
    confirm: Option<Confirm>,
    new_folder_in: Option<Option<String>>,
    /// Move `id` into `folder` (`None` = the root), at `index`.
    move_to: Option<(String, Option<String>, usize)>,
}

fn favorites(
    t: &mut BrowserTool,
    ui: &mut egui::Ui,
    cx: &mut ToolContext,
    theme: &Theme,
    icons: &HashMap<String, Option<String>>,
    out: &mut SideOut,
) {
    let s = cx.services;
    let mut acts = FavActions::default();
    ui.horizontal(|ui| {
        if ui
            .small_button(ph::FOLDER_PLUS)
            .on_hover_text(s.t("browser-new-folder"))
            .clicked()
        {
            acts.new_folder_in = Some(None);
        }
    });
    ui.add_space(2.0);
    if t.favorites.items.is_empty() {
        form::note(ui, theme, ph::STAR, &s.t("browser-no-favorites"));
    }
    let filter = t.side.filter.trim().to_lowercase();
    egui::ScrollArea::vertical()
        .id_salt("browser-favorites")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let items = t.favorites.items.clone();
            fav_level(
                t, ui, theme, s, None, &items, &filter, icons, &mut acts, out,
            );
        });
    apply_fav(t, cx, acts, out);
}

#[allow(clippy::too_many_arguments)]
fn fav_level(
    t: &mut BrowserTool,
    ui: &mut egui::Ui,
    theme: &Theme,
    s: &hs_core::Services,
    folder: Option<&str>,
    items: &[FavItem],
    filter: &str,
    icons: &HashMap<String, Option<String>>,
    acts: &mut FavActions,
    out: &mut SideOut,
) {
    let scope = folder.unwrap_or("").to_string();
    let mut reorder = hs_ui::reorder::Reorder::scoped(("browser-fav", scope.clone()), scope);
    for (index, it) in items.iter().enumerate() {
        match it {
            FavItem::Folder(f) => {
                let open = t.side.open_folders.contains(&f.id);
                let r = reorder.row(ui, index, |ui| {
                    egui::CollapsingHeader::new(format!("{} {}", ph::FOLDER, f.name))
                        .id_salt(("browser-fav-folder", &f.id))
                        .default_open(open || !filter.is_empty())
                        .show(ui, |ui| {
                            fav_level(
                                t,
                                ui,
                                theme,
                                s,
                                Some(&f.id),
                                &f.items,
                                filter,
                                icons,
                                acts,
                                out,
                            );
                        })
                });
                if r.header_response.clicked() {
                    if open {
                        t.side.open_folders.remove(&f.id);
                    } else {
                        t.side.open_folders.insert(f.id.clone());
                    }
                }
                // A folder takes a drop of its own, so a collapsed or empty
                // one can still be the place something goes.
                let rect = r.header_response.rect;
                let dragging_elsewhere = hs_ui::reorder::dragged_scope(ui.ctx()).is_some();
                if dragging_elsewhere {
                    if ui
                        .input(|i| i.pointer.interact_pos())
                        .is_some_and(|p| rect.contains(p))
                    {
                        reorder.suppress_target();
                    }
                    if let Some(d) = hs_ui::reorder::drop_onto(ui, rect, theme.accent) {
                        let from = dropped_id(t, &d.scope, d.index);
                        if let Some(id) = from
                            && !t.favorites.is_within(&f.id, &id)
                        {
                            acts.move_to = Some((id, Some(f.id.clone()), usize::MAX));
                        }
                    }
                }
                r.header_response.context_menu(|ui| {
                    if ui.button(s.t("browser-new-folder")).clicked() {
                        acts.new_folder_in = Some(Some(f.id.clone()));
                        ui.close();
                    }
                    if ui.button(s.t("browser-rename")).clicked() {
                        acts.edit = Some(f.id.clone());
                        ui.close();
                    }
                    ui.separator();
                    if ui.button(s.t("browser-delete")).clicked() {
                        acts.confirm = Some(Confirm {
                            id: f.id.clone(),
                            name: f.name.clone(),
                            children: count_items(&f.items),
                            login: false,
                        });
                        ui.close();
                    }
                });
            }
            FavItem::Link(l) => {
                if !filter.is_empty()
                    && !l.name.to_lowercase().contains(filter)
                    && !l.url.to_lowercase().contains(filter)
                {
                    continue;
                }
                reorder.row(ui, index, |ui| {
                    let icon = site_icon(&l.icon, &l.url, icons);
                    let row = icon_row(ui, theme, &icon, &l.name, &l.url, false);
                    if row.clicked() {
                        out.open = Some(l.url.clone());
                    }
                    row.context_menu(|ui| {
                        if ui.button(s.t("browser-open")).clicked() {
                            acts.open = Some(l.url.clone());
                            ui.close();
                        }
                        if ui.button(s.t("browser-edit")).clicked() {
                            acts.edit = Some(l.id.clone());
                            ui.close();
                        }
                        ui.separator();
                        if ui.button(s.t("browser-delete")).clicked() {
                            acts.confirm = Some(Confirm {
                                id: l.id.clone(),
                                name: l.name.clone(),
                                children: 0,
                                login: false,
                            });
                            ui.close();
                        }
                    });
                });
            }
        }
        ui.add_space(1.0);
    }
    if let Some(mv) = reorder.finish_move(ui, theme.accent) {
        use hs_ui::reorder::Move;
        match mv {
            Move::Within { from, to } => {
                if let Some(id) = items.get(from).map(|i| i.id().to_string()) {
                    acts.move_to = Some((id, folder.map(str::to_string), to));
                }
            }
            Move::From { scope, from, to } => {
                if let Some(id) = dropped_id(t, &scope, from) {
                    acts.move_to = Some((id, folder.map(str::to_string), to));
                }
            }
        }
    }
}

/// Which item a drag started on, given the level it came from.
fn dropped_id(t: &BrowserTool, scope: &str, index: usize) -> Option<String> {
    let folder = (!scope.is_empty()).then(|| scope.to_string());
    t.favorites
        .items(folder.as_deref())?
        .get(index)
        .map(|i| i.id().to_string())
}

fn count_items(items: &[FavItem]) -> usize {
    items
        .iter()
        .map(|i| match i {
            FavItem::Folder(f) => 1 + count_items(&f.items),
            FavItem::Link(_) => 1,
        })
        .sum()
}

fn apply_fav(t: &mut BrowserTool, cx: &mut ToolContext, acts: FavActions, out: &mut SideOut) {
    let s = cx.services;
    let mut dirty = false;
    if let Some(url) = acts.open {
        out.open = Some(url);
    }
    if let Some(parent) = acts.new_folder_in {
        let f = FavFolder {
            name: s.t("browser-folder-name"),
            ..Default::default()
        };
        let id = f.id.clone();
        if let Some(items) = t.favorites.items_mut(parent.as_deref()) {
            items.push(FavItem::Folder(f));
            dirty = true;
            t.side.editing = Some(Editing::Favorite(id));
        }
    }
    if let Some(id) = acts.edit {
        t.side.editing = Some(Editing::Favorite(id));
    }
    if let Some(c) = acts.confirm {
        t.side.confirm = Some(c);
    }
    if let Some((id, folder, at)) = acts.move_to {
        // Into itself would take the branch out of the tree with it.
        let bad = folder
            .as_deref()
            .is_some_and(|f| t.favorites.is_within(f, &id));
        if !bad && let Some(item) = t.favorites.remove(&id) {
            if let Some(items) = t.favorites.items_mut(folder.as_deref()) {
                let at = at.min(items.len());
                items.insert(at, item);
            } else {
                // The target vanished with the item; put it back at the root
                // rather than losing it.
                t.favorites.items.push(item);
            }
            dirty = true;
        }
    }
    if dirty {
        save_favorites(t, cx);
    }
}

pub fn save_favorites(t: &mut BrowserTool, cx: &mut ToolContext) {
    if let Err(e) = t.favorites.save(&t.root) {
        cx.toast(ToastLevel::Error, e.to_string());
    }
}

pub fn save_passwords(t: &mut BrowserTool, cx: &mut ToolContext) {
    if let Err(e) = t.passwords.save(&t.root) {
        cx.toast(ToastLevel::Error, e.to_string());
    }
}

// --------------------------------------------------------------------- //
// Passwords
// --------------------------------------------------------------------- //

fn passwords(
    t: &mut BrowserTool,
    ui: &mut egui::Ui,
    cx: &mut ToolContext,
    theme: &Theme,
    icons: &HashMap<String, Option<String>>,
) {
    let s = cx.services;
    let mut add = false;
    let mut edit: Option<String> = None;
    let mut confirm: Option<Confirm> = None;
    let mut move_to: Option<(usize, usize)> = None;
    ui.horizontal(|ui| {
        if ui
            .small_button(ph::PLUS)
            .on_hover_text(s.t("browser-new-login"))
            .clicked()
        {
            add = true;
        }
        if s.secrets.is_locked() {
            ui.label(
                egui::RichText::new(s.t("browser-vault-locked"))
                    .size(11.0)
                    .color(theme.warning),
            );
        }
    });
    ui.add_space(2.0);
    if t.passwords.items.is_empty() {
        form::note(ui, theme, ph::KEY, &s.t("browser-no-logins"));
    }
    let filter = t.side.filter.trim().to_lowercase();
    let selected = match &t.side.editing {
        Some(Editing::Login(id)) => Some(id.clone()),
        _ => None,
    };
    egui::ScrollArea::vertical()
        .id_salt("browser-passwords")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let mut reorder = hs_ui::reorder::Reorder::new("browser-logins");
            for (i, l) in t.passwords.items.clone().iter().enumerate() {
                if !filter.is_empty()
                    && !l.name.to_lowercase().contains(&filter)
                    && !l.url.to_lowercase().contains(&filter)
                    && !l.username.to_lowercase().contains(&filter)
                {
                    continue;
                }
                reorder.row(ui, i, |ui| {
                    let icon = site_icon(&l.icon, &l.url, icons);
                    let sub = if l.username.is_empty() {
                        l.url.clone()
                    } else {
                        format!("{} · {}", l.username, l.url)
                    };
                    let row = icon_row(
                        ui,
                        theme,
                        &icon,
                        &l.name,
                        &sub,
                        selected.as_deref() == Some(l.id.as_str()),
                    );
                    if row.clicked() {
                        edit = Some(l.id.clone());
                    }
                    row.context_menu(|ui| {
                        if ui.button(s.t("browser-edit")).clicked() {
                            edit = Some(l.id.clone());
                            ui.close();
                        }
                        ui.separator();
                        if ui.button(s.t("browser-delete")).clicked() {
                            confirm = Some(Confirm {
                                id: l.id.clone(),
                                name: l.name.clone(),
                                children: 0,
                                login: true,
                            });
                            ui.close();
                        }
                    });
                });
                ui.add_space(1.0);
            }
            if let Some((from, to)) = reorder.finish(ui, theme.accent) {
                move_to = Some((from, to));
            }
        });
    if add {
        let l = Login {
            name: s.t("browser-login-name"),
            ..Default::default()
        };
        let id = l.id.clone();
        t.passwords.items.push(l);
        t.side.editing = Some(Editing::Login(id));
        clear_secret_buffers(t);
        save_passwords(t, cx);
    }
    if let Some(id) = edit {
        t.side.editing = Some(Editing::Login(id));
        clear_secret_buffers(t);
    }
    if let Some(c) = confirm {
        t.side.confirm = Some(c);
    }
    if let Some((from, to)) = move_to {
        hs_ui::reorder::move_item(&mut t.passwords.items, from, to);
        save_passwords(t, cx);
    }
}

fn clear_secret_buffers(t: &mut BrowserTool) {
    t.side.secrets = Default::default();
    t.side.revealed = [false; 3];
}

// --------------------------------------------------------------------- //
// Shared row drawing
// --------------------------------------------------------------------- //

/// The icon a row shows: the user's own if they set one, else the site's
/// favicon, else a globe.
pub(crate) fn site_icon(
    custom: &str,
    url: &str,
    icons: &HashMap<String, Option<String>>,
) -> Option<String> {
    if !custom.is_empty() {
        return Some(custom.to_string());
    }
    icons.get(url).cloned().flatten()
}

/// A row with an image (or glyph) on the left, a name, and a quieter
/// second line.
fn icon_row(
    ui: &mut egui::Ui,
    theme: &Theme,
    icon: &Option<String>,
    name: &str,
    sub: &str,
    selected: bool,
) -> egui::Response {
    let height = if sub.is_empty() { 26.0 } else { 38.0 };
    let (rect, resp) = ui.allocate_exact_size(
        egui::Vec2::new(ui.available_width(), height),
        egui::Sense::click(),
    );
    if selected {
        ui.painter()
            .rect_filled(rect, 4.0, theme.accent.gamma_multiply(0.25));
    } else if resp.hovered() {
        ui.painter()
            .rect_filled(rect, 4.0, theme.panel_bg.gamma_multiply(1.4));
    }
    let icon_rect = egui::Rect::from_min_size(
        egui::pos2(rect.left() + 6.0, rect.center().y - 8.0),
        egui::Vec2::splat(16.0),
    );
    match icon {
        Some(uri) => {
            egui::Image::from_uri(uri.clone())
                .fit_to_exact_size(egui::Vec2::splat(16.0))
                .paint_at(ui, icon_rect);
        }
        None => {
            ui.painter().text(
                icon_rect.center(),
                egui::Align2::CENTER_CENTER,
                ph::GLOBE,
                egui::TextStyle::Body.resolve(ui.style()),
                theme.text_muted,
            );
        }
    }
    let text_left = icon_rect.right() + 8.0;
    let font = egui::TextStyle::Body.resolve(ui.style());
    let small = egui::TextStyle::Small.resolve(ui.style());
    if sub.is_empty() {
        ui.painter().text(
            egui::pos2(text_left, rect.center().y),
            egui::Align2::LEFT_CENTER,
            elide(ui, name, &font, rect.right() - text_left - 6.0),
            font,
            theme.text,
        );
    } else {
        ui.painter().text(
            egui::pos2(text_left, rect.top() + 11.0),
            egui::Align2::LEFT_CENTER,
            elide(ui, name, &font, rect.right() - text_left - 6.0),
            font,
            theme.text,
        );
        ui.painter().text(
            egui::pos2(text_left, rect.bottom() - 11.0),
            egui::Align2::LEFT_CENTER,
            elide(ui, sub, &small, rect.right() - text_left - 6.0),
            small,
            theme.text_muted,
        );
    }
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, name));
    resp
}

/// Cut a label to the width it has, with an ellipsis. A painter draws no
/// layout of its own, so the row has to do this itself.
fn elide(ui: &egui::Ui, text: &str, font: &egui::FontId, width: f32) -> String {
    let full = ui
        .painter()
        .layout_no_wrap(text.to_string(), font.clone(), egui::Color32::WHITE);
    if full.size().x <= width || text.is_empty() {
        return text.to_string();
    }
    let keep = ((width / full.size().x) * text.chars().count() as f32).floor() as usize;
    let keep = keep.saturating_sub(1).max(1);
    let mut s: String = text.chars().take(keep).collect();
    s.push('…');
    s
}

// --------------------------------------------------------------------- //
// Delete confirmation
// --------------------------------------------------------------------- //

fn confirm_dialog(t: &mut BrowserTool, ui: &mut egui::Ui, cx: &mut ToolContext, theme: &Theme) {
    let Some(c) = t.side.confirm.clone() else {
        return;
    };
    let s = cx.services;
    let body = if c.children > 0 {
        s.i18n.t_args(
            "browser-delete-folder-body",
            &[("name", &c.name), ("count", &c.children.to_string())],
        )
    } else {
        s.i18n.t_args("browser-delete-body", &[("name", &c.name)])
    };
    let mut close = false;
    let mut go = false;
    let ctx = ui.ctx().clone();
    match hs_ui::dialog::confirm(
        &ctx,
        theme,
        "browser-delete",
        &s.t("browser-delete-title"),
        &body,
        &s.t("browser-delete"),
        &s.t("cancel"),
        true,
    ) {
        hs_ui::dialog::DialogResult::Ok => {
            go = true;
            close = true;
        }
        hs_ui::dialog::DialogResult::Cancel => close = true,
        hs_ui::dialog::DialogResult::Open => {}
    }
    if go {
        if c.login {
            t.passwords.items.retain(|l| l.id != c.id);
            save_passwords(t, cx);
        } else {
            t.favorites.remove(&c.id);
            save_favorites(t, cx);
        }
        if matches!(&t.side.editing,
            Some(Editing::Favorite(id)) | Some(Editing::Login(id)) if *id == c.id)
        {
            t.side.editing = None;
        }
    }
    if close {
        t.side.confirm = None;
    }
}

// --------------------------------------------------------------------- //
// The detail pane
// --------------------------------------------------------------------- //

/// How wide the detail pane is when it is open, in points. Zero when it is
/// closed, which is what the tool subtracts from the page's rectangle.
pub fn detail_width(t: &BrowserTool) -> f32 {
    if t.side.editing.is_some() { 300.0 } else { 0.0 }
}

/// The editor beside the page. Returns true when it drew.
pub fn detail(t: &mut BrowserTool, ui: &mut egui::Ui, cx: &mut ToolContext, theme: &Theme) -> bool {
    let Some(editing) = t.side.editing.clone() else {
        return false;
    };
    let s = cx.services;
    let mut close = false;
    egui::Frame::new()
        .fill(theme.panel_bg)
        .stroke(egui::Stroke::new(1.0, theme.border))
        .inner_margin(10.0)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(match editing {
                        Editing::Favorite(_) => s.t("browser-edit-favorite"),
                        Editing::Login(_) => s.t("browser-edit-login"),
                    })
                    .strong(),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if hs_ui::form::icon_button(ui, theme, ph::X, 14.0, false)
                        .on_hover_text(s.t("browser-hide-detail"))
                        .clicked()
                    {
                        close = true;
                    }
                });
            });
            ui.separator();
            ui.add_space(6.0);
            match editing {
                Editing::Favorite(id) => favorite_detail(t, ui, cx, theme, &id),
                Editing::Login(id) => login_detail(t, ui, cx, theme, &id),
            }
        });
    if close {
        t.side.editing = None;
    }
    true
}

fn favorite_detail(
    t: &mut BrowserTool,
    ui: &mut egui::Ui,
    cx: &mut ToolContext,
    theme: &Theme,
    id: &str,
) {
    let s = cx.services;
    // A folder has only a name; a link has the rest.
    let is_folder = t.favorites.link(id).is_none();
    let mut changed = false;
    if is_folder {
        let Some(name) = folder_name_mut(t, id) else {
            return;
        };
        ui.label(egui::RichText::new(s.t("browser-name")).size(11.0));
        changed |= ui
            .add(egui::TextEdit::singleline(name).desired_width(f32::INFINITY))
            .changed();
    } else {
        let Some(l) = t.favorites.link_mut(id) else {
            return;
        };
        ui.label(egui::RichText::new(s.t("browser-name")).size(11.0));
        changed |= ui
            .add(egui::TextEdit::singleline(&mut l.name).desired_width(f32::INFINITY))
            .changed();
        ui.add_space(6.0);
        ui.label(egui::RichText::new(s.t("browser-url")).size(11.0));
        changed |= ui
            .add(egui::TextEdit::singleline(&mut l.url).desired_width(f32::INFINITY))
            .changed();
        ui.add_space(6.0);
        icon_field(ui, theme, s, &mut l.icon, &mut changed);
    }
    if changed {
        t.side.favorites_dirty = true;
    }
}

fn folder_name_mut<'a>(t: &'a mut BrowserTool, id: &str) -> Option<&'a mut String> {
    fn walk<'a>(items: &'a mut [FavItem], id: &str) -> Option<&'a mut String> {
        for it in items.iter_mut() {
            if let FavItem::Folder(f) = it {
                if f.id == id {
                    return Some(&mut f.name);
                }
                if let Some(v) = walk(&mut f.items, id) {
                    return Some(v);
                }
            }
        }
        None
    }
    walk(&mut t.favorites.items, id)
}

/// The icon field: the site's own favicon unless the user pastes a data
/// URI of their own. A file picker would be friendlier, but it cannot open
/// headlessly, so the field takes what it is given either way.
fn icon_field(
    ui: &mut egui::Ui,
    theme: &Theme,
    s: &hs_core::Services,
    icon: &mut String,
    changed: &mut bool,
) {
    ui.label(egui::RichText::new(s.t("browser-icon")).size(11.0));
    *changed |= ui
        .add(
            egui::TextEdit::singleline(icon)
                .hint_text(s.t("browser-icon-hint"))
                .desired_width(f32::INFINITY),
        )
        .changed();
    ui.horizontal(|ui| {
        if !icon.is_empty() {
            ui.add(egui::Image::from_uri(icon.clone()).fit_to_exact_size(egui::Vec2::splat(16.0)));
            if ui.small_button(s.t("browser-icon-clear")).clicked() {
                icon.clear();
                *changed = true;
            }
        } else {
            ui.label(
                egui::RichText::new(s.t("browser-icon-auto"))
                    .size(11.0)
                    .color(theme.text_muted),
            );
        }
    });
}

fn login_detail(
    t: &mut BrowserTool,
    ui: &mut egui::Ui,
    cx: &mut ToolContext,
    theme: &Theme,
    id: &str,
) {
    let s = cx.services;
    let locked = s.secrets.is_locked();
    let Some(i) = t.passwords.items.iter().position(|l| l.id == id) else {
        return;
    };
    let mut changed = false;
    {
        let l = &mut t.passwords.items[i];
        ui.label(egui::RichText::new(s.t("browser-name")).size(11.0));
        changed |= ui
            .add(egui::TextEdit::singleline(&mut l.name).desired_width(f32::INFINITY))
            .changed();
        ui.add_space(6.0);
        ui.label(egui::RichText::new(s.t("browser-url")).size(11.0));
        changed |= ui
            .add(egui::TextEdit::singleline(&mut l.url).desired_width(f32::INFINITY))
            .changed();
        ui.add_space(6.0);
        ui.label(egui::RichText::new(s.t("browser-username")).size(11.0));
        changed |= ui
            .add(egui::TextEdit::singleline(&mut l.username).desired_width(f32::INFINITY))
            .changed();
        ui.add_space(6.0);
        icon_field(ui, theme, s, &mut l.icon, &mut changed);
    }
    ui.add_space(8.0);
    ui.separator();
    ui.label(
        egui::RichText::new(s.t("browser-secrets-note"))
            .size(11.0)
            .color(theme.text_muted),
    );
    ui.add_space(4.0);
    for (slot, label_key, field) in [
        (PASSWORD, "browser-password", 0usize),
        (PASSKEY, "browser-passkey", 1),
        (API_KEY, "browser-apikey", 2),
    ] {
        let _ = field;
        secret_field(t, ui, cx, theme, i, slot, label_key, locked);
        ui.add_space(6.0);
    }
    if changed {
        t.side.passwords_dirty = true;
    }
}

/// One secret field. The stored value is a vault reference; the buffer is
/// what the user is typing. Revealing reads the vault, saving writes a new
/// secret and keeps only the reference.
#[allow(clippy::too_many_arguments)]
fn secret_field(
    t: &mut BrowserTool,
    ui: &mut egui::Ui,
    cx: &mut ToolContext,
    theme: &Theme,
    index: usize,
    slot: usize,
    label_key: &str,
    locked: bool,
) {
    let s = cx.services;
    let stored = secret_ref(&t.passwords.items[index], slot).to_string();
    ui.label(egui::RichText::new(s.t(label_key)).size(11.0));
    let mut reveal = false;
    let mut store = false;
    let mut clear = false;
    ui.horizontal(|ui| {
        let buf = &mut t.side.secrets[slot];
        let shown = t.side.revealed[slot];
        let hint = if stored.is_empty() {
            s.t("browser-secret-empty")
        } else {
            s.t("browser-secret-stored")
        };
        ui.add(
            egui::TextEdit::singleline(buf)
                .password(!shown)
                .hint_text(hint)
                .desired_width(ui.available_width() - 96.0),
        );
        if !stored.is_empty()
            && ui
                .small_button(ph::EYE)
                .on_hover_text(s.t("browser-reveal"))
                .clicked()
        {
            reveal = true;
        }
        if ui
            .small_button(ph::FLOPPY_DISK)
            .on_hover_text(s.t("browser-store-secret"))
            .clicked()
        {
            store = true;
        }
        if !stored.is_empty()
            && ui
                .small_button(ph::TRASH)
                .on_hover_text(s.t("browser-forget-secret"))
                .clicked()
        {
            clear = true;
        }
    });
    if locked && (reveal || store) {
        cx.toast(ToastLevel::Warning, s.t("browser-vault-locked"));
        return;
    }
    if reveal {
        match hs_core::secrets::SecretRef::parse(&stored) {
            Some(r) => match s.secrets.get(&r) {
                Ok(v) => {
                    use secrecy::ExposeSecret;
                    t.side.secrets[slot] = v.expose_secret().to_string();
                    t.side.revealed[slot] = true;
                }
                Err(e) => cx.toast(ToastLevel::Error, e.to_string()),
            },
            None => cx.toast(ToastLevel::Error, s.t("browser-secret-bad-ref")),
        }
    }
    if store {
        let value = t.side.secrets[slot].clone();
        if value.is_empty() {
            cx.toast(ToastLevel::Warning, s.t("browser-secret-empty"));
            return;
        }
        let label = format!("{}: {}", t.passwords.items[index].name, s.t(label_key));
        match s
            .secrets
            .create(&label, &secrecy::SecretString::from(value))
        {
            Ok(r) => {
                *secret_ref_mut(&mut t.passwords.items[index], slot) = r.to_string_ref();
                t.side.secrets[slot].clear();
                t.side.revealed[slot] = false;
                save_passwords(t, cx);
                cx.toast(ToastLevel::Success, cx.services.t("browser-secret-saved"));
            }
            Err(e) => cx.toast(ToastLevel::Error, e.to_string()),
        }
    }
    if clear {
        secret_ref_mut(&mut t.passwords.items[index], slot).clear();
        t.side.secrets[slot].clear();
        t.side.revealed[slot] = false;
        save_passwords(t, cx);
    }
    let _ = theme;
}

fn secret_ref(l: &Login, slot: usize) -> &str {
    match slot {
        PASSWORD => &l.password,
        PASSKEY => &l.passkey,
        _ => &l.api_key,
    }
}

fn secret_ref_mut(l: &mut Login, slot: usize) -> &mut String {
    match slot {
        PASSWORD => &mut l.password,
        PASSKEY => &mut l.passkey,
        _ => &mut l.api_key,
    }
}

/// Add the open page to the favourites, in `folder` (`None` = the root).
pub fn add_favorite(t: &mut BrowserTool, cx: &mut ToolContext, folder: Option<String>) {
    let (name, url) = {
        let p = t.page();
        let name = if p.title.is_empty() {
            p.url.clone()
        } else {
            p.title.clone()
        };
        (name, p.url.clone())
    };
    if url.is_empty() {
        return;
    }
    let fav = Favorite {
        name,
        url,
        ..Default::default()
    };
    if let Some(items) = t.favorites.items_mut(folder.as_deref()) {
        items.push(FavItem::Link(fav));
        save_favorites(t, cx);
        cx.toast(ToastLevel::Success, cx.services.t("browser-favorite-added"));
    }
}

// --------------------------------------------------------------------- //
// Add to Favorites
// --------------------------------------------------------------------- //

/// The Add to Favorites dialog's own state: which folder is chosen, and
/// the name of a new one if the user is making one.
#[derive(Default)]
pub struct AddFavorite {
    /// `None` = the top level.
    pub folder: Option<String>,
    pub making_folder: bool,
    pub new_folder: String,
}

pub fn add_favorite_dialog(
    t: &mut BrowserTool,
    ui: &mut egui::Ui,
    cx: &mut ToolContext,
    theme: &Theme,
) {
    if t.adding_favorite.is_none() {
        return;
    }
    let s = cx.services;
    let folders = t.favorites.folders();
    let page_url = t.page().url.clone();
    let mut close = false;
    let mut add: Option<Option<String>> = None;
    let mut make: Option<String> = None;
    let ctx = ui.ctx().clone();
    egui::Modal::new(egui::Id::new("browser-add-favorite")).show(&ctx, |ui| {
        ui.set_width(420.0);
        ui.label(
            egui::RichText::new(s.t("browser-add-favorite-title"))
                .strong()
                .size(15.0),
        );
        ui.add_space(6.0);
        if page_url.is_empty() {
            ui.colored_label(theme.text_muted, s.t("browser-fill-no-page"));
        } else {
            ui.colored_label(theme.text_muted, &page_url);
        }
        ui.add_space(10.0);
        ui.label(s.t("browser-add-favorite-where"));
        ui.add_space(4.0);
        let d = t.adding_favorite.as_mut().expect("checked above");
        egui::ScrollArea::vertical()
            .max_height(160.0)
            .show(ui, |ui| {
                if ui
                    .add(egui::Button::selectable(
                        d.folder.is_none() && !d.making_folder,
                        s.t("browser-add-favorite-root"),
                    ))
                    .clicked()
                {
                    d.folder = None;
                    d.making_folder = false;
                }
                for (id, path) in &folders {
                    if ui
                        .add(egui::Button::selectable(
                            d.folder.as_deref() == Some(id.as_str()) && !d.making_folder,
                            format!("{} {path}", ph::FOLDER),
                        ))
                        .clicked()
                    {
                        d.folder = Some(id.clone());
                        d.making_folder = false;
                    }
                }
                if ui
                    .add(egui::Button::selectable(
                        d.making_folder,
                        s.t("browser-add-favorite-new"),
                    ))
                    .clicked()
                {
                    d.making_folder = true;
                }
            });
        if d.making_folder {
            ui.add_space(6.0);
            ui.add(
                egui::TextEdit::singleline(&mut d.new_folder)
                    .hint_text(s.t("browser-add-favorite-new-hint"))
                    .desired_width(f32::INFINITY),
            );
        }
        ui.add_space(12.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if form::primary_button(ui, theme, &s.t("browser-add")).clicked() {
                if d.making_folder {
                    let name = d.new_folder.trim().to_string();
                    if !name.is_empty() {
                        make = Some(name);
                    }
                } else {
                    add = Some(d.folder.clone());
                }
            }
            if ui.button(s.t("cancel")).clicked() {
                close = true;
            }
        });
    });
    if let Some(name) = make {
        // A new folder at the top level, then the page goes into it.
        let f = FavFolder {
            name,
            ..Default::default()
        };
        let id = f.id.clone();
        t.favorites.items.push(FavItem::Folder(f));
        save_favorites(t, cx);
        add = Some(Some(id));
    }
    if let Some(folder) = add {
        add_favorite(t, cx, folder);
        close = true;
    }
    if close {
        t.adding_favorite = None;
    }
}

// --------------------------------------------------------------------- //
// Fill in Password
// --------------------------------------------------------------------- //

/// The Fill in Password dialog's state.
#[derive(Default)]
pub struct Filling {
    /// Which login the user picked, when the host has more than one.
    pub chosen: Option<String>,
}

pub fn fill_password_dialog(
    t: &mut BrowserTool,
    ui: &mut egui::Ui,
    cx: &mut ToolContext,
    theme: &Theme,
) {
    if t.filling.is_none() {
        return;
    }
    let s = cx.services;
    let url = t.page().url.clone();
    let host = crate::store::host_of(&url).unwrap_or_default();
    let matches: Vec<(String, String, String)> = t
        .passwords
        .for_host(&host)
        .into_iter()
        .map(|l| (l.id.clone(), l.name.clone(), l.username.clone()))
        .collect();
    let mut close = false;
    let mut fill: Option<String> = None;
    let ctx = ui.ctx().clone();
    egui::Modal::new(egui::Id::new("browser-fill-password")).show(&ctx, |ui| {
        ui.set_width(420.0);
        ui.label(
            egui::RichText::new(s.t("browser-fill-title"))
                .strong()
                .size(15.0),
        );
        ui.add_space(8.0);
        if url.is_empty() {
            ui.colored_label(theme.text_muted, s.t("browser-fill-no-page"));
        } else if matches.is_empty() {
            ui.colored_label(
                theme.text_muted,
                s.i18n.t_args("browser-fill-none", &[("host", &host)]),
            );
        } else {
            ui.label(s.t("browser-fill-pick"));
            ui.add_space(4.0);
            for (id, name, user) in &matches {
                let label = if user.is_empty() {
                    name.clone()
                } else {
                    format!("{name} \u{b7} {user}")
                };
                if ui.add(egui::Button::selectable(false, label)).clicked() {
                    fill = Some(id.clone());
                }
            }
        }
        if s.secrets.is_locked() {
            ui.add_space(6.0);
            ui.colored_label(theme.warning, s.t("browser-vault-locked"));
        }
        ui.add_space(12.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button(s.t("cancel")).clicked() {
                close = true;
            }
        });
    });
    if let Some(id) = fill {
        fill_login(t, cx, &id);
        close = true;
    }
    if close {
        t.filling = None;
    }
}

/// Put a login's values into the page's own fields.
///
/// What this reaches and what it cannot, deliberately:
///
/// * **Form fields** - a username, password or API-key input in the page's
///   DOM. These are filled, and `input`/`change` events are dispatched so a
///   framework mirroring the field into its own state notices.
/// * **HTTP basic authentication** is a native WebView2 dialog, outside the
///   page, and no script can answer it.
/// * **Passkeys** are WebAuthn: the browser and an authenticator agree a
///   signature, and a stored string cannot stand in for one. That field is
///   a place to keep recovery material, not something to type into a
///   prompt.
fn fill_login(t: &mut BrowserTool, cx: &mut ToolContext, id: &str) {
    let s = cx.services;
    let Some(l) = t.passwords.items.iter().find(|l| l.id == id).cloned() else {
        return;
    };
    if (!l.password.is_empty() || !l.api_key.is_empty()) && s.secrets.is_locked() {
        cx.toast(ToastLevel::Warning, s.t("browser-vault-locked"));
        return;
    }
    let reveal = |r: &str| -> String {
        hs_core::secrets::SecretRef::parse(r)
            .and_then(|r| s.secrets.get(&r).ok())
            .map(|v| {
                use secrecy::ExposeSecret;
                v.expose_secret().to_string()
            })
            .unwrap_or_default()
    };
    let password = reveal(&l.password);
    let api_key = reveal(&l.api_key);
    let page = t.page().id;
    let js = fill_script(&l.username, &password, &api_key);
    t.pending.push(crate::Cmd::Script(page, js));
    cx.toast(
        ToastLevel::Success,
        s.i18n.t_args("browser-fill-done", &[("name", &l.name)]),
    );
}

/// The script that does the filling. Its own function so the matching
/// rules can be read, and tested, without a browser.
fn fill_script(username: &str, password: &str, api_key: &str) -> String {
    // JSON is a subset of JavaScript literals, so serialising each value
    // quotes and escapes it correctly: a password holding a quote or a
    // backslash must not be able to end the string it sits in.
    let u = serde_json::to_string(username).unwrap_or_else(|_| "\"\"".into());
    let p = serde_json::to_string(password).unwrap_or_else(|_| "\"\"".into());
    let k = serde_json::to_string(api_key).unwrap_or_else(|_| "\"\"".into());
    let body = r#"
  const set = (el, v) => {
    if (!el || !v) return false;
    const proto = Object.getPrototypeOf(el);
    const d = Object.getOwnPropertyDescriptor(proto, 'value');
    if (d && d.set) { d.set.call(el, v); } else { el.value = v; }
    el.dispatchEvent(new Event('input', { bubbles: true }));
    el.dispatchEvent(new Event('change', { bubbles: true }));
    return true;
  };
  const visible = (el) => {
    const r = el.getBoundingClientRect();
    return r.width > 0 && r.height > 0;
  };
  const pick = (selectors) => {
    for (const sel of selectors) {
      for (const el of document.querySelectorAll(sel)) {
        if (!el.disabled && !el.readOnly && visible(el)) return el;
      }
    }
    return null;
  };
  const pw = pick(['input[type=password]']);
  let user = null;
  if (pw && pw.form) {
    const fields = Array.from(pw.form.querySelectorAll('input'));
    const i = fields.indexOf(pw);
    for (let j = i - 1; j >= 0; j--) {
      const ty = (fields[j].type || 'text').toLowerCase();
      if (['text','email','tel'].includes(ty) && visible(fields[j])) { user = fields[j]; break; }
    }
  }
  if (!user) {
    user = pick([
      'input[autocomplete=username]', 'input[name*=user i]', 'input[id*=user i]',
      'input[name*=email i]', 'input[id*=email i]', 'input[type=email]'
    ]);
  }
  const key = pick([
    'input[name*=api i]', 'input[id*=api i]',
    'input[name*=token i]', 'input[id*=token i]'
  ]);
  let n = 0;
"#;
    format!(
        "(function(){{{body}  if (set(user, {u})) n++;\n  if (set(pw, {p})) n++;\n  if (set(key, {k})) n++;\n  return n;\n}})()"
    )
}

#[cfg(test)]
mod dialog_tests {
    use super::*;

    #[test]
    fn a_filled_value_cannot_escape_the_string_it_sits_in() {
        let js = fill_script("me", "a\"b\\c\nd", "");
        // Serialised, not pasted: the quote and backslash are escaped and
        // the newline never breaks the literal.
        assert!(js.contains(r#""a\"b\\c\nd""#), "{js}");
        assert!(!js.contains("a\"b\\c\nd"));
    }

    #[test]
    fn the_script_anchors_on_a_password_field_before_guessing_names() {
        let js = fill_script("u", "p", "k");
        let pw = js.find("input[type=password]").expect("looks for one");
        let guess = js.find("input[name*=user i]").expect("falls back");
        assert!(pw < guess, "the password field anchors the form");
    }

    #[test]
    fn an_empty_value_is_not_filled_in() {
        // `set` refuses a falsy value, so a login with no API key leaves
        // whatever the page already had alone.
        let js = fill_script("u", "p", "");
        assert!(js.contains("if (!el || !v) return false;"));
        assert!(js.contains(r#"set(key, "")"#), "{js}");
    }
}
