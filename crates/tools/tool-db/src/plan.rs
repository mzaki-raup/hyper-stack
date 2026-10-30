//! Query plans as a tree (docs/tools/database.md §10): Postgres
//! `EXPLAIN (FORMAT JSON)`, MySQL `EXPLAIN FORMAT=JSON` and SQLite
//! `EXPLAIN QUERY PLAN` rows all become the same [`Plan`], so the result
//! panel draws one tree with cost, rows and time per node and the hottest
//! nodes marked.

use crate::driver::{Column, Engine, Row, Value};

/// One plan node.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Node {
    /// "Seq Scan on orders", "Index Scan using orders_pkey on orders",
    /// "SEARCH t USING INTEGER PRIMARY KEY (rowid=?)".
    pub label: String,
    /// Smaller facts: filter, join condition, index condition, sort key.
    pub detail: Vec<(String, String)>,
    /// Total cost (Postgres) or query cost (MySQL) of the node.
    pub cost: Option<f64>,
    /// Estimated rows.
    pub rows: Option<f64>,
    /// Actual rows (ANALYZE).
    pub actual_rows: Option<f64>,
    /// Actual total time in ms for the node (ANALYZE).
    pub time_ms: Option<f64>,
    pub children: Vec<Node>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Plan {
    pub roots: Vec<Node>,
    /// Planning / execution time, when the server reported them.
    pub planning_ms: Option<f64>,
    pub execution_ms: Option<f64>,
    /// The largest `cost` and `time_ms` over every node, for the heat.
    pub max_cost: f64,
    pub max_time: f64,
}

impl Plan {
    /// How hot a node is in 0..=1: its own time when the plan was analysed,
    /// its cost otherwise.
    pub fn heat(&self, n: &Node) -> f32 {
        match (n.time_ms, n.cost) {
            (Some(t), _) if self.max_time > 0.0 => (t / self.max_time) as f32,
            (_, Some(c)) if self.max_cost > 0.0 => (c / self.max_cost) as f32,
            _ => 0.0,
        }
    }

    fn finish(mut self) -> Plan {
        fn walk(n: &Node, mc: &mut f64, mt: &mut f64) {
            if let Some(c) = n.cost {
                *mc = mc.max(c);
            }
            if let Some(t) = n.time_ms {
                *mt = mt.max(t);
            }
            for c in &n.children {
                walk(c, mc, mt);
            }
        }
        let (mut mc, mut mt) = (0.0, 0.0);
        for r in &self.roots {
            walk(r, &mut mc, &mut mt);
        }
        self.max_cost = mc;
        self.max_time = mt;
        self
    }
}

/// The statement to run for a plan of `sql`.
pub fn explain_sql(sql: &str, engine: Engine, analyze: bool) -> String {
    match engine {
        Engine::Sqlite => format!("EXPLAIN QUERY PLAN {sql}"),
        Engine::Postgres if analyze => format!("EXPLAIN (ANALYZE, FORMAT JSON) {sql}"),
        Engine::Postgres => format!("EXPLAIN (FORMAT JSON) {sql}"),
        Engine::Mysql if analyze => format!("EXPLAIN ANALYZE {sql}"),
        Engine::Mysql => format!("EXPLAIN FORMAT=JSON {sql}"),
    }
}

/// Whether a result came from an EXPLAIN this module can draw.
pub fn is_explain(sql: &str) -> bool {
    sql.trim_start()
        .get(..7)
        .is_some_and(|p| p.eq_ignore_ascii_case("EXPLAIN"))
}

/// Parse the rows an EXPLAIN returned. `None` when they are not a shape
/// this module knows (plain-text EXPLAIN stays a grid).
pub fn parse(engine: Engine, columns: &[Column], rows: &[Row]) -> Option<Plan> {
    match engine {
        Engine::Postgres => {
            let text: String = rows
                .iter()
                .filter_map(|r| r.first())
                .map(Value::display)
                .collect::<Vec<_>>()
                .join("\n");
            let v: serde_json::Value = serde_json::from_str(text.trim()).ok()?;
            let items = v.as_array()?;
            let mut plan = Plan::default();
            for item in items {
                if let Some(p) = item.get("Plan") {
                    plan.roots.push(pg_node(p));
                }
                plan.planning_ms = plan
                    .planning_ms
                    .or(item.get("Planning Time").and_then(|x| x.as_f64()));
                plan.execution_ms = plan
                    .execution_ms
                    .or(item.get("Execution Time").and_then(|x| x.as_f64()));
            }
            (!plan.roots.is_empty()).then(|| plan.finish())
        }
        Engine::Mysql => {
            let text = rows.first()?.first()?.display();
            let v: serde_json::Value = serde_json::from_str(text.trim()).ok()?;
            let root = v.get("query_block")?;
            let mut plan = Plan::default();
            plan.roots.push(my_node("query_block", root));
            Some(plan.finish())
        }
        Engine::Sqlite => {
            // id, parent, notused, detail
            let id_col = columns.iter().position(|c| c.name == "id")?;
            let parent_col = columns.iter().position(|c| c.name == "parent")?;
            let detail_col = columns.iter().position(|c| c.name == "detail")?;
            let items: Vec<(i64, i64, String)> = rows
                .iter()
                .map(|r| {
                    let int = |i: usize| match r.get(i) {
                        Some(Value::Int(n)) => *n,
                        Some(other) => other.display().parse().unwrap_or(0),
                        None => 0,
                    };
                    (
                        int(id_col),
                        int(parent_col),
                        r.get(detail_col).map(Value::display).unwrap_or_default(),
                    )
                })
                .collect();
            fn build(items: &[(i64, i64, String)], parent: i64) -> Vec<Node> {
                items
                    .iter()
                    .filter(|(_, p, _)| *p == parent)
                    .map(|(id, _, detail)| Node {
                        label: detail.clone(),
                        children: build(items, *id),
                        ..Default::default()
                    })
                    .collect()
            }
            let plan = Plan {
                roots: build(&items, 0),
                ..Default::default()
            };
            (!plan.roots.is_empty()).then(|| plan.finish())
        }
    }
}

fn f64_of(v: Option<&serde_json::Value>) -> Option<f64> {
    v.and_then(|x| {
        x.as_f64()
            .or_else(|| x.as_str().and_then(|s| s.parse().ok()))
    })
}

fn pg_node(v: &serde_json::Value) -> Node {
    let s = |k: &str| v.get(k).and_then(|x| x.as_str()).map(str::to_string);
    let mut label = s("Node Type").unwrap_or_else(|| "?".into());
    if let Some(idx) = s("Index Name") {
        label.push_str(&format!(" using {idx}"));
    }
    if let Some(rel) = s("Relation Name") {
        label.push_str(&format!(" on {rel}"));
        if let Some(alias) = s("Alias")
            && alias != rel
        {
            label.push_str(&format!(" {alias}"));
        }
    }
    if let Some(cte) = s("CTE Name") {
        label.push_str(&format!(" {cte}"));
    }
    let mut detail = Vec::new();
    for (key, name) in [
        ("Join Type", "join"),
        ("Hash Cond", "hash cond"),
        ("Merge Cond", "merge cond"),
        ("Join Filter", "join filter"),
        ("Index Cond", "index cond"),
        ("Recheck Cond", "recheck"),
        ("Filter", "filter"),
        ("Sort Method", "sort method"),
        ("Strategy", "strategy"),
        ("Scan Direction", "direction"),
        ("Parallel Aware", "parallel"),
    ] {
        if let Some(x) = v.get(key) {
            let text = match x {
                serde_json::Value::String(t) => t.clone(),
                other => other.to_string(),
            };
            if !(key == "Parallel Aware" && text == "false") {
                detail.push((name.to_string(), text));
            }
        }
    }
    if let Some(keys) = v.get("Sort Key").and_then(|x| x.as_array()) {
        let text = keys
            .iter()
            .filter_map(|k| k.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        detail.push(("sort key".into(), text));
    }
    if let Some(n) = f64_of(v.get("Rows Removed by Filter"))
        && n > 0.0
    {
        detail.push(("rows removed by filter".into(), n.to_string()));
    }
    if let Some(n) = f64_of(v.get("Actual Loops"))
        && n > 1.0
    {
        detail.push(("loops".into(), n.to_string()));
    }
    Node {
        label,
        detail,
        cost: f64_of(v.get("Total Cost")),
        rows: f64_of(v.get("Plan Rows")),
        actual_rows: f64_of(v.get("Actual Rows")),
        time_ms: f64_of(v.get("Actual Total Time")),
        children: v
            .get("Plans")
            .and_then(|p| p.as_array())
            .map(|ps| ps.iter().map(pg_node).collect())
            .unwrap_or_default(),
    }
}

/// MySQL's JSON is a nest of named objects rather than a list of nodes:
/// every object with a `table` or a known block name becomes a node.
fn my_node(name: &str, v: &serde_json::Value) -> Node {
    let obj = v.as_object();
    let mut node = Node {
        label: name.to_string(),
        ..Default::default()
    };
    let Some(obj) = obj else {
        return node;
    };
    if let Some(t) = obj.get("table_name").and_then(|x| x.as_str()) {
        let access = obj
            .get("access_type")
            .and_then(|x| x.as_str())
            .unwrap_or("");
        node.label = if access.is_empty() {
            format!("table {t}")
        } else {
            format!("{access} on {t}")
        };
        if let Some(k) = obj.get("key").and_then(|x| x.as_str()) {
            node.label.push_str(&format!(" using {k}"));
        }
    }
    if let Some(ci) = obj.get("cost_info").and_then(|x| x.as_object()) {
        node.cost = ci
            .get("query_cost")
            .or(ci.get("prefix_cost"))
            .or(ci.get("read_cost"))
            .and_then(|x| {
                x.as_f64()
                    .or_else(|| x.as_str().and_then(|s| s.parse().ok()))
            });
    }
    node.rows =
        f64_of(obj.get("rows_examined_per_scan")).or(f64_of(obj.get("rows_produced_per_join")));
    for (key, label) in [
        ("attached_condition", "condition"),
        ("used_columns", "columns"),
        ("possible_keys", "possible keys"),
        ("ref", "ref"),
        ("filtered", "filtered %"),
        ("using_filesort", "filesort"),
        ("using_temporary_table", "temporary"),
        ("select_id", "select id"),
    ] {
        if let Some(x) = obj.get(key) {
            let text = match x {
                serde_json::Value::String(t) => t.clone(),
                serde_json::Value::Array(a) => a
                    .iter()
                    .map(|e| {
                        e.as_str()
                            .map(str::to_string)
                            .unwrap_or_else(|| e.to_string())
                    })
                    .collect::<Vec<_>>()
                    .join(", "),
                other => other.to_string(),
            };
            if text != "false" {
                node.detail.push((label.to_string(), text));
            }
        }
    }
    for (k, child) in obj {
        match child {
            serde_json::Value::Object(_) if k != "cost_info" => {
                node.children.push(my_node(k, child));
            }
            serde_json::Value::Array(items) => {
                for (i, item) in items.iter().enumerate() {
                    if item.is_object() {
                        node.children.push(my_node(&format!("{k}[{i}]"), item));
                    }
                }
            }
            _ => {}
        }
    }
    // A wrapper object with one child and nothing to say collapses into it.
    if node.children.len() == 1
        && node.detail.is_empty()
        && node.cost.is_none()
        && !obj.contains_key("table_name")
    {
        let child = node.children.remove(0);
        return Node {
            label: format!("{} › {}", node.label, child.label),
            ..child
        };
    }
    node
}

/// The plan as indented text, for Copy.
pub fn to_text(plan: &Plan) -> String {
    fn walk(n: &Node, depth: usize, out: &mut String) {
        let pad = "  ".repeat(depth);
        out.push_str(&pad);
        out.push_str(&n.label);
        let mut facts = Vec::new();
        if let Some(c) = n.cost {
            facts.push(format!("cost={c}"));
        }
        if let Some(r) = n.rows {
            facts.push(format!("rows={r}"));
        }
        if let Some(r) = n.actual_rows {
            facts.push(format!("actual rows={r}"));
        }
        if let Some(t) = n.time_ms {
            facts.push(format!("time={t} ms"));
        }
        if !facts.is_empty() {
            out.push_str(&format!("  ({})", facts.join(", ")));
        }
        out.push('\n');
        for (k, v) in &n.detail {
            out.push_str(&format!("{pad}  {k}: {v}\n"));
        }
        for c in &n.children {
            walk(c, depth + 1, out);
        }
    }
    let mut out = String::new();
    for r in &plan.roots {
        walk(r, 0, &mut out);
    }
    if let Some(p) = plan.planning_ms {
        out.push_str(&format!("planning: {p} ms\n"));
    }
    if let Some(e) = plan.execution_ms {
        out.push_str(&format!("execution: {e} ms\n"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn col(n: &str) -> Column {
        Column {
            name: n.into(),
            type_name: String::new(),
        }
    }

    #[test]
    fn explain_sql_per_engine() {
        assert_eq!(
            explain_sql("select 1", Engine::Sqlite, true),
            "EXPLAIN QUERY PLAN select 1"
        );
        assert_eq!(
            explain_sql("select 1", Engine::Postgres, true),
            "EXPLAIN (ANALYZE, FORMAT JSON) select 1"
        );
        assert_eq!(
            explain_sql("select 1", Engine::Mysql, false),
            "EXPLAIN FORMAT=JSON select 1"
        );
        assert!(is_explain("  explain select 1"));
        assert!(!is_explain("select 1"));
    }

    #[test]
    fn a_postgres_json_plan_becomes_a_tree() {
        let json = r#"[{"Plan": {"Node Type": "Hash Join", "Join Type": "Inner", "Total Cost": 50.5, "Plan Rows": 10,
            "Hash Cond": "(o.customer_id = c.id)", "Actual Total Time": 1.5, "Actual Rows": 9,
            "Plans": [
              {"Node Type": "Seq Scan", "Relation Name": "orders", "Alias": "o", "Total Cost": 20.0, "Plan Rows": 100, "Filter": "(total > 5)", "Rows Removed by Filter": 3, "Actual Total Time": 0.9},
              {"Node Type": "Hash", "Total Cost": 10.0, "Plan Rows": 5, "Plans": [
                 {"Node Type": "Index Scan", "Index Name": "customers_pkey", "Relation Name": "customers", "Alias": "c", "Total Cost": 8.0, "Plan Rows": 5, "Actual Total Time": 0.2}
              ]}
            ]}, "Planning Time": 0.3, "Execution Time": 2.1}]"#;
        let rows = vec![vec![Value::Text(json.into())]];
        let plan = parse(Engine::Postgres, &[col("QUERY PLAN")], &rows).expect("parsed");
        assert_eq!(plan.roots.len(), 1);
        let root = &plan.roots[0];
        assert_eq!(root.label, "Hash Join");
        assert_eq!(root.children.len(), 2);
        assert_eq!(root.children[0].label, "Seq Scan on orders o");
        assert_eq!(
            root.children[1].children[0].label,
            "Index Scan using customers_pkey on customers c"
        );
        assert_eq!(root.cost, Some(50.5));
        assert_eq!(plan.planning_ms, Some(0.3));
        assert_eq!(plan.execution_ms, Some(2.1));
        assert!((plan.max_time - 1.5).abs() < 1e-9);
        assert!((plan.heat(root) - 1.0).abs() < 1e-6);
        assert!(plan.heat(&root.children[0]) < 1.0);
        assert!(
            root.children[0]
                .detail
                .iter()
                .any(|(k, v)| k == "filter" && v == "(total > 5)")
        );
        let text = to_text(&plan);
        assert!(text.starts_with("Hash Join  (cost=50.5"));
        assert!(text.contains("    Index Scan using customers_pkey"));
    }

    #[test]
    fn a_mysql_json_plan_becomes_a_tree() {
        let json = r#"{"query_block": {"select_id": 1, "cost_info": {"query_cost": "3.40"},
            "nested_loop": [
              {"table": {"table_name": "c", "access_type": "ALL", "rows_examined_per_scan": 3, "cost_info": {"prefix_cost": "0.55"}}},
              {"table": {"table_name": "o", "access_type": "ref", "key": "customer_id", "rows_examined_per_scan": 2, "attached_condition": "(o.total > 5)"}}
            ]}}"#;
        let rows = vec![vec![Value::Text(json.into())]];
        let plan = parse(Engine::Mysql, &[col("EXPLAIN")], &rows).expect("parsed");
        let root = &plan.roots[0];
        assert_eq!(root.cost, Some(3.4));
        let loops: Vec<&Node> = root
            .children
            .iter()
            .filter(|c| c.label.starts_with("nested_loop"))
            .collect();
        assert_eq!(loops.len(), 2, "{:?}", root.children);
        assert_eq!(loops[0].label, "nested_loop[0] › ALL on c");
        assert!(loops[1].label.contains("ref on o using customer_id"));
        assert!(
            loops[1]
                .detail
                .iter()
                .any(|(k, v)| k == "condition" && v == "(o.total > 5)")
        );
    }

    #[test]
    fn sqlite_rows_nest_by_parent() {
        let cols = [col("id"), col("parent"), col("notused"), col("detail")];
        let rows: Vec<Row> = vec![
            vec![
                Value::Int(3),
                Value::Int(0),
                Value::Int(0),
                Value::Text("SCAN o".into()),
            ],
            vec![
                Value::Int(8),
                Value::Int(3),
                Value::Int(0),
                Value::Text("SEARCH c USING INTEGER PRIMARY KEY (rowid=?)".into()),
            ],
        ];
        let plan = parse(Engine::Sqlite, &cols, &rows).expect("parsed");
        assert_eq!(plan.roots.len(), 1);
        assert_eq!(
            plan.roots[0].children[0].label,
            "SEARCH c USING INTEGER PRIMARY KEY (rowid=?)"
        );
        assert!(parse(Engine::Sqlite, &[col("x")], &rows).is_none());
    }
}
