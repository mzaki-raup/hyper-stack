//! The table designer (FEAT-0103, docs/tools/database.md): a tab inside a
//! connection's tab where a table is edited as a form - columns, indexes,
//! foreign keys, the primary key, the name - with the ALTER (or CREATE)
//! script shown underneath before anything runs. **Apply** hands the script
//! to the connection's own run path, so it is guarded, cancellable and
//! transactional exactly as a typed script is; what an engine cannot do in
//! place is said in the script as a refusal and keeps Apply off.

use hs_core::services::Services;
use hs_editor::view::EditorTheme;
use hs_editor::{Buffer, Cursors, EditorView, languages};
use hs_ui::form;
use hs_ui::icons::ph;
use hs_ui::table::{EditColumn, edit_table};
use hs_ui::theme::Theme;
use hs_ui::tip::TipExt;

use crate::ddlgen::{self, Edit, Script};
use crate::driver::Engine;
use crate::schema::{ColumnDef, ForeignKey, IndexDef, TableSchema};

/// Which part of the table is being edited.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sub {
    Columns,
    Indexes,
    Keys,
}

/// What the pane asks of the connection tab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Run this script through the query path, then read the table again.
    Apply(String),
    /// Read the table again (Reload, or after a run).
    Reload,
}

pub struct Designer {
    pub uid: u64,
    pub schema: String,
    /// The table as read; `None` for New Table, or while it loads.
    before: Option<TableSchema>,
    /// The name the table was opened by, for the reload.
    original_name: Option<String>,
    pub edit: Edit,
    pub loading: bool,
    pub error: Option<String>,
    sub: Sub,
    /// Bumped by every edit; the script is regenerated when it moves.
    rev: u64,
    script_rev: u64,
    script: Script,
    view: EditorView,
    buf: Buffer,
    cursors: Cursors,
    /// For the view that is made anew with each script.
    dark: bool,
    font_size: f32,
    /// The typed confirmation for a script that drops something.
    confirm: Option<String>,
    /// Apply was sent: when the run ends, the table is read again.
    pub awaiting_reload: bool,
    /// The connection's run counter when Apply was sent; the reload waits
    /// for it to move, so the run guard's own confirmation does not count
    /// as the run having ended.
    pub apply_run: Option<u64>,
    /// Comma-joined column lists edited as text, per index and key row,
    /// so typing a comma does not reshape the list under the caret.
    index_cols: Vec<String>,
    key_cols: Vec<(String, String)>,
}

impl Designer {
    pub fn new(uid: u64, services: &Services, schema: &str, table: Option<&str>) -> Self {
        let st = services.settings.read();
        let dark = Theme::from_settings(&st.appearance).dark;
        let font_size = st.editor.font_size;
        let right_click = st.editor.right_click_copy_paste;
        drop(st);
        let view = script_view(uid, dark, font_size, right_click);
        let mut buf = Buffer::from_text("");
        buf.mark_saved();
        Self {
            uid,
            schema: schema.to_string(),
            before: None,
            original_name: table.map(str::to_string),
            edit: Edit::blank(schema),
            loading: table.is_some(),
            error: None,
            sub: Sub::Columns,
            rev: 1,
            script_rev: 0,
            script: Script::default(),
            view,
            buf,
            cursors: Cursors::single(0),
            dark,
            font_size,
            confirm: None,
            awaiting_reload: false,
            apply_run: None,
            index_cols: Vec::new(),
            key_cols: Vec::new(),
        }
    }

    /// The table this designer edits, as it was opened: `None` for New Table.
    pub fn original_name(&self) -> Option<&str> {
        self.original_name.as_deref()
    }

    /// What the tab says.
    pub fn title(&self, design: &str, new_table: &str) -> String {
        match &self.original_name {
            Some(n) => format!("{design}: {n}"),
            None => new_table.to_string(),
        }
    }

    /// The table as read arrived (or arrived again after Apply).
    pub fn set_table(&mut self, t: TableSchema) {
        self.original_name = Some(t.name.clone());
        self.edit = Edit::from_table(&t);
        self.index_cols = t.indexes.iter().map(|i| i.columns.join(", ")).collect();
        self.key_cols = t
            .foreign_keys
            .iter()
            .map(|k| (k.columns.join(", "), k.ref_columns.join(", ")))
            .collect();
        self.before = Some(t);
        self.loading = false;
        self.error = None;
        self.awaiting_reload = false;
        self.rev += 1;
    }

    /// A New Table designer whose CREATE has run becomes that table's
    /// designer, so the next edit is an ALTER and the table is read back.
    pub fn adopt_created(&mut self) {
        if self.original_name.is_none() && !self.edit.table.name.trim().is_empty() {
            self.original_name = Some(self.edit.table.name.trim().to_string());
            self.loading = true;
        }
    }

    pub fn fail(&mut self, e: String) {
        self.loading = false;
        self.awaiting_reload = false;
        self.error = Some(e);
    }

    /// Back to the table as read (or an empty new table).
    fn discard(&mut self) {
        match &self.before {
            Some(t) => {
                let t = t.clone();
                self.set_table(t);
            }
            None => {
                self.edit = Edit::blank(&self.schema);
                self.index_cols.clear();
                self.key_cols.clear();
                self.rev += 1;
            }
        }
    }

    /// Regenerate the script when an edit happened since the last one.
    fn refresh_script(&mut self, engine: Engine) {
        if self.script_rev == self.rev {
            return;
        }
        self.script_rev = self.rev;
        self.script = ddlgen::alter_table(self.before.as_ref(), &self.edit, engine);
        // A whole new view, not a new buffer under the old one: the view
        // caches per-line spans by buffer version and a fresh buffer
        // starts its versions again (docs/GOTCHAS.md, the query tabs).
        let right_click = self.view.right_click_copy_paste;
        self.view = script_view(self.uid, self.dark, self.font_size, right_click);
        self.buf = Buffer::from_text(&self.script.text());
        self.buf.mark_saved();
        self.cursors = Cursors::single(0);
    }

    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        theme: &Theme,
        s: &Services,
        engine: Engine,
        catalog: &[(String, String, Vec<String>)],
        read_only: bool,
    ) -> Option<Action> {
        form::pane(ui, |ui| self.body(ui, theme, s, engine, catalog, read_only))
    }

    #[allow(clippy::too_many_arguments)] // reason: the pane's whole context, passed through from `ui` (CR-0061)
    fn body(
        &mut self,
        ui: &mut egui::Ui,
        theme: &Theme,
        s: &Services,
        engine: Engine,
        catalog: &[(String, String, Vec<String>)],
        read_only: bool,
    ) -> Option<Action> {
        let mut action = None;
        if self.loading {
            form::hint(ui, theme, &s.t("db-designer-loading"));
            return None;
        }
        if let Some(e) = &self.error {
            form::note(ui, theme, ph::WARNING, e);
            if self.original_name.is_some()
                && ui
                    .button(format!(
                        "{} {}",
                        ph::ARROW_CLOCKWISE,
                        s.t("db-designer-reload")
                    ))
                    .clicked()
            {
                return Some(Action::Reload);
            }
            return None;
        }
        // Name and schema.
        let mut changed = false;
        // Centred, so the schema prefix sits on the field's middle; still
        // wraps when narrow (CR-0061).
        form::centred_row_wrapped(ui, |ui| {
            if !self.schema.is_empty() {
                ui.label(egui::RichText::new(format!("{}.", self.schema)).color(theme.text_muted));
            }
            changed |=
                form::text_field(ui, &mut self.edit.table.name, &s.t("db-designer-name-hint"))
                    .changed();
            ui.add_space(12.0);
            let material = s.settings.read().appearance.material_icons("db");
            for (sub, key, icon, kind) in [
                (
                    Sub::Columns,
                    "db-designer-columns",
                    ph::COLUMNS,
                    Some("column"),
                ),
                (Sub::Indexes, "db-designer-indexes", ph::LIST_NUMBERS, None),
                (Sub::Keys, "db-designer-keys", ph::LINK, Some("key")),
            ] {
                // Material icons on: the picture the schema tree gives a
                // column or a key; Indexes has none and keeps its glyph.
                let button = match kind
                    .filter(|_| material)
                    .and_then(hs_ui::fileicons::for_kind)
                {
                    Some(src) => egui::Button::selectable(
                        self.sub == sub,
                        (
                            egui::Image::new(src).fit_to_exact_size(egui::Vec2::splat(14.0)),
                            s.t(key),
                        ),
                    ),
                    None => {
                        egui::Button::selectable(self.sub == sub, format!("{icon} {}", s.t(key)))
                    }
                };
                if hs_ui::form::add_selectable(ui, button).clicked() {
                    self.sub = sub;
                }
            }
        });
        ui.add_space(form::PANE_GAP);
        ui.separator();
        ui.add_space(form::PANE_GAP);
        let total_h = ui.available_height();
        let form_h = (total_h * 0.55).max(120.0);
        ui.allocate_ui(egui::vec2(ui.available_width(), form_h), |ui| {
            egui::ScrollArea::both()
                .id_salt(("db-designer-form", self.uid))
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    changed |= match self.sub {
                        Sub::Columns => self.columns_ui(ui, theme, s, engine),
                        Sub::Indexes => self.indexes_ui(ui, theme, s),
                        Sub::Keys => self.keys_ui(ui, theme, s, catalog),
                    };
                });
        });
        if changed {
            self.rev += 1;
        }
        self.refresh_script(engine);
        // The script, its refusals, and the buttons.
        ui.add_space(form::PANE_GAP);
        ui.separator();
        ui.add_space(form::PANE_GAP);
        for r in &self.script.refused {
            form::note(ui, theme, ph::WARNING, r);
            ui.add_space(form::PANE_GAP);
        }
        let can_apply = !self.script.statements.is_empty() && self.script.refused.is_empty();
        form::centred_row_wrapped(ui, |ui| {
            ui.label(egui::RichText::new(s.t("db-designer-script")).strong());
            ui.add_space(12.0);
            let apply = ui
                .add_enabled(
                    can_apply && !read_only,
                    egui::Button::new(format!("{} {}", ph::PLAY, s.t("db-designer-apply"))),
                )
                .tip(if read_only {
                    s.t("db-designer-read-only")
                } else if !self.script.refused.is_empty() {
                    s.t("db-designer-refused")
                } else if self.script.statements.is_empty() {
                    s.t("db-designer-no-changes")
                } else {
                    s.t("db-designer-apply-tip")
                });
            if apply.clicked() {
                if ddlgen::drops_something(&self.script) {
                    self.confirm = Some(String::new());
                } else {
                    action = Some(Action::Apply(self.script.statements.join("\n")));
                }
            }
            if ui
                .add_enabled(
                    !self.script.statements.is_empty(),
                    egui::Button::new(format!("{} {}", ph::COPY, s.t("db-designer-copy"))),
                )
                .clicked()
            {
                ui.ctx().copy_text(self.script.statements.join("\n"));
            }
            if self.original_name.is_some()
                && ui
                    .button(format!(
                        "{} {}",
                        ph::ARROW_CLOCKWISE,
                        s.t("db-designer-reload")
                    ))
                    .clicked()
            {
                action = Some(Action::Reload);
            }
            if ui
                .button(format!(
                    "{} {}",
                    ph::ARROW_COUNTER_CLOCKWISE,
                    s.t("db-designer-discard")
                ))
                .clicked()
            {
                self.discard();
            }
        });
        ui.add_space(form::PANE_GAP);
        let ed_theme = EditorTheme::for_theme(theme);
        form::well(ui, theme, None, |ui| {
            self.view
                .show(ui, &mut self.buf, &mut self.cursors, &ed_theme);
        });
        if let Some(a) = self.confirm_ui(ui, theme, s) {
            action = Some(a);
        }
        action
    }

    /// The typed confirmation for a script that drops a column, a key or
    /// the table (guarded, not forbidden).
    fn confirm_ui(&mut self, ui: &mut egui::Ui, theme: &Theme, s: &Services) -> Option<Action> {
        let name = self
            .original_name
            .clone()
            .unwrap_or_else(|| self.edit.table.name.clone());
        let typed = self.confirm.as_mut()?;
        let mut go = false;
        let mut cancel = false;
        let m = egui::Modal::new(egui::Id::new(("db-designer-confirm", self.uid))).show(
            ui.ctx(),
            |ui| {
                ui.set_width(440.0);
                ui.heading(s.t("db-designer-confirm-title"));
                ui.add_space(6.0);
                ui.label(
                    s.i18n
                        .t_args("db-designer-confirm-body", &[("name", name.as_str())]),
                );
                ui.add_space(6.0);
                form::hint(ui, theme, &s.t("db-designer-confirm-hint"));
                let r = form::text_field(ui, typed, &name);
                if r.lost_focus()
                    && ui.input(|i| i.key_pressed(egui::Key::Enter))
                    && typed.trim() == name
                {
                    go = true;
                }
                r.request_focus();
                ui.add_space(12.0);
                // Icon and label on both, in a row that wraps (CR-0061).
                ui.horizontal_wrapped(|ui| {
                    let ok = egui::Button::new(
                        egui::RichText::new(format!("{} {}", ph::PLAY, s.t("db-designer-apply")))
                            .color(egui::Color32::WHITE),
                    )
                    .fill(theme.error);
                    if ui.add_enabled(typed.trim() == name, ok).clicked() {
                        go = true;
                    }
                    if ui.button(format!("{} {}", ph::X, s.t("cancel"))).clicked() {
                        cancel = true;
                    }
                });
            },
        );
        if m.should_close() {
            cancel = true;
        }
        if cancel {
            self.confirm = None;
            return None;
        }
        if go {
            self.confirm = None;
            return Some(Action::Apply(self.script.statements.join("\n")));
        }
        None
    }

    fn columns_ui(
        &mut self,
        ui: &mut egui::Ui,
        theme: &Theme,
        s: &Services,
        engine: Engine,
    ) -> bool {
        let mut changed = false;
        let types = type_suggestions(engine);
        let mut remove: Option<usize> = None;
        let mut swap: Option<(usize, usize)> = None;
        let n = self.edit.table.columns.len();
        let (h_name, h_type, h_null, h_default, h_auto, h_key) = (
            s.t("db-designer-col-name"),
            s.t("db-designer-col-type"),
            s.t("db-designer-col-null"),
            s.t("db-designer-col-default"),
            s.t("db-designer-col-auto"),
            s.t("db-designer-col-pk"),
        );
        let remove_tip = s.t("db-designer-remove");
        // The floors of the layout standard (.claude/rules/ui.md, CR-0061).
        let columns = [
            EditColumn::new(&h_name, 200.0),
            EditColumn::new(&h_type, 220.0),
            EditColumn::new(&h_null, 60.0),
            EditColumn::new(&h_default, 180.0),
            EditColumn::new(&h_auto, 60.0),
            EditColumn::new(&h_key, 60.0),
            EditColumn::new("", 0.0),
        ];
        let no_columns = s.t("db-designer-no-columns");
        let table = &mut self.edit.table;
        edit_table(
            ui,
            theme,
            ("db-designer-columns", self.uid),
            &columns,
            n,
            Some(&no_columns),
            |ui, i, col| {
                let w = ui.available_width() - 6.0;
                let c = &mut table.columns[i];
                match col {
                    0 => {
                        changed |= ui
                            .add(form::line_edit(&mut c.name).desired_width(w))
                            .changed()
                    }
                    1 => {
                        changed |=
                            form::pick_field_width(ui, theme, &mut c.data_type, &types, "", w)
                                .changed()
                    }
                    2 => changed |= ui.checkbox(&mut c.nullable, "").changed(),
                    3 => {
                        let mut d = c.default.clone().unwrap_or_default();
                        if ui.add(form::line_edit(&mut d).desired_width(w)).changed() {
                            c.default = (!d.trim().is_empty()).then_some(d);
                            changed = true;
                        }
                    }
                    4 => changed |= ui.checkbox(&mut c.auto_increment, "").changed(),
                    5 => {
                        let name = c.name.clone();
                        let mut pk = table.primary_key.contains(&name);
                        if ui.checkbox(&mut pk, "").changed() {
                            if pk {
                                table.primary_key.push(name);
                            } else {
                                table.primary_key.retain(|k| k != &name);
                            }
                            changed = true;
                        }
                    }
                    _ => {
                        // Both arrows always drawn (dimmed where they cannot
                        // move), so the actions stay in one aligned column.
                        let up = ui
                            .add_enabled_ui(i > 0, |ui| {
                                form::icon_button(ui, theme, ph::ARROW_UP, 14.0, false)
                            })
                            .inner;
                        if i > 0 && up.clicked() {
                            swap = Some((i, i - 1));
                        }
                        let down = ui
                            .add_enabled_ui(i + 1 < n, |ui| {
                                form::icon_button(ui, theme, ph::ARROW_DOWN, 14.0, false)
                            })
                            .inner;
                        if i + 1 < n && down.clicked() {
                            swap = Some((i, i + 1));
                        }
                        if form::icon_button(ui, theme, ph::X, 14.0, false)
                            .tip(&remove_tip)
                            .clicked()
                        {
                            remove = Some(i);
                        }
                    }
                }
            },
        );
        if let Some((a, b)) = swap {
            self.edit.table.columns.swap(a, b);
            self.edit.original.swap(a, b);
            changed = true;
        }
        if let Some(i) = remove {
            let gone = self.edit.table.columns.remove(i);
            self.edit.original.remove(i);
            self.edit.table.primary_key.retain(|k| k != &gone.name);
            changed = true;
        }
        ui.add_space(form::ROW_GAP);
        if ui
            .button(format!("{} {}", ph::PLUS, s.t("db-designer-add-column")))
            .clicked()
        {
            self.edit.table.columns.push(ColumnDef {
                name: format!("column_{}", n + 1),
                data_type: types.first().cloned().unwrap_or_else(|| "text".into()),
                nullable: true,
                default: None,
                auto_increment: false,
            });
            self.edit.original.push(None);
            changed = true;
        }
        changed
    }

    fn indexes_ui(&mut self, ui: &mut egui::Ui, theme: &Theme, s: &Services) -> bool {
        let mut changed = false;
        let mut remove: Option<usize> = None;
        while self.index_cols.len() < self.edit.table.indexes.len() {
            let i = self.index_cols.len();
            self.index_cols
                .push(self.edit.table.indexes[i].columns.join(", "));
        }
        let (h_name, h_cols, h_unique, primary) = (
            s.t("db-designer-ix-name"),
            s.t("db-designer-ix-columns"),
            s.t("db-designer-ix-unique"),
            s.t("db-designer-ix-primary"),
        );
        let columns = [
            EditColumn::new(&h_name, 200.0),
            EditColumn::new(&h_cols, 200.0),
            EditColumn::new(&h_unique, 120.0),
            EditColumn::new("", 0.0),
        ];
        let own_cols: Vec<String> = self
            .edit
            .table
            .columns
            .iter()
            .map(|c| c.name.clone())
            .filter(|n| !n.trim().is_empty())
            .collect();
        let n = self.edit.table.indexes.len();
        let no_indexes = s.t("db-designer-no-indexes");
        let (indexes, index_cols) = (&mut self.edit.table.indexes, &mut self.index_cols);
        edit_table(
            ui,
            theme,
            ("db-designer-indexes", self.uid),
            &columns,
            n,
            Some(&no_indexes),
            |ui, i, col| {
                let w = ui.available_width() - 6.0;
                let ix = &mut indexes[i];
                if ix.primary {
                    // The primary key's own index is shown and left alone.
                    let text = match col {
                        0 => ix.name.clone(),
                        1 => ix.columns.join(", "),
                        2 => primary.clone(),
                        _ => String::new(),
                    };
                    ui.label(egui::RichText::new(text).color(theme.text_muted));
                    return;
                }
                match col {
                    0 => {
                        changed |= ui
                            .add(form::line_edit(&mut ix.name).desired_width(w))
                            .changed()
                    }
                    1 => {
                        if form::multi_pick_field_width(
                            ui,
                            theme,
                            &mut index_cols[i],
                            &own_cols,
                            "a, b",
                            w,
                        )
                        .changed()
                        {
                            ix.columns = split_list(&index_cols[i]);
                            changed = true;
                        }
                    }
                    2 => changed |= ui.checkbox(&mut ix.unique, "").changed(),
                    _ => {
                        if form::icon_button(ui, theme, ph::X, 14.0, false).clicked() {
                            remove = Some(i);
                        }
                    }
                }
            },
        );
        if let Some(i) = remove {
            self.edit.table.indexes.remove(i);
            self.index_cols.remove(i);
            changed = true;
        }
        ui.add_space(form::ROW_GAP);
        if ui
            .button(format!("{} {}", ph::PLUS, s.t("db-designer-add-index")))
            .clicked()
        {
            let n = self.edit.table.indexes.len() + 1;
            self.edit.table.indexes.push(IndexDef {
                name: format!("{}_ix{n}", self.edit.table.name),
                columns: Vec::new(),
                unique: false,
                primary: false,
            });
            self.index_cols.push(String::new());
            changed = true;
        }
        changed
    }

    fn keys_ui(
        &mut self,
        ui: &mut egui::Ui,
        theme: &Theme,
        s: &Services,
        catalog: &[(String, String, Vec<String>)],
    ) -> bool {
        let mut changed = false;
        let mut remove: Option<usize> = None;
        while self.key_cols.len() < self.edit.table.foreign_keys.len() {
            let i = self.key_cols.len();
            let k = &self.edit.table.foreign_keys[i];
            self.key_cols
                .push((k.columns.join(", "), k.ref_columns.join(", ")));
        }
        let tables: Vec<String> = catalog
            .iter()
            .map(|(sch, t, _)| {
                if sch.is_empty() {
                    t.clone()
                } else {
                    format!("{sch}.{t}")
                }
            })
            .collect();
        let actions: Vec<String> = ["", "CASCADE", "SET NULL", "RESTRICT", "NO ACTION"]
            .iter()
            .map(|a| a.to_string())
            .collect();
        let heads = [
            s.t("db-designer-fk-name"),
            s.t("db-designer-fk-columns"),
            s.t("db-designer-fk-table"),
            s.t("db-designer-fk-ref-columns"),
            s.t("db-designer-fk-on-delete"),
            s.t("db-designer-fk-on-update"),
        ];
        let columns = [
            EditColumn::new(&heads[0], 200.0),
            EditColumn::new(&heads[1], 170.0),
            EditColumn::new(&heads[2], 200.0),
            EditColumn::new(&heads[3], 170.0),
            EditColumn::new(&heads[4], 130.0),
            EditColumn::new(&heads[5], 130.0),
            EditColumn::new("", 0.0),
        ];
        let own_cols: Vec<String> = self
            .edit
            .table
            .columns
            .iter()
            .map(|c| c.name.clone())
            .filter(|n| !n.trim().is_empty())
            .collect();
        let n = self.edit.table.foreign_keys.len();
        let no_keys = s.t("db-designer-no-keys");
        let (keys, key_cols) = (&mut self.edit.table.foreign_keys, &mut self.key_cols);
        edit_table(
            ui,
            theme,
            ("db-designer-keys", self.uid),
            &columns,
            n,
            Some(&no_keys),
            |ui, i, col| {
                let w = ui.available_width() - 6.0;
                let fk = &mut keys[i];
                match col {
                    0 => {
                        changed |= ui
                            .add(form::line_edit(&mut fk.name).desired_width(w))
                            .changed()
                    }
                    1 => {
                        if form::multi_pick_field_width(
                            ui,
                            theme,
                            &mut key_cols[i].0,
                            &own_cols,
                            "a, b",
                            w,
                        )
                        .changed()
                        {
                            fk.columns = split_list(&key_cols[i].0);
                            changed = true;
                        }
                    }
                    2 => {
                        let mut target = if fk.ref_schema.is_empty() {
                            fk.ref_table.clone()
                        } else {
                            format!("{}.{}", fk.ref_schema, fk.ref_table)
                        };
                        if form::pick_field_width(ui, theme, &mut target, &tables, "", w).changed()
                        {
                            match target.rsplit_once('.') {
                                Some((sch, t)) => {
                                    fk.ref_schema = sch.to_string();
                                    fk.ref_table = t.to_string();
                                }
                                None => {
                                    fk.ref_schema.clear();
                                    fk.ref_table = target.clone();
                                }
                            }
                            changed = true;
                        }
                    }
                    3 => {
                        let ref_cols: Vec<String> = catalog
                            .iter()
                            .find(|(sch, t, _)| {
                                t == &fk.ref_table
                                    && (sch == &fk.ref_schema || fk.ref_schema.is_empty())
                            })
                            .map(|(_, _, cols)| cols.clone())
                            .unwrap_or_default();
                        if form::multi_pick_field_width(
                            ui,
                            theme,
                            &mut key_cols[i].1,
                            &ref_cols,
                            "id",
                            w,
                        )
                        .changed()
                        {
                            fk.ref_columns = split_list(&key_cols[i].1);
                            changed = true;
                        }
                    }
                    4 => {
                        let mut od = fk.on_delete.clone().unwrap_or_default();
                        if form::pick_field_width(ui, theme, &mut od, &actions, "", w).changed() {
                            fk.on_delete = (!od.trim().is_empty()).then_some(od);
                            changed = true;
                        }
                    }
                    5 => {
                        let mut ou = fk.on_update.clone().unwrap_or_default();
                        if form::pick_field_width(ui, theme, &mut ou, &actions, "", w).changed() {
                            fk.on_update = (!ou.trim().is_empty()).then_some(ou);
                            changed = true;
                        }
                    }
                    _ => {
                        if form::icon_button(ui, theme, ph::X, 14.0, false).clicked() {
                            remove = Some(i);
                        }
                    }
                }
            },
        );
        if let Some(i) = remove {
            self.edit.table.foreign_keys.remove(i);
            self.key_cols.remove(i);
            changed = true;
        }
        ui.add_space(form::ROW_GAP);
        if ui
            .button(format!("{} {}", ph::PLUS, s.t("db-designer-add-key")))
            .clicked()
        {
            let n = self.edit.table.foreign_keys.len() + 1;
            self.edit.table.foreign_keys.push(ForeignKey {
                name: format!("{}_fk{n}", self.edit.table.name),
                ..Default::default()
            });
            self.key_cols.push((String::new(), String::new()));
            changed = true;
        }
        changed
    }
}

/// The read-only editor the script is shown in.
fn script_view(uid: u64, dark: bool, font_size: f32, right_click: bool) -> EditorView {
    let mut view = EditorView::new(
        egui::Id::new(("hs-db-designer", uid, hs_core::new_id())),
        languages::detect("script.sql"),
        dark,
        font_size,
    );
    view.read_only = true;
    view.right_click_copy_paste = right_click;
    view
}

/// `a, b ,c` → `["a", "b", "c"]`.
pub fn split_list(text: &str) -> Vec<String> {
    text.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

/// The types the type field offers first; anything else can still be typed.
pub fn type_suggestions(engine: Engine) -> Vec<String> {
    let list: &[&str] = match engine {
        Engine::Sqlite => &["INTEGER", "TEXT", "REAL", "BLOB", "NUMERIC"],
        Engine::Postgres => &[
            "integer",
            "bigint",
            "smallint",
            "serial",
            "bigserial",
            "numeric(10,2)",
            "real",
            "double precision",
            "text",
            "varchar(255)",
            "boolean",
            "date",
            "timestamp",
            "timestamptz",
            "uuid",
            "jsonb",
            "bytea",
        ],
        Engine::Mysql => &[
            "INT",
            "BIGINT",
            "SMALLINT",
            "TINYINT(1)",
            "DECIMAL(10,2)",
            "FLOAT",
            "DOUBLE",
            "VARCHAR(255)",
            "TEXT",
            "DATE",
            "DATETIME",
            "TIMESTAMP",
            "JSON",
            "BLOB",
        ],
        Engine::Mssql => &[
            "INT",
            "BIGINT",
            "SMALLINT",
            "BIT",
            "DECIMAL(10,2)",
            "FLOAT",
            "NVARCHAR(255)",
            "NVARCHAR(MAX)",
            "DATE",
            "DATETIME2",
            "UNIQUEIDENTIFIER",
            "VARBINARY(MAX)",
        ],
        Engine::ClickHouse => &[
            "UInt32",
            "UInt64",
            "Int32",
            "Int64",
            "Float64",
            "Decimal(10,2)",
            "String",
            "LowCardinality(String)",
            "Date",
            "DateTime",
            "Nullable(String)",
            "UUID",
        ],
        Engine::DuckDb => &[
            "INTEGER",
            "BIGINT",
            "DECIMAL(10,2)",
            "DOUBLE",
            "VARCHAR",
            "BOOLEAN",
            "DATE",
            "TIMESTAMP",
            "UUID",
            "BLOB",
        ],
        Engine::Oracle => &[
            "NUMBER",
            "NUMBER(10,2)",
            "VARCHAR2(255)",
            "CLOB",
            "DATE",
            "TIMESTAMP",
            "BLOB",
        ],
        Engine::Db2 => &[
            "INTEGER",
            "BIGINT",
            "DECIMAL(10,2)",
            "DOUBLE",
            "VARCHAR(255)",
            "CLOB",
            "DATE",
            "TIMESTAMP",
            "BLOB",
        ],
    };
    list.iter().map(|t| t.to_string()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn column_lists_split_on_commas_and_drop_blanks() {
        assert_eq!(split_list("a, b ,c,,"), vec!["a", "b", "c"]);
        assert!(split_list("  ").is_empty());
    }

    #[test]
    fn every_engine_offers_some_types() {
        for e in [
            Engine::Sqlite,
            Engine::Postgres,
            Engine::Mysql,
            Engine::Mssql,
            Engine::ClickHouse,
            Engine::DuckDb,
            Engine::Oracle,
            Engine::Db2,
        ] {
            assert!(!type_suggestions(e).is_empty(), "{e:?}");
        }
    }
}
