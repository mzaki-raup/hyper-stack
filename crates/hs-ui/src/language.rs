//! The languages a piece of text can be written in, and the dropdown
//! that picks one.
//!
//! Shared so every activity offers the same list: snippets today, and
//! anything else that stores a language id beside a body (a scratch
//! note, an editor tab, a request body) later. The stored value is
//! always the id, never the label.

use std::sync::LazyLock;

/// Languages beyond the ones a *project* can be written in
/// (`hs_core::projects::LANGUAGES`): the shells, the query and config
/// formats, and the catch-alls. A snippet is as often one of these.
const EXTRA: &[(&str, &str)] = &[
    ("sql", "SQL"),
    ("bash", "Bash"),
    ("powershell", "PowerShell"),
    ("batch", "Batch"),
    ("dockerfile", "Dockerfile"),
    ("make", "Makefile"),
    ("json", "JSON"),
    ("yaml", "YAML"),
    ("toml", "TOML"),
    ("ini", "INI"),
    ("xml", "XML"),
    ("csv", "CSV"),
    ("graphql", "GraphQL"),
    ("regex", "Regular Expression"),
    ("diff", "Diff"),
    ("text", "Plain Text"),
];

/// Every language on offer, by label. Built once: the two lists are
/// constants and sorting them on every frame would be wasted work.
pub static ALL: LazyLock<Vec<(&'static str, &'static str)>> = LazyLock::new(|| {
    let mut all: Vec<(&'static str, &'static str)> = hs_core::projects::LANGUAGES
        .iter()
        .copied()
        .chain(EXTRA.iter().copied())
        .collect();
    all.sort_by_key(|(_, label)| label.to_ascii_lowercase());
    all
});

/// The label for a language id. An id we don't know comes back as
/// itself, so a value typed before this list existed stays visible.
pub fn name(id: &str) -> &str {
    ALL.iter()
        .find(|(k, _)| *k == id)
        .map(|(_, label)| *label)
        .unwrap_or(id)
}

/// A dropdown over [`ALL`]. `value` holds the id, empty for none.
/// Returns true when the choice changed.
///
/// Size it like the text field beside it with `form::field_row`.
pub fn picker(ui: &mut egui::Ui, id_salt: &str, none_label: &str, value: &mut String) -> bool {
    let mut changed = false;
    let selected = if value.is_empty() {
        none_label.to_string()
    } else {
        name(value).to_string()
    };
    egui::ComboBox::from_id_salt(id_salt)
        .selected_text(selected)
        .width(220.0)
        .show_ui(ui, |ui| {
            if ui.selectable_label(value.is_empty(), none_label).clicked() {
                value.clear();
                changed = true;
            }
            for (id, label) in ALL.iter() {
                if ui.selectable_label(value == id, *label).clicked() {
                    *value = (*id).to_string();
                    changed = true;
                }
            }
        });
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_list_carries_projects_and_the_extras() {
        assert!(ALL.iter().any(|(id, _)| *id == "rust"));
        assert!(ALL.iter().any(|(id, _)| *id == "sql"));
        assert!(ALL.len() > hs_core::projects::LANGUAGES.len());
    }

    #[test]
    fn it_is_sorted_by_label_and_has_no_duplicate_ids() {
        let labels: Vec<String> = ALL.iter().map(|(_, l)| l.to_ascii_lowercase()).collect();
        let mut sorted = labels.clone();
        sorted.sort();
        assert_eq!(labels, sorted);
        let mut ids: Vec<&str> = ALL.iter().map(|(id, _)| *id).collect();
        ids.sort_unstable();
        let before = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), before);
    }

    #[test]
    fn an_unknown_id_is_its_own_label() {
        assert_eq!(name("rust"), "Rust");
        assert_eq!(name("sql"), "SQL");
        assert_eq!(name("brainfuck"), "brainfuck");
    }
}
