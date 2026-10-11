//! SQL completion from the schema catalog (docs/tools/database.md §6):
//! tables after `FROM`/`JOIN`/`INTO`/`UPDATE`, columns after `alias.` (with
//! the aliases of the current statement tracked), tables after `schema.`,
//! otherwise the columns of the statement's tables, tables and keywords.

use std::collections::HashMap;

use crate::driver::{Catalog, Engine};
use crate::sql::{Tok, tokenize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    Column,
    Table,
    Keyword,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub label: String,
    /// Text inserted (quoted when needed).
    pub insert: String,
    pub kind: Kind,
    /// "orders" for a column, schema for a table.
    pub detail: String,
}

/// What completion found for the cursor position: the byte range of the
/// word being typed (replaced on accept) and the candidates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completion {
    pub start: usize,
    pub end: usize,
    pub items: Vec<Candidate>,
}

const KEYWORDS: &[&str] = &[
    "SELECT",
    "FROM",
    "WHERE",
    "AND",
    "OR",
    "NOT",
    "NULL",
    "IS",
    "IN",
    "EXISTS",
    "BETWEEN",
    "LIKE",
    "ILIKE",
    "AS",
    "ON",
    "JOIN",
    "INNER",
    "LEFT",
    "RIGHT",
    "FULL",
    "OUTER",
    "CROSS",
    "GROUP BY",
    "ORDER BY",
    "HAVING",
    "LIMIT",
    "OFFSET",
    "DISTINCT",
    "UNION",
    "UNION ALL",
    "INSERT INTO",
    "VALUES",
    "UPDATE",
    "SET",
    "DELETE FROM",
    "RETURNING",
    "WITH",
    "CASE",
    "WHEN",
    "THEN",
    "ELSE",
    "END",
    "ASC",
    "DESC",
    "COUNT",
    "SUM",
    "AVG",
    "MIN",
    "MAX",
    "COALESCE",
    "CAST",
    "CREATE TABLE",
    "ALTER TABLE",
    "DROP TABLE",
    "CREATE INDEX",
    "PRIMARY KEY",
    "FOREIGN KEY",
    "REFERENCES",
    "DEFAULT",
    "BEGIN",
    "COMMIT",
    "ROLLBACK",
    "EXPLAIN",
    "TRUE",
    "FALSE",
];

const TABLE_CONTEXT: &[&str] = &["FROM", "JOIN", "INTO", "UPDATE", "TABLE", "DESCRIBE"];

fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '$'
}

/// The default schema whose tables are offered unqualified.
fn default_schema(engine: Engine, catalog: &Catalog) -> String {
    match engine {
        Engine::Postgres => "public".into(),
        Engine::Sqlite => "main".into(),
        // MySQL: the schema the session uses is the first one listed.
        Engine::Mysql => catalog
            .tables
            .first()
            .map(|t| t.0.clone())
            .unwrap_or_default(),
    }
}

/// Table name references in `stmt` → alias (or the name itself) to
/// (schema, table).
fn aliases(stmt: &str, engine: Engine, catalog: &Catalog) -> HashMap<String, (String, String)> {
    let toks = tokenize(stmt, engine);
    let name_of = |t: &Tok| match t {
        Tok::Word { raw, .. } => Some(raw.clone()),
        Tok::Quoted(s) => Some(s.clone()),
        _ => None,
    };
    let mut out = HashMap::new();
    let mut k = 0;
    while k < toks.len() {
        let is_ctx = matches!(&toks[k].tok, Tok::Word { upper, .. } if TABLE_CONTEXT.contains(&upper.as_str()));
        k += 1;
        if !is_ctx {
            continue;
        }
        // [schema.]table
        let Some(first) = toks.get(k).and_then(|t| name_of(&t.tok)) else {
            continue;
        };
        let mut schema = None;
        let mut table = first;
        k += 1;
        if matches!(toks.get(k).map(|t| &t.tok), Some(Tok::Dot))
            && let Some(t) = toks.get(k + 1).and_then(|t| name_of(&t.tok))
        {
            schema = Some(table);
            table = t;
            k += 2;
        }
        let found = catalog.tables.iter().find(|(s, t, _)| {
            t.eq_ignore_ascii_case(&table)
                && schema.as_ref().is_none_or(|x| s.eq_ignore_ascii_case(x))
        });
        let Some((s, t, _)) = found else { continue };
        let key = (s.clone(), t.clone());
        out.insert(t.to_lowercase(), key.clone());
        // [AS] alias
        if matches!(toks.get(k).map(|t| &t.tok), Some(Tok::Word { upper, .. }) if upper == "AS") {
            k += 1;
        }
        if let Some(Tok::Word { raw, upper }) = toks.get(k).map(|t| &t.tok)
            && !KEYWORDS
                .iter()
                .any(|kw| kw.split(' ').next() == Some(upper.as_str()))
            && !matches!(upper.as_str(), "USING" | "NATURAL")
        {
            out.insert(raw.to_lowercase(), key);
            k += 1;
        }
    }
    out
}

fn quote_if_needed(name: &str, engine: Engine) -> String {
    crate::sql::quote_ident(name, engine)
}

/// Completion at byte offset `cursor` of `stmt` (the statement around the
/// cursor; offsets are relative to it).
pub fn complete(stmt: &str, cursor: usize, engine: Engine, catalog: &Catalog) -> Completion {
    let cursor = cursor.min(stmt.len());
    let before = &stmt[..cursor];
    let start = before
        .char_indices()
        .rev()
        .take_while(|(_, c)| is_ident(*c))
        .last()
        .map(|(i, _)| i)
        .unwrap_or(cursor);
    let prefix = before[start..].to_lowercase();
    let mut items = Vec::new();
    let ds = default_schema(engine, catalog);

    // qualifier. — columns of an alias/table, or tables of a schema
    if before[..start].ends_with('.') {
        let q_end = start - 1;
        let q_start = before[..q_end]
            .char_indices()
            .rev()
            .take_while(|(_, c)| is_ident(*c) || *c == '"' || *c == '`')
            .last()
            .map(|(i, _)| i)
            .unwrap_or(q_end);
        let qualifier = before[q_start..q_end]
            .trim_matches(['"', '`'])
            .to_lowercase();
        let map = aliases(stmt, engine, catalog);
        if let Some((s, t)) = map.get(&qualifier)
            && let Some((_, _, cols)) = catalog.tables.iter().find(|(cs, ct, _)| cs == s && ct == t)
        {
            for c in cols {
                items.push(Candidate {
                    label: c.clone(),
                    insert: quote_if_needed(c, engine),
                    kind: Kind::Column,
                    detail: t.clone(),
                });
            }
        } else {
            for (s, t, _) in &catalog.tables {
                if s.to_lowercase() == qualifier {
                    items.push(Candidate {
                        label: t.clone(),
                        insert: quote_if_needed(t, engine),
                        kind: Kind::Table,
                        detail: s.clone(),
                    });
                }
            }
        }
    } else {
        let toks = tokenize(&stmt[..start], engine);
        let prev = toks.last().and_then(|t| match &t.tok {
            Tok::Word { upper, .. } => Some(upper.clone()),
            _ => None,
        });
        let table_ctx = prev.as_deref().is_some_and(|p| TABLE_CONTEXT.contains(&p));
        let push_tables = |items: &mut Vec<Candidate>| {
            for (s, t, _) in &catalog.tables {
                let insert = if *s == ds || s.is_empty() {
                    quote_if_needed(t, engine)
                } else {
                    format!(
                        "{}.{}",
                        quote_if_needed(s, engine),
                        quote_if_needed(t, engine)
                    )
                };
                items.push(Candidate {
                    label: t.clone(),
                    insert,
                    kind: Kind::Table,
                    detail: s.clone(),
                });
            }
        };
        if table_ctx {
            push_tables(&mut items);
        } else {
            let map = aliases(stmt, engine, catalog);
            let mut seen = std::collections::HashSet::new();
            for (s, t) in map.values() {
                if !seen.insert((s.clone(), t.clone())) {
                    continue;
                }
                if let Some((_, _, cols)) =
                    catalog.tables.iter().find(|(cs, ct, _)| cs == s && ct == t)
                {
                    for c in cols {
                        items.push(Candidate {
                            label: c.clone(),
                            insert: quote_if_needed(c, engine),
                            kind: Kind::Column,
                            detail: t.clone(),
                        });
                    }
                }
            }
            push_tables(&mut items);
            for k in KEYWORDS {
                items.push(Candidate {
                    label: (*k).to_string(),
                    insert: (*k).to_string(),
                    kind: Kind::Keyword,
                    detail: String::new(),
                });
            }
        }
    }

    // Prefix matches first, then substring matches; columns before tables
    // before keywords; nothing when the word is already complete.
    let rank = |c: &Candidate| {
        let l = c.label.to_lowercase();
        if l.starts_with(&prefix) {
            Some(0)
        } else if prefix.len() >= 2 && l.contains(&prefix) {
            Some(1)
        } else {
            None
        }
    };
    let mut ranked: Vec<(u8, Candidate)> = items
        .into_iter()
        .filter_map(|c| rank(&c).map(|r| (r, c)))
        .filter(|(_, c)| c.label.to_lowercase() != prefix || prefix.is_empty())
        .collect();
    ranked.sort_by(|a, b| {
        (a.0, a.1.kind, a.1.label.to_lowercase()).cmp(&(b.0, b.1.kind, b.1.label.to_lowercase()))
    });
    ranked.dedup_by(|a, b| {
        a.1.label == b.1.label && a.1.kind == b.1.kind && a.1.detail == b.1.detail
    });
    Completion {
        start,
        end: cursor,
        items: ranked.into_iter().map(|(_, c)| c).take(60).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cat() -> Catalog {
        Catalog {
            tables: vec![
                (
                    "public".into(),
                    "orders".into(),
                    vec!["id".into(), "status".into(), "customer_id".into()],
                ),
                (
                    "public".into(),
                    "customers".into(),
                    vec!["id".into(), "email".into()],
                ),
                (
                    "audit".into(),
                    "log".into(),
                    vec!["at".into(), "Message".into()],
                ),
            ],
        }
    }

    fn labels(c: &Completion) -> Vec<&str> {
        c.items.iter().map(|i| i.label.as_str()).collect()
    }

    #[test]
    fn tables_after_from_and_join() {
        let sql = "select * from or";
        let c = complete(sql, sql.len(), Engine::Postgres, &cat());
        assert_eq!(labels(&c), vec!["orders"]);
        assert_eq!(&sql[c.start..c.end], "or");
        let sql = "select * from orders o join ";
        let c = complete(sql, sql.len(), Engine::Postgres, &cat());
        assert!(labels(&c).contains(&"customers"));
        let log = c.items.iter().find(|i| i.label == "log").unwrap();
        assert_eq!(log.insert, "audit.log", "other schemas are qualified");
    }

    #[test]
    fn columns_after_an_alias_or_table() {
        let sql = "select o. from orders o join customers c on c.id = o.customer_id";
        let c = complete(sql, 9, Engine::Postgres, &cat());
        assert_eq!(labels(&c), vec!["customer_id", "id", "status"]);
        let sql = "select c.em from orders o join customers as c";
        let c = complete(sql, 11, Engine::Postgres, &cat());
        assert_eq!(labels(&c), vec!["email"]);
        let sql = "select * from audit.";
        let c = complete(sql, sql.len(), Engine::Postgres, &cat());
        assert_eq!(labels(&c), vec!["log"]);
        let sql = "select l.mes from audit.log l";
        let c = complete(sql, 12, Engine::Postgres, &cat());
        assert_eq!(
            c.items[0].insert, "\"Message\"",
            "mixed case is quoted in Postgres"
        );
    }

    #[test]
    fn otherwise_columns_tables_and_keywords() {
        let sql = "select sta from orders where ";
        let c = complete(sql, 10, Engine::Postgres, &cat());
        assert_eq!(labels(&c)[0], "status");
        let sql = "select * from orders wh";
        let c = complete(sql, sql.len(), Engine::Postgres, &cat());
        assert_eq!(labels(&c), vec!["WHEN", "WHERE"]);
        let sql = "sel";
        assert_eq!(
            labels(&complete(sql, 3, Engine::Mysql, &cat())),
            vec!["SELECT"]
        );
        // A finished word offers nothing more.
        assert!(
            complete("select", 6, Engine::Mysql, &cat())
                .items
                .is_empty()
        );
    }
}
