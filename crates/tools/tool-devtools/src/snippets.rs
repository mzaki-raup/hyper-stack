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
    let dir = s.workspace.read().root().to_path_buf();
    if !state.loaded {
        state.items = load(&dir);
        state.loaded = true;
    }
    form::section(ui, theme, &s.t("dt-snippets"), |ui| {
        form::hint(ui, theme, &s.t("dt-snippets-hint"));
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut state.search)
                    .hint_text(s.t("dt-snippets-search"))
                    .desired_width(260.0),
            );
            if ui
                .button(format!("{} {}", ph::PLUS, s.t("dt-snippets-new")))
                .clicked()
            {
                state.editing = Some((None, Snippet::default()));
            }
        });
        if let Some(e) = &state.error {
            form::note(ui, theme, ph::WARNING, e);
        }
        ui.add_space(6.0);
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
        if found.is_empty() {
            form::hint(ui, theme, &s.t("dt-snippets-none"));
        }
        // Fixed slots for the name and the language, so the language,
        // the tags and the row actions line up down the list instead of
        // starting wherever the name happens to end.
        let total = ui.available_width().clamp(420.0, 900.0);
        let name_w = (total * 0.34).clamp(160.0, 280.0);
        let lang_w = 70.0;
        let mut edit: Option<usize> = None;
        let mut remove: Option<usize> = None;
        let mut pick: Option<usize> = None;
        egui::ScrollArea::vertical()
            .id_salt("dt-snippets-list")
            .max_height(220.0)
            .show(ui, |ui| {
                for (i, sn) in &found {
                    ui.horizontal(|ui| {
                        let open = state.selected == Some(*i);
                        let slot = egui::Layout::left_to_right(egui::Align::Center);
                        let clicked = ui
                            .allocate_ui_with_layout(egui::vec2(name_w, 22.0), slot, |ui| {
                                // The slot keeps its width when the name is
                                // short: allocate_ui otherwise takes only
                                // what the child used.
                                ui.set_min_width(name_w);
                                ui.add(egui::Button::selectable(open, &sn.name))
                            })
                            .inner
                            .clicked();
                        if clicked {
                            pick = Some(*i);
                        }
                        ui.allocate_ui_with_layout(egui::vec2(lang_w, 22.0), slot, |ui| {
                            ui.set_min_width(lang_w);
                            if !sn.language.is_empty() {
                                ui.add(
                                    egui::Label::new(
                                        egui::RichText::new(&sn.language)
                                            .small()
                                            .color(theme.accent),
                                    )
                                    .truncate(),
                                );
                            }
                        });
                        for t in &sn.tags {
                            ui.label(
                                egui::RichText::new(format!("#{t}"))
                                    .small()
                                    .color(theme.text_muted),
                            );
                        }
                        hs_ui::toolbar::hard_right(ui, |ui| {
                            if form::icon_button(ui, theme, hs_ui::icons::TRASH, 14.0, false)
                                .clicked()
                            {
                                remove = Some(*i);
                            }
                            if form::icon_button(ui, theme, ph::PENCIL_SIMPLE, 14.0, false)
                                .clicked()
                            {
                                edit = Some(*i);
                            }
                        });
                    });
                }
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
        if let Some(i) = remove {
            state.items.remove(i);
            state.selected = None;
            if let Err(e) = save(&dir, &state.items) {
                state.error = Some(e);
            }
        }
        // The open snippet: its body, its placeholders and Copy.
        if let Some(i) = state.selected
            && let Some(sn) = state.items.get(i).cloned()
        {
            ui.add_space(8.0);
            ui.separator();
            ui.label(egui::RichText::new(&sn.name).strong());
            for (name, value) in state.fills.iter_mut() {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(format!("{{{{{name}}}}}"))
                            .small()
                            .monospace()
                            .color(theme.text_muted),
                    );
                    ui.add(
                        egui::TextEdit::singleline(value)
                            .hint_text(name.as_str())
                            .desired_width(200.0),
                    );
                });
            }
            let filled = render(&sn.body, &state.fills);
            let mut shown = filled.clone();
            ui.add(
                egui::TextEdit::multiline(&mut shown)
                    .code_editor()
                    .desired_rows(8)
                    .desired_width(f32::INFINITY)
                    .interactive(false),
            );
            ui.horizontal(|ui| {
                if form::primary_button(ui, theme, &s.t("dt-copy")).clicked() {
                    ui.ctx().copy_text(filled.clone());
                }
            });
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
                let ready = !draft.name.trim().is_empty() && !draft.body.trim().is_empty();
                if ui
                    .add_enabled(ready, egui::Button::new(s.t("save")))
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
                if ui.button(s.t("cancel")).clicked() {
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
