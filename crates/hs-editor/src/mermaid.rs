//! A native renderer for the Mermaid subset the docs use (docs/tools/editor.md
//! §5, "Markdown preview"): `flowchart`/`graph` and `sequenceDiagram`.
//!
//! Three steps, each testable without egui: [`parse`] turns the source into a
//! [`Diagram`], [`layout`] places it into a [`Scene`] of plain shapes, lines
//! and text using a caller-supplied text measurer, and [`paint`] draws the
//! scene. Anything outside the subset is reported as an error so the preview
//! can fall back to showing the source.

use std::collections::HashMap;

use egui::{Color32, Pos2, Rect, Vec2, pos2, vec2};

// ---------------------------------------------------------------------------
// Model

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    TopDown,
    BottomUp,
    LeftRight,
    RightLeft,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    Rect,
    Round,
    Stadium,
    Diamond,
    Circle,
    Cylinder,
    Hexagon,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    pub id: String,
    pub label: String,
    pub shape: Shape,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineStyle {
    Solid,
    Dotted,
    Thick,
}

/// An edge end: a node, or a whole subgraph (Mermaid allows `A --> group`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum End {
    Node(usize),
    Group(usize),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edge {
    pub from: End,
    pub to: End,
    pub label: String,
    pub style: LineStyle,
    pub arrow: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    pub id: String,
    pub title: String,
    /// Every node inside, including nested groups' nodes.
    pub nodes: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Flowchart {
    pub direction: Direction,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub groups: Vec<Group>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Participant {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoteSide {
    Left,
    Right,
    Over,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SeqItem {
    Message {
        from: usize,
        to: usize,
        text: String,
        dashed: bool,
        arrow: bool,
    },
    Note {
        side: NoteSide,
        a: usize,
        b: usize,
        text: String,
    },
    /// `loop`, `alt`, `opt`, `par`, `critical`, `break`, `rect`.
    BlockStart {
        kind: String,
        label: String,
    },
    /// `else` / `and`.
    BlockSplit {
        label: String,
    },
    BlockEnd,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sequence {
    pub participants: Vec<Participant>,
    pub items: Vec<SeqItem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Diagram {
    Flow(Flowchart),
    Sequence(Sequence),
}

// ---------------------------------------------------------------------------
// Parsing

/// Parse a Mermaid source. Errors name what is unsupported or malformed.
pub fn parse(src: &str) -> Result<Diagram, String> {
    let mut lines = src
        .lines()
        .map(|l| strip_comment(l).trim())
        .filter(|l| !l.is_empty());
    let header = lines.next().ok_or("empty diagram")?;
    let kind = header.split_whitespace().next().unwrap_or("");
    let rest: Vec<&str> = lines.collect();
    match kind {
        "flowchart" | "graph" => {
            let dir = header.split_whitespace().nth(1).unwrap_or("TB");
            parse_flow(dir, &rest).map(Diagram::Flow)
        }
        "sequenceDiagram" => parse_sequence(&rest).map(Diagram::Sequence),
        other => Err(format!("\"{other}\" diagrams are not supported")),
    }
}

fn strip_comment(l: &str) -> &str {
    match l.find("%%") {
        Some(i) => &l[..i],
        None => l,
    }
}

/// `<br>`/`<br/>` to newlines, surrounding quotes dropped, a few entities.
fn clean_label(s: &str) -> String {
    let s = s.trim();
    let s = s
        .strip_prefix('"')
        .and_then(|x| x.strip_suffix('"'))
        .unwrap_or(s);
    let mut out = s.to_string();
    for br in ["<br/>", "<br />", "<br>", "<BR>", "<BR/>"] {
        out = out.replace(br, "\n");
    }
    out.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("#quot;", "\"")
}

/// Split a line into statements on `;` outside brackets and quotes.
fn split_statements(line: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut quote = false;
    let mut start = 0;
    for (i, c) in line.char_indices() {
        match c {
            '"' => quote = !quote,
            '[' | '(' | '{' if !quote => depth += 1,
            ']' | ')' | '}' if !quote => depth -= 1,
            ';' if !quote && depth <= 0 => {
                out.push(&line[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(&line[start..]);
    out.into_iter()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect()
}

fn is_id_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

struct FlowBuilder {
    nodes: Vec<Node>,
    index: HashMap<String, usize>,
    edges: Vec<Edge>,
    groups: Vec<Group>,
    group_index: HashMap<String, usize>,
    stack: Vec<usize>,
}

impl FlowBuilder {
    fn node(&mut self, id: &str, def: Option<(String, Shape)>) -> usize {
        let i = match self.index.get(id) {
            Some(&i) => i,
            None => {
                self.nodes.push(Node {
                    id: id.to_string(),
                    label: id.to_string(),
                    shape: Shape::Rect,
                });
                let i = self.nodes.len() - 1;
                self.index.insert(id.to_string(), i);
                for &g in &self.stack {
                    self.groups[g].nodes.push(i);
                }
                i
            }
        };
        if let Some((label, shape)) = def {
            self.nodes[i].label = label;
            self.nodes[i].shape = shape;
        }
        i
    }

    fn end(&mut self, id: &str, def: Option<(String, Shape)>) -> End {
        if def.is_none()
            && let Some(&g) = self.group_index.get(id)
        {
            return End::Group(g);
        }
        End::Node(self.node(id, def))
    }
}

const SHAPES: &[(&str, &str, Shape)] = &[
    ("([", "])", Shape::Stadium),
    ("[(", ")]", Shape::Cylinder),
    ("((", "))", Shape::Circle),
    ("{{", "}}", Shape::Hexagon),
    ("[[", "]]", Shape::Rect),
    ("[/", "/]", Shape::Rect),
    ("[\\", "\\]", Shape::Rect),
    ("[", "]", Shape::Rect),
    ("(", ")", Shape::Round),
    ("{", "}", Shape::Diamond),
    (">", "]", Shape::Rect),
];

/// A node's label and shape when the reference defines them.
type NodeDef = Option<(String, Shape)>;

/// `id`, optionally followed by a shape with a label. Returns the rest.
fn parse_node_ref(s: &str) -> Option<(String, NodeDef, &str)> {
    let s = s.trim_start();
    let end = s.find(|c: char| !is_id_char(c)).unwrap_or(s.len());
    if end == 0 {
        return None;
    }
    let id = s[..end].to_string();
    let rest = &s[end..];
    for (open, close, shape) in SHAPES {
        if let Some(body) = rest.strip_prefix(open) {
            // Quoted labels may contain the closing bracket.
            let (label, after) = if let Some(q) = body.trim_start().strip_prefix('"') {
                let qe = q.find('"')?;
                let after = q[qe + 1..].trim_start().strip_prefix(close)?;
                (q[..qe].to_string(), after)
            } else {
                let ce = body.find(close)?;
                (body[..ce].to_string(), &body[ce + close.len()..])
            };
            return Some((id, Some((clean_label(&label), *shape)), after));
        }
    }
    Some((id, None, rest))
}

/// Node refs joined by `&`.
fn parse_node_group(s: &str) -> Option<(Vec<(String, NodeDef)>, &str)> {
    let mut out = Vec::new();
    let mut rest = s;
    loop {
        let (id, def, r) = parse_node_ref(rest)?;
        out.push((id, def));
        let t = r.trim_start();
        if let Some(after) = t.strip_prefix('&') {
            rest = after;
        } else {
            return Some((out, r));
        }
    }
}

struct Link {
    label: String,
    style: LineStyle,
    arrow: bool,
}

/// An edge operator at the start of `s`: `-->`, `---`, `-.->`, `==>`,
/// `--x`, `--o`, with `|label|` after or `-- label -->` around the text.
fn parse_link(s: &str) -> Option<(Link, &str)> {
    let t = s.trim_start();
    let style_of = |op: &str| {
        if op.contains('=') {
            LineStyle::Thick
        } else if op.contains('.') {
            LineStyle::Dotted
        } else {
            LineStyle::Solid
        }
    };
    // Plain operator.
    let op_len = t
        .char_indices()
        .take_while(|&(_, c)| matches!(c, '-' | '=' | '.' | '<'))
        .count();
    if op_len >= 2 {
        let mut op_end = op_len;
        let after_op = &t[op_len..];
        let mut arrow = false;
        let cross_or_circle = (after_op.starts_with('x') || after_op.starts_with('o'))
            && after_op[1..].starts_with(|c: char| c.is_whitespace() || c == '|');
        if after_op.starts_with('>') || cross_or_circle {
            arrow = true;
            op_end += 1;
        }
        let op = &t[..op_end];
        let label_form = !arrow && (op == "--" || op == "==" || op == "-.");
        if !label_form {
            let mut rest = &t[op_end..];
            let mut label = String::new();
            if let Some(r) = rest.trim_start().strip_prefix('|') {
                let e = r.find('|')?;
                label = clean_label(&r[..e]);
                rest = &r[e + 1..];
            }
            return Some((
                Link {
                    label,
                    style: style_of(op),
                    arrow,
                },
                rest,
            ));
        }
        // `-- text -->` form: find the closing operator.
        let closers: &[&str] = match op {
            "--" => &["-->", "---", "--x", "--o"],
            "==" => &["==>", "==="],
            _ => &[".->", ".-"],
        };
        let body = &t[op_end..];
        let (pos, closer) = closers
            .iter()
            .filter_map(|c| body.find(c).map(|p| (p, *c)))
            .min_by_key(|(p, _)| *p)?;
        return Some((
            Link {
                label: clean_label(&body[..pos]),
                style: style_of(op),
                arrow: closer.ends_with('>') || closer.ends_with('x') || closer.ends_with('o'),
            },
            &body[pos + closer.len()..],
        ));
    }
    None
}

fn parse_flow(dir: &str, lines: &[&str]) -> Result<Flowchart, String> {
    let direction = match dir {
        "TB" | "TD" => Direction::TopDown,
        "BT" => Direction::BottomUp,
        "LR" => Direction::LeftRight,
        "RL" => Direction::RightLeft,
        other => return Err(format!("unknown direction \"{other}\"")),
    };
    let mut b = FlowBuilder {
        nodes: Vec::new(),
        index: HashMap::new(),
        edges: Vec::new(),
        groups: Vec::new(),
        group_index: HashMap::new(),
        stack: Vec::new(),
    };
    for line in lines {
        for st in split_statements(line) {
            let first = st.split_whitespace().next().unwrap_or("");
            match first {
                "subgraph" => {
                    let rest = st["subgraph".len()..].trim();
                    let (id, title) = match parse_node_ref(rest) {
                        Some((id, Some((label, _)), _)) => (id, label),
                        Some((id, None, r)) if r.trim().is_empty() => (id.clone(), id),
                        _ => {
                            let t = clean_label(rest);
                            (t.clone(), t)
                        }
                    };
                    b.groups.push(Group {
                        id: id.clone(),
                        title,
                        nodes: Vec::new(),
                    });
                    let g = b.groups.len() - 1;
                    b.group_index.insert(id, g);
                    b.stack.push(g);
                    continue;
                }
                "end" => {
                    b.stack.pop();
                    continue;
                }
                "direction" | "classDef" | "class" | "style" | "linkStyle" | "click" => continue,
                _ => {}
            }
            let (mut left, mut rest) =
                parse_node_group(st).ok_or_else(|| format!("cannot read \"{st}\""))?;
            let mut left_ends: Vec<End> = left.drain(..).map(|(id, d)| b.end(&id, d)).collect();
            loop {
                let r = rest.trim_start();
                if r.is_empty() || r.starts_with(":::") {
                    break;
                }
                let (link, after) = parse_link(r).ok_or_else(|| format!("cannot read \"{st}\""))?;
                let (right, after) =
                    parse_node_group(after).ok_or_else(|| format!("cannot read \"{st}\""))?;
                let right_ends: Vec<End> = right.into_iter().map(|(id, d)| b.end(&id, d)).collect();
                for &f in &left_ends {
                    for &t in &right_ends {
                        b.edges.push(Edge {
                            from: f,
                            to: t,
                            label: link.label.clone(),
                            style: link.style,
                            arrow: link.arrow,
                        });
                    }
                }
                left_ends = right_ends;
                rest = after;
            }
        }
    }
    if b.nodes.is_empty() {
        return Err("the flowchart has no nodes".into());
    }
    Ok(Flowchart {
        direction,
        nodes: b.nodes,
        edges: b.edges,
        groups: b.groups,
    })
}

fn parse_sequence(lines: &[&str]) -> Result<Sequence, String> {
    let mut participants: Vec<Participant> = Vec::new();
    let mut items = Vec::new();
    let idx = |participants: &mut Vec<Participant>, id: &str| -> usize {
        let id = id.trim();
        match participants.iter().position(|p| p.id == id) {
            Some(i) => i,
            None => {
                participants.push(Participant {
                    id: id.to_string(),
                    label: id.to_string(),
                });
                participants.len() - 1
            }
        }
    };
    // Longest arrows first so `-->>` is not read as `-->`.
    const ARROWS: &[(&str, bool, bool)] = &[
        ("-->>", true, true),
        ("->>", false, true),
        ("--x", true, true),
        ("-x", false, true),
        ("--)", true, true),
        ("-)", false, true),
        ("-->", true, false),
        ("->", false, false),
    ];
    for line in lines {
        let first = line.split_whitespace().next().unwrap_or("");
        match first {
            "participant" | "actor" => {
                let rest = line[first.len()..].trim();
                let (id, label) = match rest.split_once(" as ") {
                    Some((id, label)) => (id.trim(), clean_label(label)),
                    None => (rest, rest.to_string()),
                };
                let i = idx(&mut participants, id);
                participants[i].label = label;
                continue;
            }
            "autonumber" | "activate" | "deactivate" | "title" | "create" | "destroy" | "box" => {
                continue;
            }
            "loop" | "alt" | "opt" | "par" | "critical" | "break" | "rect" => {
                items.push(SeqItem::BlockStart {
                    kind: first.to_string(),
                    label: clean_label(line[first.len()..].trim()),
                });
                continue;
            }
            "else" | "and" | "option" => {
                items.push(SeqItem::BlockSplit {
                    label: clean_label(line[first.len()..].trim()),
                });
                continue;
            }
            "end" => {
                items.push(SeqItem::BlockEnd);
                continue;
            }
            _ => {}
        }
        if first.eq_ignore_ascii_case("note") {
            let rest = line[first.len()..].trim();
            let (place, text) = rest
                .split_once(':')
                .ok_or_else(|| format!("cannot read \"{line}\""))?;
            let place = place.trim();
            let (side, who) = if let Some(w) = place.strip_prefix("right of") {
                (NoteSide::Right, w)
            } else if let Some(w) = place.strip_prefix("left of") {
                (NoteSide::Left, w)
            } else if let Some(w) = place.strip_prefix("over") {
                (NoteSide::Over, w)
            } else {
                return Err(format!("cannot read \"{line}\""));
            };
            let mut ids = who.split(',');
            let a = idx(&mut participants, ids.next().unwrap_or(""));
            let b = ids.next().map(|x| idx(&mut participants, x)).unwrap_or(a);
            items.push(SeqItem::Note {
                side,
                a,
                b,
                text: clean_label(text),
            });
            continue;
        }
        let (head, text) = line.split_once(':').unwrap_or((line, ""));
        let found = ARROWS
            .iter()
            .filter_map(|(op, dashed, arrow)| head.find(op).map(|p| (p, *op, *dashed, *arrow)))
            .min_by_key(|(p, op, ..)| (*p, usize::MAX - op.len()));
        let Some((pos, op, dashed, arrow)) = found else {
            return Err(format!("cannot read \"{line}\""));
        };
        let from = idx(&mut participants, &head[..pos]);
        let to_raw = head[pos + op.len()..].trim_start_matches(['+', '-']);
        let to = idx(&mut participants, to_raw);
        items.push(SeqItem::Message {
            from,
            to,
            text: clean_label(text),
            dashed,
            arrow,
        });
    }
    if participants.is_empty() {
        return Err("the sequence diagram has no participants".into());
    }
    Ok(Sequence {
        participants,
        items,
    })
}

// ---------------------------------------------------------------------------
// Scene

#[derive(Debug, Clone, PartialEq)]
pub enum Prim {
    Shape {
        rect: Rect,
        shape: Shape,
        label: String,
    },
    /// A container (subgraph, sequence block, participant box).
    Frame {
        rect: Rect,
        title: String,
        filled: bool,
    },
    Note {
        rect: Rect,
        text: String,
    },
    Line {
        points: Vec<Pos2>,
        style: LineStyle,
        arrow: bool,
    },
    Text {
        pos: Pos2,
        text: String,
        /// Draw on a background patch (edge labels).
        boxed: bool,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Scene {
    pub size: Vec2,
    pub prims: Vec<Prim>,
}

const PAD: Vec2 = Vec2::new(14.0, 8.0);
const NODE_GAP: f32 = 28.0;
const RANK_GAP: f32 = 56.0;
const MARGIN: f32 = 12.0;
/// Widest a top-down rank gets before it wraps onto another row.
const WRAP_WIDTH: f32 = 760.0;

/// Place a diagram. `measure` returns the size of (possibly multi-line) text.
pub fn layout(d: &Diagram, measure: &dyn Fn(&str) -> Vec2) -> Scene {
    match d {
        Diagram::Flow(f) => layout_flow(f, measure),
        Diagram::Sequence(s) => layout_sequence(s, measure),
    }
}

fn node_size(n: &Node, measure: &dyn Fn(&str) -> Vec2) -> Vec2 {
    let t = measure(&n.label);
    let base = t + PAD * 2.0;
    match n.shape {
        Shape::Diamond => vec2(base.x * 1.4, base.y * 1.6),
        Shape::Circle => {
            let d = base.x.max(base.y);
            vec2(d, d)
        }
        Shape::Cylinder => vec2(base.x, base.y + 10.0),
        Shape::Hexagon => vec2(base.x + 20.0, base.y),
        _ => vec2(base.x.max(60.0), base.y),
    }
}

/// Nodes an end stands for, for ranking and ordering.
fn members(f: &Flowchart, e: End) -> Vec<usize> {
    match e {
        End::Node(i) => vec![i],
        End::Group(g) => f.groups[g].nodes.clone(),
    }
}

fn layout_flow(f: &Flowchart, measure: &dyn Fn(&str) -> Vec2) -> Scene {
    let n = f.nodes.len();
    // Ranks: longest path from the sources, edges into a cycle ignored.
    let pairs: Vec<(usize, usize)> = f
        .edges
        .iter()
        .flat_map(|e| {
            let to = members(f, e.to);
            members(f, e.from)
                .into_iter()
                .flat_map(move |a| to.clone().into_iter().map(move |b| (a, b)))
        })
        .filter(|(a, b)| a != b)
        .collect();
    let mut rank = vec![0usize; n];
    for _ in 0..n {
        let mut changed = false;
        for &(a, b) in &pairs {
            if rank[b] < rank[a] + 1 && rank[a] + 1 < n {
                rank[b] = rank[a] + 1;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    let ranks = rank.iter().copied().max().unwrap_or(0) + 1;
    let mut rows: Vec<Vec<usize>> = vec![Vec::new(); ranks];
    for i in 0..n {
        rows[rank[i]].push(i);
    }
    // Order within a rank by the mean position of neighbours (a few sweeps).
    for sweep in 0..4 {
        let pos: HashMap<usize, f32> = rows
            .iter()
            .flat_map(|r| r.iter().enumerate().map(|(i, &v)| (v, i as f32)))
            .collect();
        let down = sweep % 2 == 0;
        for (r, row) in rows.iter_mut().enumerate() {
            let key = |v: usize| -> f32 {
                let neigh: Vec<f32> = pairs
                    .iter()
                    .filter_map(|&(a, b)| {
                        if down && b == v && rank[a] < r {
                            pos.get(&a).copied()
                        } else if !down && a == v && rank[b] > r {
                            pos.get(&b).copied()
                        } else {
                            None
                        }
                    })
                    .collect();
                if neigh.is_empty() {
                    pos[&v]
                } else {
                    neigh.iter().sum::<f32>() / neigh.len() as f32
                }
            };
            let mut keyed: Vec<(f32, usize)> = row.iter().map(|&v| (key(v), v)).collect();
            keyed.sort_by(|x, y| x.0.total_cmp(&y.0));
            *row = keyed.into_iter().map(|(_, v)| v).collect();
        }
    }
    let sizes: Vec<Vec2> = f.nodes.iter().map(|nd| node_size(nd, measure)).collect();
    let horizontal = matches!(f.direction, Direction::LeftRight | Direction::RightLeft);
    // Along = the rank axis; across = within a rank.
    let along = |s: Vec2| if horizontal { s.x } else { s.y };
    let across = |s: Vec2| if horizontal { s.y } else { s.x };
    // Visual rows: a top-down rank wider than WRAP_WIDTH wraps onto more
    // rows (12 crates side by side would otherwise shrink to unreadable).
    let span = |r: &[usize]| {
        r.iter().map(|&v| across(sizes[v])).sum::<f32>()
            + NODE_GAP * r.len().saturating_sub(1) as f32
    };
    let mut visual: Vec<(usize, Vec<usize>)> = Vec::new();
    for (r, row) in rows.iter().enumerate() {
        let mut cur: Vec<usize> = Vec::new();
        for &v in row {
            cur.push(v);
            if !horizontal && cur.len() > 1 && span(&cur) > WRAP_WIDTH {
                let last = cur.pop().expect("non-empty");
                visual.push((r, std::mem::take(&mut cur)));
                cur.push(last);
            }
        }
        visual.push((r, cur));
    }
    let widest = visual.iter().map(|(_, v)| span(v)).fold(0.0, f32::max);
    // Room for edge labels between ranks.
    let label_room = f
        .edges
        .iter()
        .filter(|e| !e.label.is_empty())
        .map(|e| along(measure(&e.label)) + 8.0)
        .fold(0.0, f32::max);
    let gap = RANK_GAP.max(label_room + 24.0);
    let group_top = if f.groups.is_empty() { 0.0 } else { 24.0 };
    let mut centre = vec![Pos2::ZERO; n];
    let mut a = MARGIN + group_top;
    for (k, (r, row)) in visual.iter().enumerate() {
        let extent = row.iter().map(|&v| along(sizes[v])).fold(0.0, f32::max);
        let mut c = MARGIN + group_top + (widest - span(row)) / 2.0;
        for &v in row {
            let s = sizes[v];
            let mid_along = a + extent / 2.0;
            let mid_across = c + across(s) / 2.0;
            centre[v] = if horizontal {
                pos2(mid_along, mid_across)
            } else {
                pos2(mid_across, mid_along)
            };
            c += across(s) + NODE_GAP;
        }
        let same_rank_next = visual.get(k + 1).is_some_and(|(nr, _)| nr == r);
        a += extent + if same_rank_next { NODE_GAP / 2.0 } else { gap };
    }
    let total_along = a - gap + MARGIN;
    let total_across = widest + 2.0 * (MARGIN + group_top);
    let mut size = if horizontal {
        vec2(total_along, total_across)
    } else {
        vec2(total_across, total_along)
    };
    // Mirror for bottom-up / right-to-left.
    for c in &mut centre {
        match f.direction {
            Direction::BottomUp => c.y = size.y - c.y,
            Direction::RightLeft => c.x = size.x - c.x,
            _ => {}
        }
    }
    let rect_of = |v: usize| Rect::from_center_size(centre[v], sizes[v]);
    let mut prims = Vec::new();
    // Groups first so they sit underneath; nested (smaller) ones drawn later.
    let mut group_rects: Vec<Rect> = f
        .groups
        .iter()
        .map(|g| {
            let mut r = Rect::NOTHING;
            for &v in &g.nodes {
                r = r.union(rect_of(v));
            }
            if r == Rect::NOTHING {
                return Rect::from_min_size(pos2(MARGIN, MARGIN), vec2(80.0, 40.0));
            }
            let title = measure(&g.title);
            let mut r = r.expand2(vec2(10.0, 10.0));
            r.min.y -= title.y + 4.0;
            r.max.x = r.max.x.max(r.min.x + title.x + 16.0);
            r
        })
        .collect();
    // An outer group must enclose its nested groups' frames.
    for i in 0..f.groups.len() {
        for j in 0..f.groups.len() {
            if i != j
                && f.groups[j]
                    .nodes
                    .iter()
                    .all(|v| f.groups[i].nodes.contains(v))
                && f.groups[j].nodes.len() < f.groups[i].nodes.len()
            {
                group_rects[i] = group_rects[i].union(group_rects[j].expand(6.0));
            }
        }
    }
    let mut order: Vec<usize> = (0..f.groups.len()).collect();
    order.sort_by_key(|&g| std::cmp::Reverse(f.groups[g].nodes.len()));
    for g in order {
        prims.push(Prim::Frame {
            rect: group_rects[g],
            title: f.groups[g].title.clone(),
            filled: false,
        });
        size.x = size.x.max(group_rects[g].max.x + MARGIN);
        size.y = size.y.max(group_rects[g].max.y + MARGIN);
    }
    for e in &f.edges {
        let ra = match e.from {
            End::Node(v) => rect_of(v),
            End::Group(g) => group_rects[g],
        };
        let rb = match e.to {
            End::Node(v) => rect_of(v),
            End::Group(g) => group_rects[g],
        };
        let points = if e.from == e.to {
            let r = ra;
            vec![
                pos2(r.max.x, r.center().y - 6.0),
                pos2(r.max.x + 18.0, r.center().y - 6.0),
                pos2(r.max.x + 18.0, r.center().y + 6.0),
                pos2(r.max.x, r.center().y + 6.0),
            ]
        } else {
            vec![clip(ra, rb.center()), clip(rb, ra.center())]
        };
        if !e.label.is_empty() {
            let mid = if points.len() == 2 {
                points[0] + (points[1] - points[0]) / 2.0
            } else {
                points[1]
            };
            prims.push(Prim::Line {
                points,
                style: e.style,
                arrow: e.arrow,
            });
            prims.push(Prim::Text {
                pos: mid,
                text: e.label.clone(),
                boxed: true,
            });
        } else {
            prims.push(Prim::Line {
                points,
                style: e.style,
                arrow: e.arrow,
            });
        }
    }
    for (v, nd) in f.nodes.iter().enumerate() {
        prims.push(Prim::Shape {
            rect: rect_of(v),
            shape: nd.shape,
            label: nd.label.clone(),
        });
    }
    Scene { size, prims }
}

/// Where the segment from `r`'s centre towards `toward` leaves `r`.
fn clip(r: Rect, toward: Pos2) -> Pos2 {
    let c = r.center();
    let d = toward - c;
    if d.length_sq() < 1e-6 {
        return c;
    }
    let hx = r.width() / 2.0;
    let hy = r.height() / 2.0;
    let tx = if d.x.abs() > 1e-6 {
        hx / d.x.abs()
    } else {
        f32::INFINITY
    };
    let ty = if d.y.abs() > 1e-6 {
        hy / d.y.abs()
    } else {
        f32::INFINITY
    };
    c + d * tx.min(ty)
}

/// Greedy word wrap at `width` characters, keeping existing line breaks.
fn wrap(text: &str, width: usize) -> String {
    let mut out = String::new();
    for (i, line) in text.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let mut len = 0;
        for word in line.split(' ') {
            let w = word.chars().count();
            if len > 0 && len + 1 + w > width {
                out.push('\n');
                len = 0;
            } else if len > 0 {
                out.push(' ');
                len += 1;
            }
            out.push_str(word);
            len += w;
        }
    }
    out
}

/// Labels wrapped so one long message does not push the participants apart.
fn wrapped(s: &Sequence) -> Sequence {
    Sequence {
        participants: s
            .participants
            .iter()
            .map(|p| Participant {
                id: p.id.clone(),
                label: wrap(&p.label, 20),
            })
            .collect(),
        items: s
            .items
            .iter()
            .map(|it| match it {
                SeqItem::Message {
                    from,
                    to,
                    text,
                    dashed,
                    arrow,
                } => SeqItem::Message {
                    from: *from,
                    to: *to,
                    text: wrap(text, 30),
                    dashed: *dashed,
                    arrow: *arrow,
                },
                SeqItem::Note { side, a, b, text } => SeqItem::Note {
                    side: *side,
                    a: *a,
                    b: *b,
                    text: wrap(text, 30),
                },
                other => other.clone(),
            })
            .collect(),
    }
}

fn layout_sequence(s: &Sequence, measure: &dyn Fn(&str) -> Vec2) -> Scene {
    let s = &wrapped(s);
    let n = s.participants.len();
    let boxes: Vec<Vec2> = s
        .participants
        .iter()
        .map(|p| measure(&p.label) + PAD * 2.0)
        .map(|v| vec2(v.x.max(70.0), v.y))
        .collect();
    let box_h = boxes.iter().map(|b| b.y).fold(0.0, f32::max);
    // Centres: start from box widths, then widen gaps for message labels.
    let mut x = vec![0.0f32; n];
    let mut cur = MARGIN + boxes.first().map(|b| b.x / 2.0).unwrap_or(0.0);
    for i in 0..n {
        if i > 0 {
            cur += boxes[i - 1].x / 2.0 + 40.0 + boxes[i].x / 2.0;
        }
        x[i] = cur;
    }
    let widen = |x: &mut Vec<f32>, a: usize, b: usize, need: f32| {
        let (a, b) = (a.min(b), a.max(b));
        if a == b {
            return;
        }
        let have = x[b] - x[a];
        if have < need {
            for v in x.iter_mut().skip(b) {
                *v += need - have;
            }
        }
    };
    for it in &s.items {
        match it {
            SeqItem::Message { from, to, text, .. } => {
                let w = measure(text).x + 24.0;
                if from == to {
                    if *from + 1 < n {
                        widen(&mut x, *from, *from + 1, w + 30.0);
                    }
                } else {
                    widen(&mut x, *from, *to, w);
                }
            }
            SeqItem::Note { side, a, b, text } => {
                let w = measure(text).x + 20.0;
                match side {
                    NoteSide::Right if *a + 1 < n => widen(&mut x, *a, *a + 1, w + 30.0),
                    NoteSide::Left if *a > 0 => widen(&mut x, *a - 1, *a, w + 30.0),
                    NoteSide::Over if a != b => widen(&mut x, *a, *b, w - 20.0),
                    _ => {}
                }
            }
            _ => {}
        }
    }
    let mut prims = Vec::new();
    let mut y = MARGIN + box_h + 20.0;
    let left = MARGIN;
    let mut right =
        x.last().copied().unwrap_or(0.0) + boxes.last().map(|b| b.x / 2.0).unwrap_or(0.0);
    // Open blocks: top y, title, and the `else`/`and` splits (y, label).
    type Split = (f32, String);
    let mut blocks: Vec<(f32, String, Vec<Split>)> = Vec::new();
    let mut frames: Vec<Prim> = Vec::new();
    let mut notes: Vec<Prim> = Vec::new();
    for it in &s.items {
        match it {
            SeqItem::Message {
                from,
                to,
                text,
                dashed,
                arrow,
            } => {
                let t = measure(text);
                let style = if *dashed {
                    LineStyle::Dotted
                } else {
                    LineStyle::Solid
                };
                if from == to {
                    let x0 = x[*from];
                    if !text.is_empty() {
                        prims.push(Prim::Text {
                            pos: pos2(x0 + 30.0 + t.x / 2.0 + 4.0, y + t.y / 2.0),
                            text: text.clone(),
                            boxed: false,
                        });
                        right = right.max(x0 + 40.0 + t.x);
                    }
                    let ly = y + t.y.max(10.0);
                    prims.push(Prim::Line {
                        points: vec![
                            pos2(x0, ly),
                            pos2(x0 + 30.0, ly),
                            pos2(x0 + 30.0, ly + 16.0),
                            pos2(x0, ly + 16.0),
                        ],
                        style,
                        arrow: *arrow,
                    });
                    y = ly + 16.0 + 18.0;
                } else {
                    if !text.is_empty() {
                        prims.push(Prim::Text {
                            pos: pos2((x[*from] + x[*to]) / 2.0, y + t.y / 2.0),
                            text: text.clone(),
                            boxed: false,
                        });
                    }
                    let ly = y + t.y + 4.0;
                    prims.push(Prim::Line {
                        points: vec![pos2(x[*from], ly), pos2(x[*to], ly)],
                        style,
                        arrow: *arrow,
                    });
                    y = ly + 18.0;
                }
            }
            SeqItem::Note { side, a, b, text } => {
                let t = measure(text) + vec2(16.0, 10.0);
                let rect = match side {
                    NoteSide::Right => Rect::from_min_size(pos2(x[*a] + 10.0, y), t),
                    NoteSide::Left => Rect::from_min_size(pos2(x[*a] - 10.0 - t.x, y), t),
                    NoteSide::Over => {
                        let (l, r) = (x[*a].min(x[*b]), x[*a].max(x[*b]));
                        let w = t.x.max(r - l + 40.0);
                        Rect::from_min_size(pos2((l + r) / 2.0 - w / 2.0, y), vec2(w, t.y))
                    }
                };
                right = right.max(rect.max.x);
                notes.push(Prim::Note {
                    rect,
                    text: text.clone(),
                });
                y = rect.max.y + 12.0;
            }
            SeqItem::BlockStart { kind, label } => {
                let title = if label.is_empty() {
                    kind.clone()
                } else {
                    format!("{kind} [{label}]")
                };
                blocks.push((y, title, Vec::new()));
                y += measure("Ag").y + 14.0;
            }
            SeqItem::BlockSplit { label } => {
                if let Some(b) = blocks.last_mut() {
                    b.2.push((y, label.clone()));
                }
                y += measure("Ag").y + 12.0;
            }
            SeqItem::BlockEnd => {
                if let Some((top, title, splits)) = blocks.pop() {
                    let inset = 6.0 * blocks.len() as f32;
                    let rect = Rect::from_min_max(
                        pos2(left + inset, top),
                        pos2(right + 8.0 - inset, y + 4.0),
                    );
                    frames.push(Prim::Frame {
                        rect,
                        title,
                        filled: false,
                    });
                    for (sy, label) in splits {
                        frames.push(Prim::Line {
                            points: vec![pos2(rect.min.x, sy), pos2(rect.max.x, sy)],
                            style: LineStyle::Dotted,
                            arrow: false,
                        });
                        if !label.is_empty() {
                            frames.push(Prim::Text {
                                pos: pos2(rect.center().x, sy + measure(&label).y / 2.0 + 3.0),
                                text: format!("[{label}]"),
                                boxed: false,
                            });
                        }
                    }
                    y += 14.0;
                }
            }
        }
    }
    let bottom_top = y + 8.0;
    let mut out = Vec::new();
    // Lifelines behind everything.
    for &xi in &x {
        out.push(Prim::Line {
            points: vec![pos2(xi, MARGIN + box_h), pos2(xi, bottom_top)],
            style: LineStyle::Dotted,
            arrow: false,
        });
    }
    out.extend(frames);
    for (i, p) in s.participants.iter().enumerate() {
        for top in [MARGIN, bottom_top] {
            out.push(Prim::Shape {
                rect: Rect::from_min_size(
                    pos2(x[i] - boxes[i].x / 2.0, top),
                    vec2(boxes[i].x, box_h),
                ),
                shape: Shape::Rect,
                label: p.label.clone(),
            });
        }
    }
    out.extend(prims);
    out.extend(notes);
    Scene {
        size: vec2(right + MARGIN + 8.0, bottom_top + box_h + MARGIN),
        prims: out,
    }
}

// ---------------------------------------------------------------------------
// Painting

pub struct Palette {
    pub text: Color32,
    pub line: Color32,
    pub fill: Color32,
    pub frame: Color32,
    pub note: Color32,
    pub label_bg: Color32,
}

/// Draw `scene` with its top-left at `origin`.
pub fn paint(
    painter: &egui::Painter,
    origin: Pos2,
    scene: &Scene,
    font: &egui::FontId,
    pal: &Palette,
) {
    let at = |p: Pos2| origin + p.to_vec2();
    let text = |pos: Pos2, s: &str, colour: Color32| {
        let galley = painter.layout(s.to_string(), font.clone(), colour, f32::INFINITY);
        let size = galley.size();
        // Centre each line: re-layout per line keeps multi-line labels tidy.
        if s.contains('\n') {
            let lines: Vec<&str> = s.lines().collect();
            let lh = size.y / lines.len() as f32;
            for (i, l) in lines.iter().enumerate() {
                let g = painter.layout_no_wrap(l.to_string(), font.clone(), colour);
                let p = pos2(
                    pos.x - g.size().x / 2.0,
                    pos.y - size.y / 2.0 + i as f32 * lh,
                );
                painter.galley(p, g, colour);
            }
        } else {
            painter.galley(pos - size / 2.0, galley, colour);
        }
        size
    };
    let stroke = egui::Stroke::new(1.2, pal.line);
    for prim in &scene.prims {
        match prim {
            Prim::Frame {
                rect,
                title,
                filled,
            } => {
                let r = rect.translate(origin.to_vec2());
                if *filled {
                    painter.rect_filled(r, 4.0, pal.fill);
                }
                painter.rect_stroke(
                    r,
                    4.0,
                    egui::Stroke::new(1.0, pal.frame),
                    egui::StrokeKind::Inside,
                );
                if !title.is_empty() {
                    let g = painter.layout_no_wrap(title.clone(), font.clone(), pal.text);
                    let tag = Rect::from_min_size(r.min, g.size() + vec2(10.0, 4.0));
                    painter.rect_filled(tag, 3.0, pal.label_bg);
                    painter.galley(tag.min + vec2(5.0, 2.0), g, pal.text);
                }
            }
            Prim::Note { rect, text: t } => {
                let r = rect.translate(origin.to_vec2());
                painter.rect_filled(r, 2.0, pal.note);
                painter.rect_stroke(
                    r,
                    2.0,
                    egui::Stroke::new(1.0, pal.frame),
                    egui::StrokeKind::Inside,
                );
                text(r.center(), t, pal.text);
            }
            Prim::Shape { rect, shape, label } => {
                let r = rect.translate(origin.to_vec2());
                draw_shape(painter, r, *shape, pal);
                text(r.center(), label, pal.text);
            }
            Prim::Line {
                points,
                style,
                arrow,
            } => {
                let pts: Vec<Pos2> = points.iter().map(|&p| at(p)).collect();
                let s = match style {
                    LineStyle::Thick => egui::Stroke::new(2.4, pal.line),
                    _ => stroke,
                };
                for w in pts.windows(2) {
                    if *style == LineStyle::Dotted {
                        painter.add(egui::Shape::dashed_line(&[w[0], w[1]], s, 4.0, 3.0));
                    } else {
                        painter.line_segment([w[0], w[1]], s);
                    }
                }
                if *arrow && pts.len() >= 2 {
                    let tip = pts[pts.len() - 1];
                    let dir = (tip - pts[pts.len() - 2]).normalized();
                    let side = vec2(-dir.y, dir.x);
                    let back = tip - dir * 8.0;
                    painter.add(egui::Shape::convex_polygon(
                        vec![tip, back + side * 4.0, back - side * 4.0],
                        pal.line,
                        egui::Stroke::NONE,
                    ));
                }
            }
            Prim::Text {
                pos,
                text: t,
                boxed,
            } => {
                let p = at(*pos);
                if *boxed {
                    let g = painter.layout_no_wrap(t.clone(), font.clone(), pal.text);
                    let r = Rect::from_center_size(p, g.size() + vec2(8.0, 4.0));
                    painter.rect_filled(r, 3.0, pal.label_bg);
                }
                text(p, t, pal.text);
            }
        }
    }
}

fn draw_shape(painter: &egui::Painter, r: Rect, shape: Shape, pal: &Palette) {
    let stroke = egui::Stroke::new(1.2, pal.frame);
    match shape {
        Shape::Rect => {
            painter.rect_filled(r, 2.0, pal.fill);
            painter.rect_stroke(r, 2.0, stroke, egui::StrokeKind::Inside);
        }
        Shape::Round => {
            painter.rect_filled(r, 8.0, pal.fill);
            painter.rect_stroke(r, 8.0, stroke, egui::StrokeKind::Inside);
        }
        Shape::Stadium => {
            let rad = (r.height() / 2.0) as u8;
            painter.rect_filled(r, rad, pal.fill);
            painter.rect_stroke(r, rad, stroke, egui::StrokeKind::Inside);
        }
        Shape::Circle => {
            painter.circle(
                r.center(),
                r.width().min(r.height()) / 2.0,
                pal.fill,
                stroke,
            );
        }
        Shape::Diamond => {
            let c = r.center();
            let pts = vec![
                pos2(c.x, r.min.y),
                pos2(r.max.x, c.y),
                pos2(c.x, r.max.y),
                pos2(r.min.x, c.y),
            ];
            painter.add(egui::Shape::convex_polygon(pts, pal.fill, stroke));
        }
        Shape::Hexagon => {
            let c = r.center();
            let k = 10.0;
            let pts = vec![
                pos2(r.min.x + k, r.min.y),
                pos2(r.max.x - k, r.min.y),
                pos2(r.max.x, c.y),
                pos2(r.max.x - k, r.max.y),
                pos2(r.min.x + k, r.max.y),
                pos2(r.min.x, c.y),
            ];
            painter.add(egui::Shape::convex_polygon(pts, pal.fill, stroke));
        }
        Shape::Cylinder => {
            painter.rect_filled(r, 6.0, pal.fill);
            painter.rect_stroke(r, 6.0, stroke, egui::StrokeKind::Inside);
            let top = Rect::from_min_size(r.min, vec2(r.width(), 10.0));
            painter.add(egui::Shape::ellipse_stroke(
                top.center(),
                top.size() / 2.0,
                stroke,
            ));
        }
    }
}

/// Parse, lay out and draw `src` into `ui`, shrinking it to the width
/// available. Returns the parse error so the caller can show the source.
pub fn show(ui: &mut egui::Ui, src: &str, base_size: f32, pal: &Palette) -> Result<(), String> {
    let diagram = parse(src)?;
    let size = base_size - 1.0;
    let font = egui::FontId::proportional(size);
    let painter = ui.painter().clone();
    let measure = |t: &str| {
        painter
            .layout(t.to_string(), font.clone(), pal.text, f32::INFINITY)
            .size()
    };
    let mut scene = layout(&diagram, &measure);
    // Too wide: shrink geometry and text together, not below a readable
    // size; anything still wider scrolls.
    let avail = ui.available_width().max(100.0);
    let mut font = font.clone();
    if scene.size.x > avail {
        let k = (avail / scene.size.x).max(MIN_SCALE);
        scene = scene.scaled(k);
        font = egui::FontId::proportional(size * k);
    }
    let draw = |ui: &mut egui::Ui| {
        let (rect, _) =
            ui.allocate_exact_size(scene.size.max(vec2(avail, 0.0)), egui::Sense::hover());
        let x = rect.min.x + ((avail - scene.size.x) / 2.0).max(0.0);
        paint(
            &ui.painter_at(rect),
            pos2(x, rect.min.y),
            &scene,
            &font,
            pal,
        );
    };
    if scene.size.x > avail {
        // Still too wide at the smallest scale: scroll sideways.
        egui::ScrollArea::horizontal()
            .id_salt(("mermaid", src))
            .show(ui, draw);
    } else {
        draw(ui);
    }
    Ok(())
}

/// The smallest a diagram is shrunk to fit (≈ 7 px text at the default size).
const MIN_SCALE: f32 = 0.55;

impl Scene {
    /// The same scene with every coordinate multiplied by `k`.
    pub fn scaled(&self, k: f32) -> Scene {
        let p = |v: Pos2| pos2(v.x * k, v.y * k);
        let r = |v: Rect| Rect::from_min_max(p(v.min), p(v.max));
        Scene {
            size: self.size * k,
            prims: self
                .prims
                .iter()
                .map(|prim| match prim.clone() {
                    Prim::Shape { rect, shape, label } => Prim::Shape {
                        rect: r(rect),
                        shape,
                        label,
                    },
                    Prim::Frame {
                        rect,
                        title,
                        filled,
                    } => Prim::Frame {
                        rect: r(rect),
                        title,
                        filled,
                    },
                    Prim::Note { rect, text } => Prim::Note {
                        rect: r(rect),
                        text,
                    },
                    Prim::Line {
                        points,
                        style,
                        arrow,
                    } => Prim::Line {
                        points: points.into_iter().map(p).collect(),
                        style,
                        arrow,
                    },
                    Prim::Text { pos, text, boxed } => Prim::Text {
                        pos: p(pos),
                        text,
                        boxed,
                    },
                })
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn measure(t: &str) -> Vec2 {
        let lines: Vec<&str> = t.split('\n').collect();
        let w = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0);
        vec2(w as f32 * 7.0, lines.len() as f32 * 14.0)
    }

    fn flow(src: &str) -> Flowchart {
        match parse(src).unwrap() {
            Diagram::Flow(f) => f,
            other => panic!("not a flowchart: {other:?}"),
        }
    }

    fn label(f: &Flowchart, i: usize) -> &str {
        &f.nodes[i].label
    }

    #[test]
    fn flowchart_nodes_shapes_and_edges() {
        let f = flow(
            "flowchart LR\n  A[Start] --> B{Ok?}\n  B -->|yes| C([Done])\n  B -- no --> D((Retry))\n  D -.-> A\n  C ==> E[(DB)]\n  %% comment\n",
        );
        assert_eq!(f.direction, Direction::LeftRight);
        assert_eq!(f.nodes.len(), 5);
        assert_eq!(f.nodes[1].shape, Shape::Diamond);
        assert_eq!(f.nodes[2].shape, Shape::Stadium);
        assert_eq!(f.nodes[3].shape, Shape::Circle);
        assert_eq!(f.nodes[4].shape, Shape::Cylinder);
        assert_eq!(label(&f, 1), "Ok?");
        assert_eq!(f.edges.len(), 5);
        assert_eq!(f.edges[1].label, "yes");
        assert_eq!(f.edges[2].label, "no");
        assert!(f.edges[2].arrow);
        assert_eq!(f.edges[3].style, LineStyle::Dotted);
        assert_eq!(f.edges[4].style, LineStyle::Thick);
    }

    #[test]
    fn chains_ampersands_semicolons_and_quotes() {
        let f = flow("graph TD\n  A --> B --> C; D & E --> F\n  G[\"a [b] c\"] --- H");
        assert_eq!(f.edges.len(), 2 + 2 + 1);
        assert!(f.edges.iter().all(|e| e.arrow || e.label.is_empty()));
        assert!(!f.edges[4].arrow, "--- has no arrow");
        let g = f.nodes.iter().find(|n| n.id == "G").unwrap();
        assert_eq!(g.label, "a [b] c");
    }

    #[test]
    fn the_architecture_diagram_parses_with_subgraphs_and_group_edges() {
        let src = r#"flowchart TB
    subgraph bin["hyperstack (binary)"]
        Shell["App shell<br/>window · tab space"]
    end
    subgraph tools["tools/* (one crate each)"]
        Ed[tool-editor] ; Db[tool-db] ; Ssh[tool-ssh]
    end
    subgraph shared["shared crates"]
        UI["hs-ui"]
        Core["hs-core"]
    end
    Shell --> tools
    tools --> UI
    UI --> Core
    Ssh --> Core
"#;
        let f = flow(src);
        assert_eq!(f.groups.len(), 3);
        assert_eq!(f.groups[1].title, "tools/* (one crate each)");
        assert_eq!(f.groups[1].nodes.len(), 3);
        assert_eq!(label(&f, 0), "App shell\nwindow · tab space");
        assert_eq!(f.edges[0].to, End::Group(1));
        let scene = layout(&Diagram::Flow(f.clone()), &measure);
        // Ranks follow the edges: Shell above the tools, the tools above UI, UI above Core.
        let centre = |id: &str| {
            let i = f.nodes.iter().position(|n| n.id == id).unwrap();
            scene
                .prims
                .iter()
                .filter_map(|p| match p {
                    Prim::Shape { rect, label, .. } if *label == f.nodes[i].label => {
                        Some(rect.center())
                    }
                    _ => None,
                })
                .next()
                .unwrap()
        };
        assert!(centre("Shell").y < centre("Ed").y);
        assert!(centre("Ed").y < centre("UI").y);
        assert!(centre("UI").y < centre("Core").y);
    }

    #[test]
    fn wide_ranks_wrap_onto_more_rows() {
        let mut src = String::from("flowchart TB\n");
        for i in 0..12 {
            src.push_str(&format!("  R --> N{i}[a fairly long node {i}]\n"));
        }
        let f = flow(&src);
        let scene = layout(&Diagram::Flow(f), &measure);
        assert!(
            scene.size.x <= WRAP_WIDTH + 2.0 * MARGIN + 1.0,
            "{}",
            scene.size.x
        );
        let rows: std::collections::BTreeSet<i32> = scene
            .prims
            .iter()
            .filter_map(|p| match p {
                Prim::Shape { rect, label, .. } if label != "R" => Some(rect.center().y as i32),
                _ => None,
            })
            .collect();
        assert!(rows.len() >= 2, "the twelve children wrap: {rows:?}");
    }

    #[test]
    fn flow_layout_never_overlaps_nodes() {
        let f = flow("flowchart TD\n A-->B\n A-->C\n A-->D\n B-->E\n C-->E\n D-->E\n E-->A");
        let scene = layout(&Diagram::Flow(f), &measure);
        let rects: Vec<Rect> = scene
            .prims
            .iter()
            .filter_map(|p| match p {
                Prim::Shape { rect, .. } => Some(*rect),
                _ => None,
            })
            .collect();
        assert_eq!(rects.len(), 5);
        for (i, a) in rects.iter().enumerate() {
            assert!(scene.size.x >= a.max.x && scene.size.y >= a.max.y);
            for b in &rects[i + 1..] {
                assert!(!a.intersects(*b), "{a:?} overlaps {b:?}");
            }
        }
    }

    #[test]
    fn sequence_messages_notes_and_blocks() {
        let src = r#"sequenceDiagram
    participant UI as UI thread
    participant Rt as tokio runtime
    actor User
    User->>UI: click
    UI->>Rt: spawn
    Rt-->>UI: result
    UI->>UI: redraw
    Note right of Rt: off the UI thread
    Note over UI,Rt: bounded channel
    loop every frame
        UI-xRt: poll
    alt ok
        Rt--)UI: done
    else failed
        Rt-->UI: error
    end
    end
"#;
        let Diagram::Sequence(s) = parse(src).unwrap() else {
            panic!("not a sequence");
        };
        assert_eq!(s.participants.len(), 3);
        assert_eq!(s.participants[0].label, "UI thread");
        assert_eq!(s.participants[2].id, "User");
        let msgs: Vec<_> = s
            .items
            .iter()
            .filter_map(|i| match i {
                SeqItem::Message {
                    from,
                    to,
                    dashed,
                    arrow,
                    ..
                } => Some((*from, *to, *dashed, *arrow)),
                _ => None,
            })
            .collect();
        assert_eq!(msgs[0], (2, 0, false, true));
        assert_eq!(msgs[2], (1, 0, true, true), "-->> is dashed with an arrow");
        assert_eq!(msgs[3], (0, 0, false, true), "self message");
        assert_eq!(msgs[6], (1, 0, true, false), "--> is dashed without a head");
        let scene = layout(&Diagram::Sequence(s), &measure);
        let frames = scene
            .prims
            .iter()
            .filter(|p| matches!(p, Prim::Frame { .. }))
            .count();
        assert_eq!(frames, 2, "loop and alt blocks");
        let notes = scene
            .prims
            .iter()
            .filter(|p| matches!(p, Prim::Note { .. }))
            .count();
        assert_eq!(notes, 2);
        // Every participant is drawn at the top and the bottom.
        let boxes = scene
            .prims
            .iter()
            .filter(|p| matches!(p, Prim::Shape { .. }))
            .count();
        assert_eq!(boxes, 6);
    }

    #[test]
    fn wrap_breaks_on_words_and_keeps_newlines() {
        assert_eq!(
            wrap("spawn task, hand it a JobHandle", 12),
            "spawn task,\nhand it a\nJobHandle"
        );
        assert_eq!(wrap("a\nb c", 10), "a\nb c");
        assert_eq!(wrap("averyveryverylongword", 5), "averyveryverylongword");
    }

    #[test]
    fn unsupported_and_broken_sources_are_errors() {
        assert!(parse("gantt\n title x").unwrap_err().contains("gantt"));
        assert!(parse("").is_err());
        assert!(parse("flowchart XY\n A-->B").is_err());
        assert!(parse("sequenceDiagram\n hello there").is_err());
    }

    #[test]
    fn long_labels_widen_the_gap_between_participants() {
        let Diagram::Sequence(s) =
            parse("sequenceDiagram\n A->>B: a very very very long message label here").unwrap()
        else {
            panic!()
        };
        let scene = layout(&Diagram::Sequence(s), &measure);
        assert!(scene.size.x > measure("a very very very long message label here").x);
    }
}
