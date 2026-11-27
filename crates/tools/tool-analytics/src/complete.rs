//! Completion for the SQL page (docs/tools/analytics.md §4.6): the word
//! under the caret against a database's tables and columns and the SQL
//! keywords. Pure: the page asks with the text and the caret and draws the
//! answer. Smaller than the Database tool's engine on purpose: the server
//! gives table and column names, not a live catalog with types and
//! relations, so there is nothing to rank by.

use crate::provider::TableMeta;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Column,
    Table,
    Keyword,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub label: String,
    /// The column's type or the table's schema; empty for a keyword.
    pub detail: String,
    pub kind: Kind,
}

/// What to offer for the word ending at the caret, and the byte range the
/// pick replaces.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Completion {
    pub start: usize,
    pub end: usize,
    pub items: Vec<Item>,
}

const KEYWORDS: &[&str] = &[
    "SELECT",
    "FROM",
    "WHERE",
    "GROUP BY",
    "ORDER BY",
    "HAVING",
    "LIMIT",
    "OFFSET",
    "JOIN",
    "LEFT JOIN",
    "INNER JOIN",
    "RIGHT JOIN",
    "ON",
    "AS",
    "AND",
    "OR",
    "NOT",
    "IN",
    "IS NULL",
    "IS NOT NULL",
    "LIKE",
    "BETWEEN",
    "DISTINCT",
    "COUNT",
    "SUM",
    "AVG",
    "MIN",
    "MAX",
    "CASE",
    "WHEN",
    "THEN",
    "ELSE",
    "END",
    "UNION",
    "WITH",
    "ASC",
    "DESC",
    "CAST",
];

/// The most items one list shows.
const MAX_ITEMS: usize = 30;

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// The completion at byte offset `caret` of `text`. `force` offers a list
/// for an empty word too (Ctrl+Space); typing needs two characters, or a
/// dot after a table's name.
pub fn complete(text: &str, caret: usize, tables: &[TableMeta], force: bool) -> Completion {
    let caret = caret.min(text.len());
    let before = &text[..caret];
    let word_len: usize = before
        .chars()
        .rev()
        .take_while(|c| is_word(*c))
        .map(char::len_utf8)
        .sum();
    let start = caret - word_len;
    let prefix = &text[start..caret];
    let dotted = text[..start].ends_with('.');
    if !force && !dotted && prefix.chars().count() < 2 {
        return Completion::default();
    }
    let lower = prefix.to_lowercase();
    let wanted = |label: &str| {
        label.to_lowercase().starts_with(&lower) && !label.eq_ignore_ascii_case(prefix)
    };
    let mut items = Vec::new();
    if dotted {
        // `orders.`: that table's columns.
        let head = &text[..start - 1];
        let qualifier: String = head
            .chars()
            .rev()
            .take_while(|c| is_word(*c))
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        for t in tables
            .iter()
            .filter(|t| t.name.eq_ignore_ascii_case(&qualifier))
        {
            for f in t.fields.iter().filter(|f| wanted(&f.name)) {
                items.push(Item {
                    label: f.name.clone(),
                    detail: f.ty.clone(),
                    kind: Kind::Column,
                });
            }
        }
    } else {
        // Columns of the tables the statement already names, then every
        // table, then keywords.
        let named: Vec<&TableMeta> = tables.iter().filter(|t| mentions(text, &t.name)).collect();
        for t in &named {
            for f in t.fields.iter().filter(|f| wanted(&f.name)) {
                if !items.iter().any(|i: &Item| i.label == f.name) {
                    items.push(Item {
                        label: f.name.clone(),
                        detail: f.ty.clone(),
                        kind: Kind::Column,
                    });
                }
            }
        }
        for t in tables.iter().filter(|t| wanted(&t.name)) {
            items.push(Item {
                label: t.name.clone(),
                detail: t.schema.clone(),
                kind: Kind::Table,
            });
        }
        for k in KEYWORDS.iter().filter(|k| wanted(k)) {
            items.push(Item {
                label: (*k).to_string(),
                detail: String::new(),
                kind: Kind::Keyword,
            });
        }
    }
    items.truncate(MAX_ITEMS);
    Completion {
        start,
        end: caret,
        items,
    }
}

/// Whether `text` holds `name` as a whole word, ignoring case.
fn mentions(text: &str, name: &str) -> bool {
    let (t, n) = (text.to_lowercase(), name.to_lowercase());
    t.match_indices(&n).any(|(i, _)| {
        let before = t[..i].chars().next_back();
        let after = t[i + n.len()..].chars().next();
        !before.is_some_and(is_word) && !after.is_some_and(is_word)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::FieldMeta;

    fn table(name: &str, cols: &[(&str, &str)]) -> TableMeta {
        TableMeta {
            id: name.into(),
            schema: "public".into(),
            name: name.into(),
            fields: cols
                .iter()
                .map(|(n, t)| FieldMeta {
                    name: (*n).into(),
                    ty: (*t).into(),
                })
                .collect(),
        }
    }

    fn shop() -> Vec<TableMeta> {
        vec![
            table(
                "orders",
                &[("id", "integer"), ("total", "numeric"), ("status", "text")],
            ),
            table("customers", &[("id", "integer"), ("name", "text")]),
        ]
    }

    fn labels(c: &Completion) -> Vec<&str> {
        c.items.iter().map(|i| i.label.as_str()).collect()
    }

    #[test]
    fn a_table_name_completes_from_two_characters() {
        let c = complete("SELECT * FROM or", 16, &shop(), false);
        assert_eq!(labels(&c), ["orders", "ORDER BY"]);
        assert_eq!((c.start, c.end), (14, 16), "the word under the caret");
        assert!(
            complete("SELECT * FROM o", 15, &shop(), false)
                .items
                .is_empty()
        );
        assert!(
            !complete("SELECT * FROM o", 15, &shop(), true)
                .items
                .is_empty()
        );
    }

    #[test]
    fn after_a_dot_the_tables_columns_are_offered() {
        let c = complete("SELECT orders. FROM orders", 14, &shop(), false);
        assert_eq!(labels(&c), ["id", "total", "status"]);
        assert_eq!(c.items[1].detail, "numeric");
        let c = complete("SELECT orders.st FROM orders", 16, &shop(), false);
        assert_eq!(labels(&c), ["status"]);
        assert!(
            complete("SELECT nothing. FROM orders", 15, &shop(), false)
                .items
                .is_empty()
        );
    }

    #[test]
    fn columns_come_from_the_tables_the_statement_names() {
        let c = complete("SELECT to FROM orders", 9, &shop(), false);
        assert_eq!(labels(&c)[0], "total", "a column of the named table first");
        let c = complete("SELECT na FROM orders", 9, &shop(), false);
        assert!(
            !labels(&c).contains(&"name"),
            "customers is not named, so its columns are not offered"
        );
    }

    #[test]
    fn a_word_already_typed_in_full_is_not_offered_back() {
        let c = complete("SELECT * FROM orders", 20, &shop(), false);
        assert!(!labels(&c).contains(&"orders"));
    }

    #[test]
    fn the_list_is_capped_and_a_name_matches_as_a_whole_word() {
        let many: Vec<(String, &str)> = (0..80).map(|i| (format!("c{i}"), "int")).collect();
        let cols: Vec<(&str, &str)> = many.iter().map(|(n, t)| (n.as_str(), *t)).collect();
        let c = complete("SELECT c FROM big", 8, &[table("big", &cols)], true);
        assert_eq!(c.items.len(), MAX_ITEMS);
        assert!(mentions("select * from orders o", "orders"));
        assert!(!mentions("select * from preorders", "orders"));
    }
}
