//! Snippets (docs/tools/devtools.md §2 Workspace helpers): short pieces
//! of code, SQL or shell kept per workspace with a language and tags,
//! searched by any of them, copied or sent to the editor.
//!
//! Stored as one TOML file under the workspace so they travel with it
//! and a diff of the workspace shows what changed.

use hs_core::Services;
use hs_core::storage;
use hs_ui::form;
use hs_ui::icons::ph;
use hs_ui::theme::Theme;
use serde::{Deserialize, Serialize};

pub const SCHEMA: u32 = 1;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Snippet {
    pub name: String,
    /// `sql`, `bash`, `rust`, … - free text, shown as a badge.
    pub language: String,
    pub tags: Vec<String>,
    pub body: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct File {
    schema: u32,
    snippet: Vec<Snippet>,
}

pub fn file_for(workspace_dir: &std::path::Path) -> std::path::PathBuf {
    workspace_dir.join("snippets.toml")
}

pub fn load(workspace_dir: &std::path::Path) -> Vec<Snippet> {
    match storage::read_toml::<File>(&file_for(workspace_dir)) {
        Ok(f) if f.schema <= SCHEMA => f.snippet,
        _ => Vec::new(),
    }
}

pub fn save(workspace_dir: &std::path::Path, items: &[Snippet]) -> Result<(), String> {
    storage::write_toml(
        &file_for(workspace_dir),
        &File {
            schema: SCHEMA,
            snippet: items.to_vec(),
        },
    )
    .map_err(|e| e.to_string())
}

/// Snippets whose name, language, tags or body contain `needle`.
pub fn search<'a>(items: &'a [Snippet], needle: &str) -> Vec<&'a Snippet> {
    let n = needle.trim().to_lowercase();
    if n.is_empty() {
        return items.iter().collect();
    }
    items
        .iter()
        .filter(|s| {
            s.name.to_lowercase().contains(&n)
                || s.language.to_lowercase().contains(&n)
                || s.body.to_lowercase().contains(&n)
                || s.tags.iter().any(|t| t.to_lowercase().contains(&n))
        })
        .collect()
}

/// `{{name}}` placeholders in a snippet, in order, without repeats.
pub fn placeholders(body: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut rest = body;
    while let Some(start) = rest.find("{{") {
        let after = &rest[start + 2..];
        let Some(end) = after.find("}}") else { break };
        let name = after[..end].trim().to_string();
        if !name.is_empty() && !name.contains("{{") && !out.contains(&name) {
            out.push(name);
        }
        rest = &after[end + 2..];
    }
    out
}

/// A unique name in `items`: "Name", then "Name 001", "Name 002", …
/// (the same mechanism the rest of the app uses for a clash).
pub fn unique_name(items: &[Snippet], wanted: &str) -> String {
    let wanted = wanted.trim();
    if !items.iter().any(|s| s.name == wanted) {
        return wanted.to_string();
    }
    for n in 1..1000 {
        let candidate = format!("{wanted} {n:03}");
        if !items.iter().any(|s| s.name == candidate) {
            return candidate;
        }
    }
    wanted.to_string()
}

#[derive(Default)]
pub struct State {
    pub items: Vec<Snippet>,
    pub loaded: bool,
    pub search: String,
    /// The snippet being edited, by its index, and the draft.
    pub editing: Option<(Option<usize>, Snippet)>,
    pub error: Option<String>,
    /// Values typed for a snippet's placeholders, keyed by name.
    pub fills: Vec<(String, String)>,
    pub selected: Option<usize>,
    /// A Delete waiting for its confirmation, by index (CR-0067).
    pub confirm_remove: Option<usize>,
}

/// The snippet with its `{{placeholders}}` filled in.
pub fn render(body: &str, fills: &[(String, String)]) -> String {
    let mut out = body.to_string();
    for (k, v) in fills {
        out = out.replace(&format!("{{{{{k}}}}}"), v);
    }
    out
}

pub fn ui(state: &mut State, ui: &mut egui::Ui, s: &Services, theme: &Theme) {
    use hs_ui::table::{EditColumn, edit_table};
    use hs_ui::tip::TipExt;
    let dir = s.workspace.read().root().to_path_buf();
    if !state.loaded {
        state.items = load(&dir);
        state.loaded = true;
    }
    crate::util::page_title(ui, s, theme, "dt-snippets", Some("dt-snippets-sub"));
    form::section(ui, theme, &s.t("dt-snippets"), |ui| {
        form::hint(ui, theme, &s.t("dt-snippets-hint"));
        ui.add_space(form::ROW_GAP);
        crate::util::button_row(ui, |ui| {
            ui.add(
                form::line_edit(&mut state.search)
                    .hint_text(s.t("dt-snippets-search"))
                    .desired_width(form::FIELD_MIN_WIDTH),
            );
            if form::button(ui, theme, ph::PLUS, &s.t("dt-snippets-new")).clicked() {
                state.editing = Some((None, Snippet::default()));
            }
        });
        if let Some(e) = &state.error {
            form::note(ui, theme, ph::WARNING, e);
        }
        ui.add_space(form::ROW_GAP);
        let found: Vec<(usize, Snippet)> = {
            let hits = search(&state.items, &state.search);
            hits.into_iter()
                .filter_map(|h| {
                    state
                        .items
                        .iter()
                        .position(|x| x == h)
                        .map(|i| (i, h.clone()))
                })
                .collect()
        };
        let mut edit: Option<usize> = None;
        let mut remove: Option<usize> = None;
        let mut pick: Option<usize> = None;
        let (name_col, lang_col, tags_col, actions_col, empty) = (
            s.t("dt-snippets-name"),
            s.t("dt-snippets-language"),
            s.t("dt-snippets-tags"),
            s.t("dt-col-actions"),
            s.t("dt-snippets-none"),
        );
        let delete_label = s.t("delete");
        let edit_label = s.t("dt-snippets-edit");
        let selected = state.selected;
        egui::ScrollArea::vertical()
            .id_salt("dt-snippets-list")
            .max_height(260.0)
            .show(ui, |ui| {
                edit_table(
                    ui,
                    theme,
                    "dt-snippets-table",
                    &[
                        EditColumn::new(&name_col, 220.0),
                        EditColumn::new(&lang_col, 100.0),
                        EditColumn::new(&tags_col, 200.0),
                        EditColumn::new(&actions_col, 0.0),
                    ],
                    found.len(),
                    Some(&empty),
                    |ui, r, c| {
                        let (i, sn) = &found[r];
                        match c {
                            0 => {
                                let row = form::selectable_label(
                                    ui,
                                    selected == Some(*i),
                                    sn.name.as_str(),
                                );
                                row.context_menu(|ui| {
                                    if hs_ui::toolbar::btn(
                                        ui,
                                        theme,
                                        hs_ui::icons::TRASH,
                                        &delete_label,
                                    )
                                    .clicked()
                                    {
                                        remove = Some(*i);
                                        ui.close();
                                    }
                                });
                                if row.clicked() {
                                    pick = Some(*i);
                                }
                            }
                            1 => {
                                ui.add(
                                    egui::Label::new(
                                        egui::RichText::new(&sn.language).color(theme.accent),
                                    )
                                    .truncate(),
                                );
                            }
                            2 => {
                                let tags = sn
                                    .tags
                                    .iter()
                                    .map(|t| format!("#{t}"))
                                    .collect::<Vec<_>>()
                                    .join(" ");
                                ui.add(
                                    egui::Label::new(
                                        egui::RichText::new(tags).color(theme.text_muted),
                                    )
                                    .truncate(),
                                );
                            }
                            _ => {
                                if form::icon_button(ui, theme, ph::PENCIL_SIMPLE, 14.0, false)
                                    .tip(edit_label.as_str())
                                    .clicked()
                                {
                                    edit = Some(*i);
                                }
                                if form::icon_button(ui, theme, hs_ui::icons::TRASH, 14.0, false)
                                    .tip(delete_label.as_str())
                                    .clicked()
                                {
                                    remove = Some(*i);
                                }
                            }
                        }
                    },
                );
            });
        if let Some(i) = pick {
            state.selected = (state.selected != Some(i)).then_some(i);
            state.fills = state
                .items
                .get(i)
                .map(|sn| {
                    placeholders(&sn.body)
                        .into_iter()
                        .map(|p| (p, String::new()))
                        .collect()
                })
                .unwrap_or_default();
        }
        if let Some(i) = edit {
            state.editing = Some((Some(i), state.items[i].clone()));
        }
        if remove.is_some() {
            state.confirm_remove = remove;
        }
        // Delete, from the trash button or the right-click menu, behind a
        // confirmation (CR-0067).
        if let Some(i) = state.confirm_remove {
            let name = state
                .items
                .get(i)
                .map(|sn| sn.name.clone())
                .unwrap_or_default();
            match hs_ui::dialog::confirm(
                ui.ctx(),
                theme,
                "dt-snippet-delete",
                &s.t("dt-snippets-delete"),
                &s.i18n
                    .t_args("dt-snippets-delete-body", &[("name", name.as_str())]),
                &format!("{} {}", hs_ui::icons::TRASH, s.t("delete")),
                &format!("{} {}", ph::X, s.t("cancel")),
                true,
            ) {
                hs_ui::dialog::DialogResult::Ok => {
                    state.confirm_remove = None;
                    if i < state.items.len() {
                        state.items.remove(i);
                        state.selected = None;
                        if let Err(e) = save(&dir, &state.items) {
                            state.error = Some(e);
                        }
                    }
                }
                hs_ui::dialog::DialogResult::Cancel => state.confirm_remove = None,
                hs_ui::dialog::DialogResult::Open => {}
            }
        }
        // The open snippet: its body, its placeholders and Copy.
        if let Some(i) = state.selected
            && let Some(sn) = state.items.get(i).cloned()
        {
            ui.add_space(form::PANE_GAP);
            ui.separator();
            ui.add_space(form::PANE_GAP);
            ui.label(egui::RichText::new(&sn.name).strong());
            if !state.fills.is_empty() {
                ui.add_space(form::ROW_GAP);
                form::field_grid(ui, "dt-snippet-fills", |ui| {
                    for (name, value) in state.fills.iter_mut() {
                        form::label(ui, &format!("{{{{{name}}}}}"));
                        form::text_field(ui, value, name.as_str());
                        ui.end_row();
                    }
                });
            }
            ui.add_space(form::ROW_GAP);
            let filled = render(&sn.body, &state.fills);
            let mut shown = filled.clone();
            ui.add(
                egui::TextEdit::multiline(&mut shown)
                    .code_editor()
                    .desired_rows(8)
                    .desired_width(f32::INFINITY)
                    .interactive(false),
            );
            ui.add_space(form::ROW_GAP);
            if form::primary_button(
                ui,
                theme,
                &format!("{} {}", hs_ui::icons::COPY, s.t("dt-copy")),
            )
            .clicked()
            {
                ui.ctx().copy_text(filled.clone());
            }
        }
    });
    // The editor.
    if let Some((index, mut draft)) = state.editing.take() {
        let mut close = false;
        let mut keep = true;
        egui::Modal::new(egui::Id::new("dt-snippet-edit")).show(ui.ctx(), |ui| {
            ui.set_width(520.0);
            ui.heading(s.t(if index.is_some() {
                "dt-snippets-edit"
            } else {
                "dt-snippets-new"
            }));
            ui.add_space(6.0);
            form::field_grid(ui, "dt-snippet-grid", |ui| {
                form::label(ui, &s.t("dt-snippets-name"));
                form::text_field(ui, &mut draft.name, "List failed orders");
                ui.end_row();
                form::label(ui, &s.t("dt-snippets-language"));
                form::field_row(ui, |ui| {
                    hs_ui::language::picker(
                        ui,
                        "dt-snippet-language",
                        &s.t("language-none"),
                        &mut draft.language,
                    );
                });
                ui.end_row();
                form::label(ui, &s.t("dt-snippets-tags"));
                let mut tags = draft.tags.join(", ");
                if form::text_field(ui, &mut tags, "orders, support").changed() {
                    draft.tags = tags
                        .split(',')
                        .map(|t| t.trim().to_string())
                        .filter(|t| !t.is_empty())
                        .collect();
                }
                ui.end_row();
            });
            ui.add_space(form::ROW_GAP);
            ui.label(s.t("dt-snippets-body"));
            ui.add(
                egui::TextEdit::multiline(&mut draft.body)
                    .code_editor()
                    .desired_rows(10)
                    .desired_width(f32::INFINITY),
            );
            form::hint(ui, theme, &s.t("dt-snippets-placeholder-hint"));
            ui.add_space(10.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = form::BUTTON_GAP;
                let ready = !draft.name.trim().is_empty() && !draft.body.trim().is_empty();
                if ui
                    .add_enabled_ui(ready, |ui| {
                        form::primary_button(
                            ui,
                            theme,
                            &format!("{} {}", ph::FLOPPY_DISK, s.t("save")),
                        )
                    })
                    .inner
                    .clicked()
                {
                    match index {
                        Some(i) => state.items[i] = draft.clone(),
                        None => {
                            // A clash gets the usual " 001" suffix.
                            draft.name = unique_name(&state.items, &draft.name);
                            state.items.push(draft.clone());
                        }
                    }
                    if let Err(e) = save(&dir, &state.items) {
                        state.error = Some(e);
                    }
                    close = true;
                    keep = false;
                }
                if form::button(ui, theme, ph::X, &s.t("cancel")).clicked() {
                    close = true;
                    keep = false;
                }
            });
        });
        if keep && !close {
            state.editing = Some((index, draft));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snip(name: &str, lang: &str, tags: &[&str], body: &str) -> Snippet {
        Snippet {
            name: name.into(),
            language: lang.into(),
            tags: tags.iter().map(|t| t.to_string()).collect(),
            body: body.into(),
        }
    }

    #[test]
    fn search_looks_at_every_field() {
        let items = [
            snip("Failed orders", "sql", &["orders"], "SELECT * FROM orders"),
            snip("Restart", "bash", &["ops"], "systemctl restart nginx"),
        ];
        assert_eq!(search(&items, "").len(), 2);
        assert_eq!(search(&items, "ORDERS").len(), 1);
        assert_eq!(search(&items, "bash")[0].name, "Restart");
        assert_eq!(search(&items, "nginx")[0].name, "Restart");
        assert_eq!(search(&items, "ops")[0].name, "Restart");
        assert!(search(&items, "nothing").is_empty());
    }

    #[test]
    fn placeholders_are_found_once_each_in_order() {
        let body = "SELECT * FROM {{table}} WHERE id = {{id}} AND x = {{table}}";
        assert_eq!(placeholders(body), vec!["table", "id"]);
        assert!(placeholders("no placeholders").is_empty());
        assert!(placeholders("{{unclosed").is_empty());
        assert_eq!(
            render(
                body,
                &[("table".into(), "orders".into()), ("id".into(), "7".into())]
            ),
            "SELECT * FROM orders WHERE id = 7 AND x = orders"
        );
    }

    #[test]
    fn a_clashing_name_gets_the_usual_suffix() {
        let mut items = vec![snip("Query", "sql", &[], "x")];
        assert_eq!(unique_name(&items, "Other"), "Other");
        assert_eq!(unique_name(&items, "Query"), "Query 001");
        items.push(snip("Query 001", "sql", &[], "y"));
        assert_eq!(unique_name(&items, "Query"), "Query 002");
        assert_eq!(
            unique_name(&items, "  Query  "),
            "Query 002",
            "trimmed first"
        );
    }

    #[test]
    fn snippets_round_trip_through_the_workspace_file() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load(dir.path()).is_empty());
        let items = vec![snip("A", "sql", &["t"], "SELECT 1")];
        save(dir.path(), &items).unwrap();
        assert_eq!(load(dir.path()), items);
        assert!(file_for(dir.path()).ends_with("snippets.toml"));
    }
}
