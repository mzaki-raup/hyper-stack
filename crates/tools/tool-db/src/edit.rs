//! Inline editing of a result (docs/tools/database.md §8): a result that
//! came from one table with a primary key collects cell edits and row
//! deletions, shows them as the `UPDATE` / `DELETE` statements they will
//! become, and applies them in one transaction through the same guards as
//! anything typed in the editor.

use std::collections::BTreeMap;

use crate::driver::{Column, Engine, Row, Value};
use crate::export::sql_literal;
use crate::sql::quote_ident;

/// One change to a row of the result: a cell's new value, or the row gone.
#[derive(Debug, Clone, PartialEq)]
pub enum Change {
    Set { col: usize, value: Value },
    Delete,
}

/// The table a result can write back to, once its key is known.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Target {
    /// Quoted, qualified table name as the statements will spell it.
    pub table: String,
    /// Indices of the primary-key columns in the result.
    pub key_cols: Vec<usize>,
}

/// Pending changes of one result, by row.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Edits {
    pub target: Target,
    /// Row index → the changes to it. A deleted row keeps only `Delete`.
    pub rows: BTreeMap<usize, Vec<Change>>,
}

impl Edits {
    pub fn new(target: Target) -> Edits {
        Edits {
            target,
            rows: BTreeMap::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// How many rows carry a change.
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// Set a cell; setting it back to what the data holds drops the edit.
    pub fn set(&mut self, row: usize, col: usize, value: Value, original: &Value) {
        let changes = self.rows.entry(row).or_default();
        if changes.iter().any(|c| matches!(c, Change::Delete)) {
            return;
        }
        changes.retain(|c| !matches!(c, Change::Set { col: c2, .. } if *c2 == col));
        if value != *original {
            changes.push(Change::Set { col, value });
        }
        if changes.is_empty() {
            self.rows.remove(&row);
        }
    }

    pub fn delete(&mut self, row: usize) {
        self.rows.insert(row, vec![Change::Delete]);
    }

    pub fn is_deleted(&self, row: usize) -> bool {
        self.rows
            .get(&row)
            .is_some_and(|c| c.iter().any(|x| matches!(x, Change::Delete)))
    }

    /// The value a cell shows: its pending edit or the data's own.
    pub fn value<'a>(&'a self, row: usize, col: usize, data: &'a Value) -> &'a Value {
        self.rows
            .get(&row)
            .and_then(|cs| {
                cs.iter().find_map(|c| match c {
                    Change::Set { col: c2, value } if *c2 == col => Some(value),
                    _ => None,
                })
            })
            .unwrap_or(data)
    }

    pub fn discard_row(&mut self, row: usize) {
        self.rows.remove(&row);
    }

    /// The statements that apply every change, one per row, in row order.
    /// The WHERE names every key column with the row's *original* values,
    /// so a key that was itself edited still finds its row.
    pub fn statements(&self, columns: &[Column], rows: &[Row], engine: Engine) -> Vec<String> {
        let mut out = Vec::with_capacity(self.rows.len());
        for (&row, changes) in &self.rows {
            let Some(data) = rows.get(row) else { continue };
            let where_ = self
                .target
                .key_cols
                .iter()
                .map(|&k| {
                    let name = quote_ident(&columns[k].name, engine);
                    match data.get(k).unwrap_or(&Value::Null) {
                        Value::Null => format!("{name} IS NULL"),
                        v => format!("{name} = {}", sql_literal(v, engine)),
                    }
                })
                .collect::<Vec<_>>()
                .join(" AND ");
            if changes.iter().any(|c| matches!(c, Change::Delete)) {
                out.push(format!("DELETE FROM {} WHERE {where_};", self.target.table));
                continue;
            }
            let sets: Vec<String> = changes
                .iter()
                .filter_map(|c| match c {
                    Change::Set { col, value } => Some(format!(
                        "{} = {}",
                        quote_ident(&columns[*col].name, engine),
                        sql_literal(value, engine)
                    )),
                    Change::Delete => None,
                })
                .collect();
            if !sets.is_empty() {
                out.push(format!(
                    "UPDATE {} SET {} WHERE {where_};",
                    self.target.table,
                    sets.join(", ")
                ));
            }
        }
        out
    }
}

/// Turn what was typed into a value shaped like the cell it replaces: a
/// number stays a number, text stays text. `null` makes the cell NULL.
pub fn parse_input(text: &str, like: &Value, null: bool) -> Value {
    if null {
        return Value::Null;
    }
    match like {
        Value::Int(_) => text
            .trim()
            .parse::<i64>()
            .map(Value::Int)
            .unwrap_or_else(|_| Value::Number(text.trim().to_string())),
        Value::Float(_) => text
            .trim()
            .parse::<f64>()
            .map(Value::Float)
            .unwrap_or_else(|_| Value::Text(text.to_string())),
        Value::Number(_) => {
            if text.trim().parse::<f64>().is_ok() {
                Value::Number(text.trim().to_string())
            } else {
                Value::Text(text.to_string())
            }
        }
        _ => Value::Text(text.to_string()),
    }
}

/// Which result columns are the table's primary key, given the table's
/// columns from introspection (`badges` carry "PK"). `None` when the table
/// has no key or a key column is missing from the result.
pub fn key_columns(
    result: &[Column],
    table_columns: &[crate::driver::SchemaObject],
) -> Option<Vec<usize>> {
    let keys: Vec<&str> = table_columns
        .iter()
        .filter(|c| c.badges.iter().any(|b| b == "PK"))
        .map(|c| c.name.as_str())
        .collect();
    if keys.is_empty() {
        return None;
    }
    keys.iter()
        .map(|k| result.iter().position(|c| c.name.eq_ignore_ascii_case(k)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::driver::{ObjectKind, SchemaObject};

    fn col(n: &str) -> Column {
        Column {
            name: n.into(),
            type_name: String::new(),
        }
    }

    #[test]
    fn edits_become_update_and_delete_statements() {
        let cols = [col("id"), col("name"), col("price")];
        let rows: Vec<Row> = vec![
            vec![
                Value::Int(1),
                Value::Text("a".into()),
                Value::Number("10.50".into()),
            ],
            vec![Value::Int(2), Value::Text("b".into()), Value::Null],
            vec![Value::Int(3), Value::Text("c".into()), Value::Null],
        ];
        let mut e = Edits::new(Target {
            table: "public.items".into(),
            key_cols: vec![0],
        });
        e.set(0, 1, Value::Text("A".into()), &rows[0][1]);
        e.set(0, 2, Value::Null, &rows[0][2]);
        e.set(1, 1, Value::Text("b".into()), &rows[1][1]); // unchanged: no edit
        e.delete(2);
        assert_eq!(e.len(), 2);
        assert!(e.is_deleted(2));
        assert_eq!(e.value(0, 1, &rows[0][1]), &Value::Text("A".into()));
        assert_eq!(e.value(1, 1, &rows[1][1]), &Value::Text("b".into()));
        assert_eq!(
            e.statements(&cols, &rows, Engine::Postgres),
            vec![
                "UPDATE public.items SET name = 'A', price = NULL WHERE id = 1;".to_string(),
                "DELETE FROM public.items WHERE id = 3;".to_string(),
            ]
        );
        // Editing a key column still finds the row by its original key.
        e.set(0, 0, Value::Int(9), &rows[0][0]);
        assert!(e.statements(&cols, &rows, Engine::Postgres)[0].contains("id = 9 WHERE id = 1"));
        e.discard_row(0);
        e.discard_row(2);
        assert!(e.is_empty());
    }

    #[test]
    fn typed_text_takes_the_cells_shape() {
        assert_eq!(parse_input("42", &Value::Int(1), false), Value::Int(42));
        assert_eq!(
            parse_input("x", &Value::Int(1), false),
            Value::Number("x".into())
        );
        assert_eq!(
            parse_input("1.5", &Value::Number("2".into()), false),
            Value::Number("1.5".into())
        );
        assert_eq!(
            parse_input("hi", &Value::Text(String::new()), false),
            Value::Text("hi".into())
        );
        assert_eq!(parse_input("hi", &Value::Null, true), Value::Null);
    }

    #[test]
    fn key_columns_come_from_the_pk_badges() {
        let so = |n: &str, pk: bool| SchemaObject {
            kind: ObjectKind::Column,
            name: n.into(),
            detail: String::new(),
            badges: if pk { vec!["PK".into()] } else { vec![] },
        };
        let table = [so("id", true), so("name", false)];
        assert_eq!(
            key_columns(&[col("name"), col("ID")], &table),
            Some(vec![1])
        );
        assert_eq!(key_columns(&[col("name")], &table), None);
        assert_eq!(key_columns(&[col("id")], &[so("id", false)]), None);
    }
}
