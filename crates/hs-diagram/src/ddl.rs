//! A small SQL DDL importer for ERD documents. It reads the subset developers
//! paste most often: `CREATE TABLE` with column types, inline and table-level
//! keys, `REFERENCES`, and `ALTER TABLE … FOREIGN KEY`.

use crate::model::{
    Cardinality, Column, ColumnKey, DiagramDoc, DiagramKind, Edge, EdgeRoute, Entity, Node,
    NodeKind, Style,
};

/// Parse `CREATE TABLE` / `ALTER TABLE … FOREIGN KEY` into an ERD.
pub fn import(text: &str) -> Result<DiagramDoc, String> {
    let cleaned = strip_comments(text);
    let mut doc = DiagramDoc::new(DiagramKind::Erd, "Imported schema");
    doc.subtitle = "Imported from SQL DDL".into();
    doc.auto_layout = true;
    let mut foreign: Vec<(String, String, String)> = Vec::new();

    for statement in cleaned.split(';') {
        let statement = statement.trim();
        if statement.is_empty() {
            continue;
        }
        let upper = statement.to_ascii_uppercase();
        if upper.starts_with("CREATE TABLE") {
            parse_create(statement, &mut doc, &mut foreign)?;
        } else if upper.starts_with("ALTER TABLE") {
            parse_alter(statement, &mut foreign)?;
        }
    }
    if doc.nodes.is_empty() {
        return Err("no CREATE TABLE statement found".into());
    }

    let mut seen = std::collections::HashSet::new();
    for (from, to, _column) in foreign {
        if from == to || !seen.insert((from.clone(), to.clone())) {
            continue;
        }
        doc.edges.push(Edge {
            id: format!("e{}", doc.edges.len() + 1),
            from: from.clone(),
            to: to.clone(),
            label: "1 .. *".into(),
            route: EdgeRoute::Straight,
            cardinality: Some(Cardinality::OneToMany),
            ..Default::default()
        });
    }
    Ok(doc)
}

fn parse_create(
    statement: &str,
    doc: &mut DiagramDoc,
    foreign: &mut Vec<(String, String, String)>,
) -> Result<(), String> {
    let open = statement
        .find('(')
        .ok_or("CREATE TABLE without a column list")?;
    let close = statement
        .rfind(')')
        .ok_or("CREATE TABLE without a closing )")?;
    let head = statement[..open].trim();
    let name = head
        .split_whitespace()
        .last()
        .map(clean_ident)
        .filter(|n| !n.is_empty())
        .ok_or("CREATE TABLE without a name")?;
    let body = &statement[open + 1..close];
    let parts = split_commas(body);
    let mut columns = Vec::new();
    let mut primary: Vec<String> = Vec::new();
    let mut constraints: Vec<String> = Vec::new();
    for part in parts {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let upper = part.to_ascii_uppercase();
        if upper.starts_with("PRIMARY KEY") {
            primary.extend(paren_names(part));
            continue;
        }
        if upper.starts_with("CONSTRAINT")
            || upper.starts_with("FOREIGN KEY")
            || upper.starts_with("UNIQUE")
            || upper.starts_with("CHECK")
        {
            constraints.push(part.to_string());
            continue;
        }
        let mut words = part.split_whitespace();
        let Some(name) = words.next() else { continue };
        let name = clean_ident(name);
        let data_type = words
            .next()
            .unwrap_or("TEXT")
            .trim_end_matches(',')
            .to_string();
        let rest = part.to_ascii_uppercase();
        let key = if rest.contains("PRIMARY KEY") {
            ColumnKey::Primary
        } else if rest.contains("REFERENCES") {
            ColumnKey::Foreign
        } else if rest.contains("UNIQUE") {
            ColumnKey::Unique
        } else {
            ColumnKey::None
        };
        if key == ColumnKey::Foreign
            && let Some(target) = reference_target(part)
            && target != name
        {
            foreign.push((name.clone(), target, name.clone()));
        }
        columns.push(Column {
            name,
            data_type,
            key,
        });
    }
    for column in &mut columns {
        if primary.contains(&column.name) {
            column.key = ColumnKey::Primary;
        }
    }
    for constraint in constraints {
        if let Some((column, target)) = constraint_reference(&constraint) {
            foreign.push((column, target, String::new()));
        }
    }
    let id = sanitize(&name);
    if doc.nodes.iter().any(|n| n.id == id) {
        return Ok(());
    }
    doc.nodes.push(Node {
        id,
        label: name,
        kind: NodeKind::Entity,
        x: 0.0,
        y: 0.0,
        w: 0.0,
        h: 0.0,
        style: Style::default(),
        erd: Some(Entity { columns }),
    });
    Ok(())
}

fn parse_alter(statement: &str, foreign: &mut Vec<(String, String, String)>) -> Result<(), String> {
    let upper = statement.to_ascii_uppercase();
    let Some(table_pos) = upper.find("TABLE") else {
        return Ok(());
    };
    let after = statement[table_pos + 5..].trim();
    let Some(table) = after.split_whitespace().next() else {
        return Ok(());
    };
    let table = clean_ident(table);
    if let Some((column, target)) = constraint_reference(statement) {
        foreign.push((column, target, table));
    }
    Ok(())
}

/// `(a, b)` → `[a, b]`.
fn paren_names(text: &str) -> Vec<String> {
    let Some(open) = text.find('(') else {
        return Vec::new();
    };
    let Some(close) = text[open..].find(')') else {
        return Vec::new();
    };
    text[open + 1..open + close]
        .split(',')
        .map(|s| clean_ident(s.trim()))
        .filter(|s| !s.is_empty())
        .collect()
}

/// The local column and referenced table of a `FOREIGN KEY (c) REFERENCES t`
/// or inline `c … REFERENCES t` fragment.
fn constraint_reference(text: &str) -> Option<(String, String)> {
    let upper = text.to_ascii_uppercase();
    let key = upper.find("FOREIGN KEY")?;
    let after = &text[key + "FOREIGN KEY".len()..];
    let column = paren_names(after).first().cloned()?;
    let target = reference_target(after)?;
    Some((column, target))
}

fn reference_target(text: &str) -> Option<String> {
    let upper = text.to_ascii_uppercase();
    let at = upper.find("REFERENCES")?;
    let after = text[at + "REFERENCES".len()..].trim();
    let target = after
        .split(|c: char| c.is_whitespace() || c == '(')
        .next()?;
    Some(clean_ident(target))
}

fn split_commas(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;
    for (i, c) in text.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => {
                out.push(&text[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(&text[start..]);
    out
}

fn strip_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '-' && chars.peek() == Some(&'-') {
            for c in chars.by_ref() {
                if c == '\n' {
                    out.push('\n');
                    break;
                }
            }
        } else if c == '/' && chars.peek() == Some(&'*') {
            chars.next();
            let mut prev = '\0';
            for c in chars.by_ref() {
                if prev == '*' && c == '/' {
                    break;
                }
                prev = c;
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn clean_ident(name: &str) -> String {
    let last = name.rsplit('.').next().unwrap_or(name);
    last.trim_matches(|c| c == '"' || c == '`' || c == '[' || c == ']' || c == '\'')
        .trim()
        .to_string()
}

fn sanitize(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() || c == '_' {
            out.push(c);
        } else if c.is_whitespace() {
            out.push('_');
        }
    }
    if out.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        out.insert(0, 't');
    }
    if out.is_empty() { "table".into() } else { out }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_tables_keys_and_foreign_keys() {
        let sql = r#"
            CREATE TABLE customers (
                id UUID PRIMARY KEY,
                email TEXT UNIQUE
            );
            CREATE TABLE orders (
                id UUID PRIMARY KEY,
                customer_id UUID REFERENCES customers(id),
                total NUMERIC
            );
            ALTER TABLE orders
                ADD CONSTRAINT fk_customer FOREIGN KEY (customer_id) REFERENCES customers(id);
        "#;
        let doc = import(sql).expect("parses");
        assert_eq!(doc.nodes.len(), 2);
        let orders = doc.nodes.iter().find(|n| n.label == "orders").unwrap();
        let erd = orders.erd.as_ref().unwrap();
        assert_eq!(erd.columns.len(), 3);
        assert_eq!(erd.columns[0].key, ColumnKey::Primary);
        assert_eq!(erd.columns[1].key, ColumnKey::Foreign);
        assert_eq!(doc.edges.len(), 1);
        assert_eq!(doc.edges[0].label, "1 .. *");
    }

    #[test]
    fn rejects_input_without_tables() {
        assert!(import("SELECT 1;").is_err());
    }
}
