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
        let mut action = None;
        ui.add_space(form::PANE_GAP);
        if self.loading {
            form::hint(ui, theme, &s.t("db-designer-loading"));
            return None;
        }
        if let Some(e) = &self.error {
            form::note(ui, theme, ph::WARNING, e);
            if self.original_name.is_some() && ui.button(s.t("db-designer-reload")).clicked() {
                return Some(Action::Reload);
            }
            return None;
        }
        // Name and schema.
        let mut changed = false;
        ui.horizontal(|ui| {
            if !self.schema.is_empty() {
                ui.label(egui::RichText::new(format!("{}.", self.schema)).color(theme.text_muted));
            }
            changed |=
                form::text_field(ui, &mut self.edit.table.name, &s.t("db-designer-name-hint"))
                    .changed();
            ui.add_space(12.0);
            for (sub, key) in [
                (Sub::Columns, "db-designer-columns"),
                (Sub::Indexes, "db-designer-indexes"),
                (Sub::Keys, "db-designer-keys"),
            ] {
                if ui
                    .add(egui::Button::selectable(self.sub == sub, s.t(key)))
                    .clicked()
                {
                    self.sub = sub;
                }
            }
        });
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
        ui.separator();
        for r in &self.script.refused {
            form::note(ui, theme, ph::WARNING, r);
        }
        let can_apply = !self.script.statements.is_empty() && self.script.refused.is_empty();
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(s.t("db-designer-script"))
                    .small()
                    .strong(),
            );
            hs_ui::toolbar::hard_right(ui, |ui| {
                if ui.button(s.t("db-designer-discard")).clicked() {
                    self.discard();
                }
                if self.original_name.is_some() && ui.button(s.t("db-designer-reload")).clicked() {
                    action = Some(Action::Reload);
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
            });
        });
        let ed_theme = EditorTheme::for_theme(theme);
        self.view
            .show(ui, &mut self.buf, &mut self.cursors, &ed_theme);
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
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let ok = egui::Button::new(
                        egui::RichText::new(s.t("db-designer-apply")).color(egui::Color32::WHITE),
                    )
                    .fill(theme.error);
                    if ui.add_enabled(typed.trim() == name, ok).clicked() {
                        go = true;
                    }
                    if ui.button(s.t("cancel")).clicked() {
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
        egui::Grid::new(("db-designer-columns", self.uid))
            .num_columns(7)
            .spacing([8.0, 4.0])
            .striped(true)
            .show(ui, |ui| {
                for key in [
                    "db-designer-col-name",
                    "db-designer-col-type",
                    "db-designer-col-null",
                    "db-designer-col-default",
                    "db-designer-col-auto",
                    "db-designer-col-pk",
                ] {
                    ui.label(egui::RichText::new(s.t(key)).small().strong());
                }
                ui.label("");
                ui.end_row();
                for i in 0..n {
                    let c = &mut self.edit.table.columns[i];
                    changed |= ui
                        .add_sized([150.0, 22.0], egui::TextEdit::singleline(&mut c.name))
                        .changed();
                    changed |= form::pick_field(ui, theme, &mut c.data_type, &types, "").changed();
                    changed |= ui.checkbox(&mut c.nullable, "").changed();
                    let mut d = c.default.clone().unwrap_or_default();
                    if ui
                        .add_sized([130.0, 22.0], egui::TextEdit::singleline(&mut d))
                        .changed()
                    {
                        c.default = (!d.trim().is_empty()).then_some(d);
                        changed = true;
                    }
                    changed |= ui.checkbox(&mut c.auto_increment, "").changed();
                    let name = c.name.clone();
                    let mut pk = self.edit.table.primary_key.contains(&name);
                    if ui.checkbox(&mut pk, "").changed() {
                        if pk {
                            self.edit.table.primary_key.push(name.clone());
                        } else {
                            self.edit.table.primary_key.retain(|k| k != &name);
                        }
                        changed = true;
                    }
                    ui.horizontal(|ui| {
                        if i > 0
                            && form::icon_button(ui, theme, ph::ARROW_UP, 12.0, false).clicked()
                        {
                            swap = Some((i, i - 1));
                        }
                        if i + 1 < n
                            && form::icon_button(ui, theme, ph::ARROW_DOWN, 12.0, false).clicked()
                        {
                            swap = Some((i, i + 1));
                        }
                        if form::icon_button(ui, theme, ph::X, 12.0, false)
                            .tip(s.t("db-designer-remove"))
                            .clicked()
                        {
                            remove = Some(i);
                        }
                    });
                    ui.end_row();
                }
            });
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
        ui.add_space(4.0);
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
        egui::Grid::new(("db-designer-indexes", self.uid))
            .num_columns(4)
            .spacing([8.0, 4.0])
            .striped(true)
            .show(ui, |ui| {
                for key in [
                    "db-designer-ix-name",
                    "db-designer-ix-columns",
                    "db-designer-ix-unique",
                ] {
                    ui.label(egui::RichText::new(s.t(key)).small().strong());
                }
                ui.label("");
                ui.end_row();
                for (i, ix) in self.edit.table.indexes.iter_mut().enumerate() {
                    if ix.primary {
                        ui.label(egui::RichText::new(&ix.name).color(theme.text_muted));
                        ui.label(
                            egui::RichText::new(ix.columns.join(", ")).color(theme.text_muted),
                        );
                        ui.label(s.t("db-designer-ix-primary"));
                        ui.label("");
                        ui.end_row();
                        continue;
                    }
                    changed |= ui
                        .add_sized([180.0, 22.0], egui::TextEdit::singleline(&mut ix.name))
                        .changed();
                    if form::text_field(ui, &mut self.index_cols[i], "a, b").changed() {
                        ix.columns = split_list(&self.index_cols[i]);
                        changed = true;
                    }
                    changed |= ui.checkbox(&mut ix.unique, "").changed();
                    if form::icon_button(ui, theme, ph::X, 12.0, false).clicked() {
                        remove = Some(i);
                    }
                    ui.end_row();
                }
            });
        if let Some(i) = remove {
            self.edit.table.indexes.remove(i);
            self.index_cols.remove(i);
            changed = true;
        }
        ui.add_space(4.0);
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
        egui::Grid::new(("db-designer-keys", self.uid))
            .num_columns(7)
            .spacing([8.0, 4.0])
            .striped(true)
            .show(ui, |ui| {
                for key in [
                    "db-designer-fk-name",
                    "db-designer-fk-columns",
                    "db-designer-fk-table",
                    "db-designer-fk-ref-columns",
                    "db-designer-fk-on-delete",
                    "db-designer-fk-on-update",
                ] {
                    ui.label(egui::RichText::new(s.t(key)).small().strong());
                }
                ui.label("");
                ui.end_row();
                for (i, fk) in self.edit.table.foreign_keys.iter_mut().enumerate() {
                    changed |= ui
                        .add_sized([160.0, 22.0], egui::TextEdit::singleline(&mut fk.name))
                        .changed();
                    if form::text_field(ui, &mut self.key_cols[i].0, "a, b").changed() {
                        fk.columns = split_list(&self.key_cols[i].0);
                        changed = true;
                    }
                    let mut target = if fk.ref_schema.is_empty() {
                        fk.ref_table.clone()
                    } else {
                        format!("{}.{}", fk.ref_schema, fk.ref_table)
                    };
                    if form::pick_field(ui, theme, &mut target, &tables, "").changed() {
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
                    let ref_cols: Vec<String> = catalog
                        .iter()
                        .find(|(sch, t, _)| {
                            t == &fk.ref_table
                                && (sch == &fk.ref_schema || fk.ref_schema.is_empty())
                        })
                        .map(|(_, _, cols)| cols.clone())
                        .unwrap_or_default();
                    if form::pick_field(ui, theme, &mut self.key_cols[i].1, &ref_cols, "id")
                        .changed()
                    {
                        fk.ref_columns = split_list(&self.key_cols[i].1);
                        changed = true;
                    }
                    let mut od = fk.on_delete.clone().unwrap_or_default();
                    if form::pick_field(ui, theme, &mut od, &actions, "").changed() {
                        fk.on_delete = (!od.trim().is_empty()).then_some(od);
                        changed = true;
                    }
                    let mut ou = fk.on_update.clone().unwrap_or_default();
                    if form::pick_field(ui, theme, &mut ou, &actions, "").changed() {
                        fk.on_update = (!ou.trim().is_empty()).then_some(ou);
                        changed = true;
                    }
                    if form::icon_button(ui, theme, ph::X, 12.0, false).clicked() {
                        remove = Some(i);
                    }
                    ui.end_row();
                }
            });
        if let Some(i) = remove {
            self.edit.table.foreign_keys.remove(i);
            self.key_cols.remove(i);
            changed = true;
        }
        ui.add_space(4.0);
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
