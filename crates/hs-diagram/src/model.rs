//! The editor-native `.hdiag` document.

use serde::{Deserialize, Serialize};

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DiagramKind {
    #[default]
    Flow,
    Architecture,
    Erd,
    Sequence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PageSize {
    #[default]
    Fit,
    A4,
    Letter,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Page {
    #[serde(default)]
    pub size: PageSize,
    #[serde(default = "default_margin")]
    pub margin: f32,
    #[serde(default = "default_background")]
    pub background: String,
}

fn default_margin() -> f32 {
    24.0
}

fn default_background() -> String {
    "#ffffff".into()
}

impl Default for Page {
    fn default() -> Self {
        Self {
            size: PageSize::Fit,
            margin: default_margin(),
            background: default_background(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    #[default]
    Process,
    RoundedRectangle,
    Ellipse,
    Decision,
    StartEnd,
    InputOutput,
    Database,
    Note,
    Text,
    Container,
    Frontend,
    Backend,
    Store,
    Queue,
    External,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum EdgeRoute {
    Straight,
    #[default]
    Orthogonal,
    Curved,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Arrow {
    None,
    #[default]
    End,
    Both,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Style {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_size: Option<f32>,
    #[serde(default)]
    pub bold: bool,
    #[serde(default)]
    pub italic: bool,
    #[serde(default)]
    pub dashed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub radius: Option<f32>,
}

impl Style {
    pub fn is_empty(&self) -> bool {
        *self == Style::default()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Node {
    pub id: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub kind: NodeKind,
    #[serde(default)]
    pub x: f32,
    #[serde(default)]
    pub y: f32,
    #[serde(default)]
    pub w: f32,
    #[serde(default)]
    pub h: f32,
    #[serde(default)]
    pub style: Style,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Edge {
    pub id: String,
    pub from: String,
    pub to: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub route: EdgeRoute,
    #[serde(default)]
    pub arrow: Arrow,
    #[serde(default)]
    pub dashed: bool,
    #[serde(default)]
    pub via: Vec<Point>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub struct Point {
    #[serde(default)]
    pub x: f32,
    #[serde(default)]
    pub y: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Group {
    pub id: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub nodes: Vec<String>,
    #[serde(default)]
    pub style: Style,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Meta {
    #[serde(default)]
    pub created: String,
    #[serde(default)]
    pub modified: String,
    #[serde(default = "default_app")]
    pub app: String,
}

fn default_app() -> String {
    "HyperStack".into()
}

impl Default for Meta {
    fn default() -> Self {
        Self {
            created: String::new(),
            modified: String::new(),
            app: default_app(),
        }
    }
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiagramDoc {
    #[serde(default = "default_schema")]
    pub schema: u32,
    #[serde(default)]
    pub kind: DiagramKind,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub subtitle: String,
    #[serde(default)]
    pub page: Page,
    #[serde(default)]
    pub nodes: Vec<Node>,
    #[serde(default)]
    pub edges: Vec<Edge>,
    #[serde(default)]
    pub groups: Vec<Group>,
    #[serde(default)]
    pub meta: Meta,
    /// `true` when the nodes have no meaningful geometry and the generator
    /// should place them; the editor saves `false`.
    #[serde(default = "default_true")]
    pub auto_layout: bool,
}

fn default_schema() -> u32 {
    SCHEMA_VERSION
}

impl DiagramDoc {
    pub fn new(kind: DiagramKind, title: impl Into<String>) -> Self {
        Self {
            schema: SCHEMA_VERSION,
            kind,
            title: title.into(),
            subtitle: String::new(),
            page: Page::default(),
            nodes: Vec::new(),
            edges: Vec::new(),
            groups: Vec::new(),
            meta: Meta::default(),
            auto_layout: true,
        }
    }

    pub fn node(&self, id: &str) -> Option<&Node> {
        self.nodes.iter().find(|n| n.id == id)
    }

    pub fn node_mut(&mut self, id: &str) -> Option<&mut Node> {
        self.nodes.iter_mut().find(|n| n.id == id)
    }
}
