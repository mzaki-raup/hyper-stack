//! Diagram Editor: a direct-manipulation canvas over `hs-diagram`.
//!
//! Release 1 deliberately keeps one scene: the canvas paints the same
//! `hs_diagram::Scene` that SVG, PNG and PDF export, so what the user moves is
//! what the file contains.

use std::path::{Path, PathBuf};

use hs_core::Services;
use hs_core::tool::{ToastLevel, ToolContext};
use hs_diagram::export::{PngOptions, pdf, png, svg};
use hs_diagram::model::{
    Arrow, DiagramDoc, DiagramKind, Edge, EdgeRoute, Group, Node, NodeKind, Point, Style,
};
use hs_diagram::{Prim, Rect, Scene, layout_approx, validate};
use hs_ui::form;
use hs_ui::theme::Theme;
use hs_ui::tip::TipExt;

/// Frame id, press position and the nodes captured for a frame drag.
type FrameDrag = (String, egui::Pos2, Vec<(String, Point)>);
/// Frame id, start rect, node geometry (id, x, y, w, h) and press position.
type FrameResize = (
    String,
    egui::Rect,
    Vec<(String, f32, f32, f32, f32)>,
    egui::Pos2,
);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AlignMode {
    Left,
    CentreX,
    Right,
    Top,
    MiddleY,
    Bottom,
    DistributeX,
    DistributeY,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ImportKind {
    Archify,
    DrawIo,
    Mermaid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum DiagramView {
    #[default]
    Preview,
    Source,
    Split,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolMode {
    Select,
    Edge,
}

pub struct DiagramState {
    pub doc: DiagramDoc,
    pub path: Option<PathBuf>,
    pub dirty: bool,
    undo: Vec<DiagramDoc>,
    redo: Vec<DiagramDoc>,
    undo_limit: usize,
    selected: Option<String>,
    /// Additional nodes selected with Shift-click; the primary is `selected`.
    multi: Vec<String>,
    selected_edge: Option<String>,
    selected_group: Option<String>,
    /// World-space polyline of every edge from the last painted scene.
    edge_paths: Vec<Vec<Point>>,
    view: DiagramView,
    zoom: f32,
    pan: egui::Vec2,
    show_grid: bool,
    tool: ToolMode,
    edge_from: Option<String>,
    drag_node: Option<(String, Point, egui::Pos2)>,
    drag_resize: Option<(String, f32, f32, egui::Pos2)>,
    drag_via: Option<(String, usize, Point, egui::Pos2)>,
    drag_frame: Option<FrameDrag>,
    drag_frame_resize: Option<FrameResize>,
    drag_pan: Option<(egui::Vec2, egui::Pos2)>,
    last_center: Point,
    scene_cache: Option<(u64, std::sync::Arc<Scene>)>,
    saved: Option<PathBuf>,
    pub(crate) status: Option<String>,
    save_open: bool,
    save_project: usize,
    save_subfolder: String,
    save_name: String,
    save_overwrite: bool,
    save_error: Option<String>,
    drawio_import: Option<(String, Vec<hs_diagram::import::DrawioPage>, usize)>,
}

impl DiagramState {
    pub fn new(kind: DiagramKind) -> Self {
        let doc = match kind {
            DiagramKind::Flow => hs_diagram::template::flow(),
            DiagramKind::Architecture => hs_diagram::template::architecture(),
            DiagramKind::Erd => hs_diagram::template::erd(),
            DiagramKind::Sequence => hs_diagram::template::sequence(),
        };
        Self::from_doc(doc)
    }

    pub fn blank() -> Self {
        Self::from_doc(DiagramDoc::new(DiagramKind::Flow, "Untitled diagram"))
    }

    pub fn from_doc(doc: DiagramDoc) -> Self {
        let selected = doc.nodes.first().map(|n| n.id.clone());
        let mut state = Self {
            doc,
            path: None,
            dirty: false,
            undo: Vec::new(),
            redo: Vec::new(),
            undo_limit: 20,
            selected,
            multi: Vec::new(),
            selected_edge: None,
            selected_group: None,
            edge_paths: Vec::new(),
            view: DiagramView::Preview,
            zoom: 0.0,
            pan: egui::Vec2::ZERO,
            show_grid: true,
            tool: ToolMode::Select,
            edge_from: None,
            drag_node: None,
            drag_resize: None,
            drag_via: None,
            drag_frame: None,
            drag_frame_resize: None,
            drag_pan: None,
            last_center: Point { x: 0.0, y: 0.0 },
            scene_cache: None,
            saved: None,
            status: None,
            save_open: false,
            save_project: 0,
            save_subfolder: "docs/diagrams".into(),
            save_name: String::new(),
            save_overwrite: false,
            save_error: None,
            drawio_import: None,
        };
        state.dirty = false;
        state
    }

    pub fn open_path(path: &Path) -> Result<Self, String> {
        let (doc, issues) = hs_diagram::io::load(path)?;
        let errors: Vec<_> = issues
            .iter()
            .filter(|i| i.severity == hs_diagram::Severity::Error)
            .collect();
        if !errors.is_empty() {
            return Err(format!(
                "the diagram has {} validation error(s), first: {}",
                errors.len(),
                errors[0].message
            ));
        }
        let mut state = Self::from_doc(doc);
        state.path = Some(path.to_path_buf());
        Ok(state)
    }

    pub fn dirty(&self) -> bool {
        self.dirty
    }

    pub fn take_saved(&mut self) -> Option<PathBuf> {
        self.saved.take()
    }

    /// Freeze the automatic layout into node geometry the first time the user
    /// edits it, so the document is the editor's from then on.
    fn freeze(&mut self) -> bool {
        if !self.doc.auto_layout {
            return false;
        }
        let scene = layout_approx(&self.doc);
        let rects: Vec<Rect> = scene
            .prims
            .iter()
            .filter_map(|p| match p {
                Prim::Shape { rect, .. } => Some(*rect),
                _ => None,
            })
            .collect();
        if rects.len() != self.doc.nodes.len() {
            return false;
        }
        for (node, r) in self.doc.nodes.iter_mut().zip(rects) {
            node.x = r.x;
            node.y = r.y;
            node.w = r.w;
            node.h = r.h;
        }
        self.doc.auto_layout = false;
        true
    }

    fn add_edge_waypoint(&mut self, id: &str) {
        let Some(edge) = self.doc.edges.iter().find(|e| e.id == id) else {
            return;
        };
        let from = self.doc.nodes.iter().find(|n| n.id == edge.from);
        let to = self.doc.nodes.iter().find(|n| n.id == edge.to);
        let (Some(from), Some(to)) = (from, to) else {
            return;
        };
        let ro = |n: &Node| Rect {
            x: n.x,
            y: n.y,
            w: n.w,
            h: n.h,
        };
        let index = self.doc.edges.iter().position(|e| e.id == id);
        let mid = index
            .and_then(|i| self.edge_paths.get(i))
            .and_then(|p| p.get(p.len() / 2))
            .copied()
            .unwrap_or_else(|| {
                hs_diagram::route::route(ro(from), ro(to), edge.route, &edge.via)
                    .first()
                    .copied()
                    .unwrap_or(Point {
                        x: from.x + from.w / 2.0,
                        y: from.y + from.h / 2.0,
                    })
            });
        self.push_undo();
        if let Some(edge) = self.doc.edges.iter_mut().find(|e| e.id == id) {
            edge.via.push(mid);
        }
        self.dirty = true;
    }

    /// One rect per document node, in node order. The layout pushes the
    /// node shapes first; a sequence diagram then adds one activation
    /// shape per message, which must not be mistaken for a node (indexing
    /// `doc.nodes` with such an index panicked on click).
    fn node_rects(&self, scene: &Scene) -> Vec<Rect> {
        scene
            .prims
            .iter()
            .filter_map(|p| match p {
                Prim::Shape { rect, .. } => Some(*rect),
                _ => None,
            })
            .take(self.doc.nodes.len())
            .collect()
    }

    /// The laid-out scene for the current document, recomputed only when
    /// the document changed: idle repaints (hover, pan) reuse it.
    fn scene_cached(&mut self) -> std::sync::Arc<Scene> {
        let key = fingerprint(&self.doc);
        if let Some((k, scene)) = &self.scene_cache
            && *k == key
        {
            return scene.clone();
        }
        let scene = std::sync::Arc::new(layout_approx(&self.doc));
        self.scene_cache = Some((key, scene.clone()));
        scene
    }

    fn push_undo(&mut self) {
        self.undo.push(self.doc.clone());
        let limit = self.undo_limit.clamp(1, 100);
        while self.undo.len() > limit {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.dirty = true;
    }

    /// Drop every selection, pending edge start and drag that refers to
    /// ids of a document that just got swapped out (undo, redo, replace).
    fn clear_selection(&mut self) {
        if self
            .selected
            .as_ref()
            .is_some_and(|id| self.doc.node(id).is_none())
        {
            self.selected = None;
        }
        self.multi.retain(|id| self.doc.node(id).is_some());
        if self
            .selected_edge
            .as_ref()
            .is_some_and(|id| !self.doc.edges.iter().any(|e| &e.id == id))
        {
            self.selected_edge = None;
        }
        if self
            .selected_group
            .as_ref()
            .is_some_and(|id| !self.doc.groups.iter().any(|g| &g.id == id))
        {
            self.selected_group = None;
        }
        self.edge_from = None;
        self.drag_node = None;
        self.drag_resize = None;
        self.drag_via = None;
        self.drag_frame = None;
        self.drag_frame_resize = None;
    }

    fn undo(&mut self) {
        if let Some(doc) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.doc, doc));
            self.dirty = true;
            self.clear_selection();
        }
    }

    fn redo(&mut self) {
        if let Some(doc) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.doc, doc));
            self.dirty = true;
            self.clear_selection();
        }
    }

    fn add_node_at(&mut self, kind: NodeKind, at: Point) {
        self.push_undo();
        self.freeze();
        let mut n = self.doc.nodes.len() + 1;
        while self.doc.nodes.iter().any(|x| x.id == format!("node{n}")) {
            n += 1;
        }
        let size = match kind {
            NodeKind::Decision => (150.0, 90.0),
            NodeKind::Note => (170.0, 90.0),
            NodeKind::Entity => (220.0, 160.0),
            _ => (160.0, 64.0),
        };
        let node = Node {
            id: format!("node{n}"),
            label: if kind == NodeKind::Entity {
                format!("entity{n}\nid UUID PK")
            } else {
                format!("Node {n}")
            },
            kind,
            x: at.x - size.0 / 2.0,
            y: at.y - size.1 / 2.0,
            w: size.0,
            h: size.1,
            style: Style::default(),
            erd: if kind == NodeKind::Entity {
                Some(hs_diagram::model::Entity {
                    columns: vec![hs_diagram::model::Column {
                        name: "id".into(),
                        data_type: "UUID".into(),
                        key: hs_diagram::model::ColumnKey::Primary,
                    }],
                })
            } else {
                None
            },
        };
        self.selected = Some(node.id.clone());
        let id = node.id.clone();
        self.doc.nodes.push(node);
        if let Some(gid) = &self.selected_group
            && let Some(group) = self.doc.groups.iter_mut().find(|g| &g.id == gid)
        {
            group.nodes.push(id);
        }
        self.tool = ToolMode::Select;
    }

    fn add_frame(&mut self) {
        self.push_undo();
        self.freeze();
        let ids: Vec<String> = if self.multi.is_empty() {
            match &self.selected {
                Some(id) => vec![id.clone()],
                None => self.doc.nodes.iter().map(|n| n.id.clone()).collect(),
            }
        } else {
            self.multi.clone()
        };
        if ids.is_empty() {
            return;
        }
        let mut n = self.doc.groups.len() + 1;
        while self
            .doc
            .groups
            .iter()
            .any(|g| g.label == format!("Frame {n}"))
        {
            n += 1;
        }
        let id = format!("frame{n}");
        self.doc.groups.push(Group {
            id: id.clone(),
            label: format!("Frame {n}"),
            nodes: ids,
            style: Style::default(),
            label_placement: Default::default(),
            label_align: Default::default(),
        });
        self.selected_group = Some(id);
        self.selected = None;
        self.selected_edge = None;
        self.multi.clear();
        self.dirty = true;
    }

    fn delete_selected(&mut self) {
        if let Some(id) = self.selected_edge.take() {
            self.push_undo();
            self.doc.edges.retain(|e| e.id != id);
            self.dirty = true;
            return;
        }
        let mut ids = self.multi.clone();
        if let Some(id) = &self.selected {
            ids.push(id.clone());
        }
        if ids.is_empty() {
            return;
        }
        self.push_undo();
        self.freeze();
        self.doc.nodes.retain(|n| !ids.contains(&n.id));
        self.doc
            .edges
            .retain(|e| !ids.contains(&e.from) && !ids.contains(&e.to));
        // A frame that still lists a deleted node fails validation, which
        // then refuses to reopen the saved file.
        for group in &mut self.doc.groups {
            group.nodes.retain(|n| !ids.contains(n));
        }
        self.selected = None;
        self.multi.clear();
    }

    /// The Edge tool's click on a node or a frame: the first click picks
    /// the source, the second adds the edge. Frames are endpoints too, so
    /// a frame can connect to another frame or to a node.
    fn edge_endpoint_clicked(&mut self, id: String) {
        let Some(from) = self.edge_from.take() else {
            self.edge_from = Some(id);
            return;
        };
        if from == id {
            return;
        }
        self.push_undo();
        self.freeze();
        let mut n = self.doc.edges.len() + 1;
        while self.doc.edges.iter().any(|e| e.id == format!("e{n}")) {
            n += 1;
        }
        let edge_id = format!("e{n}");
        self.doc.edges.push(Edge {
            id: edge_id.clone(),
            from,
            to: id,
            label: String::new(),
            route: EdgeRoute::Orthogonal,
            arrow: Arrow::End,
            dashed: false,
            dotted: false,
            via: Vec::new(),
            style: Style::default(),
            label_at: None,
            cardinality: None,
        });
        self.selected_edge = Some(edge_id);
        self.selected = None;
        self.selected_group = None;
        self.multi.clear();
        self.dirty = true;
    }

    /// All selected node ids, primary first.
    fn selection(&self) -> Vec<String> {
        let mut ids = Vec::new();
        if let Some(id) = &self.selected {
            ids.push(id.clone());
        }
        for id in &self.multi {
            if !ids.contains(id) {
                ids.push(id.clone());
            }
        }
        ids
    }

    fn align_selection(&mut self, mode: AlignMode) {
        let ids = self.selection();
        if ids.len() < 2 {
            return;
        }
        self.push_undo();
        // Align/Distribute write real node.x/y; while auto_layout is still on,
        // the layout engine recomputes positions from the graph every frame
        // and silently discards them (freeze bakes the current rects in and
        // turns auto_layout off, like drag and Add Frame already do).
        self.freeze();
        // Ids and boxes travel together: zipping two lists went wrong as
        // soon as a selected id no longer existed, every later node then
        // took its predecessor's width and height.
        let boxes: Vec<(String, (f32, f32, f32, f32))> = ids
            .iter()
            .filter_map(|id| {
                self.doc
                    .nodes
                    .iter()
                    .find(|n| &n.id == id)
                    .map(|n| (id.clone(), (n.x, n.y, n.w, n.h)))
            })
            .collect();
        if boxes.len() < 2 {
            return;
        }
        let min_x = boxes.iter().map(|b| b.1.0).fold(f32::INFINITY, f32::min);
        let max_x = boxes
            .iter()
            .map(|b| b.1.0 + b.1.2)
            .fold(f32::NEG_INFINITY, f32::max);
        let min_y = boxes.iter().map(|b| b.1.1).fold(f32::INFINITY, f32::min);
        let max_y = boxes
            .iter()
            .map(|b| b.1.1 + b.1.3)
            .fold(f32::NEG_INFINITY, f32::max);
        let cy = (min_y + max_y) / 2.0;
        let cx = (min_x + max_x) / 2.0;
        for (id, (_, _, w, h)) in boxes {
            let id = &id;
            let Some(n) = self.doc.node_mut(id) else {
                continue;
            };
            match mode {
                AlignMode::Left => n.x = min_x,
                AlignMode::CentreX => n.x = cx - w / 2.0,
                AlignMode::Right => n.x = max_x - w,
                AlignMode::Top => n.y = min_y,
                AlignMode::MiddleY => n.y = cy - h / 2.0,
                AlignMode::Bottom => n.y = max_y - h,
                AlignMode::DistributeX | AlignMode::DistributeY => {}
            }
        }
        if matches!(mode, AlignMode::DistributeX | AlignMode::DistributeY) {
            let mut order = ids.clone();
            let key = |diagram: &DiagramDoc, id: &str| -> f32 {
                diagram
                    .nodes
                    .iter()
                    .find(|n| n.id == id)
                    .map(|n| {
                        if mode == AlignMode::DistributeX {
                            n.x + n.w / 2.0
                        } else {
                            n.y + n.h / 2.0
                        }
                    })
                    .unwrap_or(0.0)
            };
            order.sort_by(|a, b| {
                key(&self.doc, a)
                    .partial_cmp(&key(&self.doc, b))
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            let first = key(&self.doc, &order[0]);
            let last = key(&self.doc, &order[order.len() - 1]);
            let step = (last - first) / (order.len() - 1) as f32;
            for (i, id) in order.iter().enumerate() {
                let target = first + step * i as f32;
                if let Some(n) = self.doc.node_mut(id) {
                    if mode == AlignMode::DistributeX {
                        n.x = target - n.w / 2.0;
                    } else {
                        n.y = target - n.h / 2.0;
                    }
                }
            }
        }
        self.dirty = true;
    }

    pub fn toolbar(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        let s = cx.services;
        let theme = Theme::from_settings(&s.settings.read().appearance);
        if ui
            .button(format!(
                "{} {}",
                hs_ui::icons::ph::FILE_PLUS,
                s.t("gfx-diagram-new")
            ))
            .clicked()
        {
            self.replace(hs_diagram::template::flow());
        }
        if ui
            .button(format!(
                "{} {}",
                hs_ui::icons::ph::FOLDER_OPEN,
                s.t("gfx-diagram-open")
            ))
            .clicked()
        {
            self.open_dialog(cx);
        }
        if ui
            .button(format!(
                "{} {}",
                hs_ui::icons::ph::FLOPPY_DISK,
                s.t("gfx-diagram-save")
            ))
            .clicked()
        {
            self.save(cx);
        }
        ui.menu_button(
            format!(
                "{} {}",
                hs_ui::icons::ph::DOWNLOAD_SIMPLE,
                s.t("gfx-diagram-import-menu")
            ),
            |ui| {
                if ui.button(s.t("gfx-import-archify")).clicked() {
                    self.import(cx, ImportKind::Archify);
                    ui.close();
                }
                if ui.button(s.t("gfx-import-drawio")).clicked() {
                    self.import(cx, ImportKind::DrawIo);
                    ui.close();
                }
                if ui.button(s.t("gfx-import-mermaid")).clicked() {
                    self.import(cx, ImportKind::Mermaid);
                    ui.close();
                }
            },
        );
        if ui
            .button(format!(
                "{} {}",
                hs_ui::icons::ph::FOLDERS,
                s.t("gfx-diagram-save-project")
            ))
            .clicked()
        {
            self.save_name = crate::diagram::sanitise(&self.doc.title);
            self.save_error = None;
            self.save_overwrite = false;
            self.save_open = true;
        }
        ui.separator();
        for (view, icon, key) in [
            (
                DiagramView::Preview,
                hs_ui::icons::ph::EYE,
                "gfx-view-preview",
            ),
            (
                DiagramView::Source,
                hs_ui::icons::ph::CODE,
                "gfx-view-source",
            ),
            (
                DiagramView::Split,
                hs_ui::icons::ph::COLUMNS,
                "gfx-view-split",
            ),
        ] {
            if ui
                .selectable_label(self.view == view, format!("{} {}", icon, s.t(key)))
                .clicked()
            {
                self.view = view;
            }
        }
        ui.menu_button(
            format!("{} {}", hs_ui::icons::ph::EXPORT, s.t("gfx-diagram-export")),
            |ui| {
                if ui.button("PNG (2×)").clicked() {
                    self.export(cx, ExportKind::Png);
                    ui.close();
                }
                if ui.button("SVG").clicked() {
                    self.export(cx, ExportKind::Svg);
                    ui.close();
                }
                if ui.button("PDF").clicked() {
                    self.export(cx, ExportKind::Pdf);
                    ui.close();
                }
            },
        );
        ui.separator();
        if ui
            .add_enabled(
                !self.undo.is_empty(),
                egui::Button::new(format!(
                    "{} {}",
                    hs_ui::icons::ph::ARROW_COUNTER_CLOCKWISE,
                    s.t("gfx-diagram-undo")
                )),
            )
            .clicked()
        {
            self.undo();
        }
        if ui
            .add_enabled(
                !self.redo.is_empty(),
                egui::Button::new(format!(
                    "{} {}",
                    hs_ui::icons::ph::ARROW_CLOCKWISE,
                    s.t("gfx-diagram-redo")
                )),
            )
            .clicked()
        {
            self.redo();
        }
        // Everything below draws on, or adds to, the canvas: in the
        // Source view there is no canvas to act on, so the whole group
        // goes rather than sitting there inert (owner's requirement,
        // 2026-10-26).
        if self.view == DiagramView::Source {
            return;
        }
        // Second row: what acts on the canvas. The rule that used to end
        // the first row goes with it - a row that ends needs no
        // separator (owner's requirement, 2026-10-27).
        hs_ui::toolbar::row_break(ui, &theme);
        if ui
            .button(format!(
                "{} {}",
                hs_ui::icons::ph::FRAME_CORNERS,
                s.t("gfx-zoom-fit")
            ))
            .clicked()
        {
            self.zoom = 0.0;
            self.pan = egui::Vec2::ZERO;
        }
        ui.checkbox(
            &mut self.show_grid,
            format!(
                "{} {}",
                hs_ui::icons::ph::GRID_FOUR,
                s.t("gfx-diagram-grid")
            ),
        );
        if ui
            .selectable_label(
                self.tool == ToolMode::Select,
                format!("{} {}", hs_ui::icons::ph::CURSOR, s.t("gfx-diagram-select")),
            )
            .clicked()
        {
            self.tool = ToolMode::Select;
            self.edge_from = None;
        }
        if ui
            .button(format!(
                "{} {}",
                hs_ui::icons::ph::RECTANGLE,
                s.t("gfx-diagram-add-process")
            ))
            .clicked()
        {
            self.add_node_at(NodeKind::Process, self.last_center);
        }
        if ui
            .button(format!(
                "{} {}",
                hs_ui::icons::ph::DIAMOND,
                s.t("gfx-diagram-add-decision")
            ))
            .clicked()
        {
            self.add_node_at(NodeKind::Decision, self.last_center);
        }
        if ui
            .button(format!(
                "{} {}",
                hs_ui::icons::ph::NOTE_PENCIL,
                s.t("gfx-diagram-add-note")
            ))
            .clicked()
        {
            self.add_node_at(NodeKind::Note, self.last_center);
        }
        if self.doc.kind == DiagramKind::Erd
            && ui
                .button(format!(
                    "{} {}",
                    hs_ui::icons::ph::TABLE,
                    s.t("gfx-diagram-add-entity")
                ))
                .clicked()
        {
            self.add_node_at(NodeKind::Entity, self.last_center);
        }
        if self.doc.kind == DiagramKind::Erd
            && ui
                .button(format!(
                    "{} {}",
                    hs_ui::icons::ph::DATABASE,
                    s.t("gfx-diagram-import-ddl")
                ))
                .tip(s.t("gfx-diagram-import-ddl-tip"))
                .clicked()
            && let Some(path) = rfd::FileDialog::new()
                .add_filter("SQL", &["sql", "ddl", "txt"])
                .pick_file()
        {
            match std::fs::read_to_string(&path)
                .map_err(|e| e.to_string())
                .and_then(|text| hs_diagram::ddl::import(&text))
            {
                Ok(doc) => self.replace(doc),
                Err(e) => cx.toast(ToastLevel::Error, e),
            }
        }
        if self.doc.kind == DiagramKind::Sequence
            && ui
                .button(format!(
                    "{} {}",
                    hs_ui::icons::ph::USER,
                    s.t("gfx-diagram-add-participant")
                ))
                .clicked()
        {
            self.add_node_at(NodeKind::Frontend, self.last_center);
        }
        if ui
            .button(format!(
                "{} {}",
                hs_ui::icons::ph::SELECTION,
                s.t("gfx-diagram-add-frame")
            ))
            .tip(s.t("gfx-diagram-add-frame-tip"))
            .clicked()
        {
            self.add_frame();
        }
        if ui
            .selectable_label(
                self.tool == ToolMode::Edge,
                format!(
                    "{} {}",
                    hs_ui::icons::ph::ARROW_RIGHT,
                    s.t("gfx-diagram-add-edge")
                ),
            )
            .clicked()
        {
            self.tool = ToolMode::Edge;
            self.edge_from = None;
        }
        ui.menu_button(
            format!(
                "{} {}",
                hs_ui::icons::ph::ALIGN_LEFT,
                s.t("gfx-diagram-align")
            ),
            |ui| {
                let enough = self.selection().len() >= 2;
                for (key, mode) in [
                    ("gfx-align-left", AlignMode::Left),
                    ("gfx-align-centre", AlignMode::CentreX),
                    ("gfx-align-right", AlignMode::Right),
                    ("gfx-align-top", AlignMode::Top),
                    ("gfx-align-middle", AlignMode::MiddleY),
                    ("gfx-align-bottom", AlignMode::Bottom),
                ] {
                    let label = s.t(key);
                    if ui.add_enabled(enough, egui::Button::new(label)).clicked() {
                        self.align_selection(mode);
                        ui.close();
                    }
                }
                ui.separator();
                for (key, mode) in [
                    ("gfx-distribute-h", AlignMode::DistributeX),
                    ("gfx-distribute-v", AlignMode::DistributeY),
                ] {
                    let label = s.t(key);
                    if ui.add_enabled(enough, egui::Button::new(label)).clicked() {
                        self.align_selection(mode);
                        ui.close();
                    }
                }
            },
        )
        .response
        .tip(s.t("gfx-diagram-align-tip"));
    }

    /// Where a project save lands: `<root>/<subfolder>/<sanitised title>.hdiag`.
    pub(crate) fn project_save_path(root: &Path, subfolder: &str, name: &str) -> PathBuf {
        let raw = name.trim();
        let stem = raw.strip_suffix(".hdiag").unwrap_or(raw);
        let stem = if stem.trim().is_empty() {
            "diagram".to_string()
        } else {
            sanitise(stem)
        };
        root.join(subfolder.trim()).join(format!("{stem}.hdiag"))
    }

    /// Pick a page when a draw.io file has more than one.
    fn drawio_window(&mut self, ctx: &egui::Context, cx: &mut ToolContext) {
        // Taken, not cloned: the XML of a multi-page draw.io file is
        // routinely megabytes, and this runs every frame the picker is up.
        let Some((text, pages, mut selected)) = self.drawio_import.take() else {
            return;
        };
        let s = cx.services.clone();
        let theme = Theme::from_settings(&s.settings.read().appearance);
        let mut done: Option<bool> = None;
        let modal = egui::Modal::new(egui::Id::new("gfx-drawio-pages")).show(ctx, |ui| {
            ui.set_width(400.0);
            ui.painter().rect_filled(ui.max_rect(), 8.0, theme.panel_bg);
            ui.heading(s.t("gfx-diagram-import"));
            ui.label(s.t("gfx-drawio-choose-page"));
            egui::ComboBox::from_id_salt("gfx-drawio-page")
                .selected_text(&pages[selected].name)
                .show_ui(ui, |ui| {
                    for (i, page) in pages.iter().enumerate() {
                        ui.selectable_value(&mut selected, i, &page.name);
                    }
                });
            ui.add_space(10.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if form::primary_button(ui, &theme, &s.t("gfx-diagram-import")).clicked() {
                    done = Some(true);
                }
                if ui.button(s.t("cancel")).clicked() {
                    done = Some(false);
                }
            });
        });
        if modal.should_close() && done.is_none() {
            done = Some(false);
        }
        match done {
            Some(true) => {
                self.drawio_import = None;
                match hs_diagram::import::drawio_xml_page(&text, selected) {
                    Ok(doc) => {
                        self.replace(doc);
                        self.path = None;
                    }
                    Err(e) => cx.toast(ToastLevel::Error, e),
                }
            }
            Some(false) => self.drawio_import = None,
            None => self.drawio_import = Some((text, pages, selected)),
        }
    }

    /// The Save to Project dialog: project roots come from the Editor through
    /// `OpenTabs`, so Graphics never guesses or depends on the editor crate.
    fn save_window(&mut self, ctx: &egui::Context, cx: &mut ToolContext) {
        if !self.save_open {
            return;
        }
        let s = cx.services.clone();
        let theme = Theme::from_settings(&s.settings.read().appearance);
        let projects = s.open_tabs.projects();
        let mut close = false;
        let modal = egui::Modal::new(egui::Id::new("gfx-save-project")).show(ctx, |ui| {
            ui.set_width(480.0);
            ui.painter().rect_filled(ui.max_rect(), 8.0, theme.panel_bg);
            ui.heading(s.t("gfx-diagram-save-project"));
            if projects.is_empty() {
                ui.label(s.t("gfx-diagram-no-projects"));
                if ui.button(s.t("gfx-diagram-cancel")).clicked() {
                    close = true;
                }
                return;
            }
            form::field_grid(ui, "gfx-save-project-grid", |ui| {
                form::label(ui, &s.t("gfx-diagram-project"));
                let current = self.save_project.min(projects.len().saturating_sub(1));
                egui::ComboBox::from_id_salt("gfx-save-project")
                    .selected_text(
                        projects[current]
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_else(|| projects[current].display().to_string()),
                    )
                    .show_ui(ui, |ui| {
                        for (i, root) in projects.iter().enumerate() {
                            let label = root
                                .file_name()
                                .map(|n| n.to_string_lossy().to_string())
                                .unwrap_or_else(|| root.display().to_string());
                            ui.selectable_value(&mut self.save_project, i, label);
                        }
                    });
                ui.end_row();
                form::label(ui, &s.t("gfx-diagram-subfolder"));
                form::text_field(ui, &mut self.save_subfolder, "docs/diagrams");
                ui.end_row();
                form::label(ui, &s.t("gfx-diagram-filename"));
                form::text_field(ui, &mut self.save_name, "checkout-flow");
                ui.end_row();
            });
            let root = &projects[self.save_project.min(projects.len() - 1)];
            let path = Self::project_save_path(root, &self.save_subfolder, &self.save_name);
            ui.add_space(4.0);
            ui.add(
                egui::Label::new(
                    egui::RichText::new(path.display().to_string())
                        .small()
                        .color(egui::Color32::GRAY),
                )
                .truncate(),
            )
            .tip(path.display().to_string());
            if path.exists() {
                ui.checkbox(&mut self.save_overwrite, s.t("gfx-diagram-overwrite"));
            }
            if let Some(error) = &self.save_error {
                ui.colored_label(egui::Color32::from_rgb(220, 90, 90), error);
            }
            ui.add_space(8.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if form::primary_button(ui, &theme, &s.t("gfx-diagram-save")).clicked() {
                    if path.exists() && !self.save_overwrite {
                        self.save_error = Some(s.t("gfx-diagram-exists"));
                    } else {
                        match hs_diagram::io::save(&path, &self.doc) {
                            Ok(()) => {
                                self.path = Some(path.clone());
                                self.saved = Some(path.clone());
                                self.dirty = false;
                                self.save_open = false;
                                self.save_error = None;
                                self.status = Some(format!(
                                    "{} {}",
                                    s.t("gfx-diagram-saved"),
                                    path.display()
                                ));
                            }
                            Err(e) => self.save_error = Some(e),
                        }
                    }
                }
                if ui.button(s.t("gfx-diagram-cancel")).clicked() {
                    close = true;
                }
            });
        });
        if close || modal.should_close() {
            self.save_open = false;
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext, theme: &Theme) {
        self.undo_limit = cx.services.settings.read().general.undo_limit as usize;
        self.save_window(ui.ctx(), cx);
        self.drawio_window(ui.ctx(), cx);
        egui::Panel::left("gfx-diagram-props")
            .default_size(290.0)
            .size_range(240.0..=(ui.available_width() * 0.4).max(240.0))
            .resizable(true)
            .show(ui, |ui| self.properties(ui, cx, theme));
        if self.view == DiagramView::Split {
            egui::Panel::right("gfx-diagram-source")
                .default_size(420.0)
                .size_range(260.0..=(ui.available_width() * 0.6).max(260.0))
                .resizable(true)
                .show(ui, |ui| self.source_view(ui, cx.services, theme));
        }
        if self.view == DiagramView::Source {
            self.source_view(ui, cx.services, theme);
            return;
        }
        let canvas = ui.available_rect_before_wrap();
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(canvas)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        child.set_clip_rect(canvas);
        self.canvas(&mut child, cx, theme);
    }

    fn source_view(&mut self, ui: &mut egui::Ui, s: &Services, theme: &Theme) {
        let text = hs_diagram::io::to_json(&self.doc);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(s.t("gfx-source-json")).strong());
            if ui.button(s.t("gfx-source-copy")).clicked() {
                ui.ctx().copy_text(text.clone());
            }
        });
        ui.separator();
        egui::ScrollArea::both()
            .id_salt((
                "gfx-diagram-source-scroll",
                matches!(self.view, DiagramView::Source),
            ))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let mut read_only = text;
                ui.add(
                    egui::TextEdit::multiline(&mut read_only)
                        .code_editor()
                        .desired_width(f32::INFINITY)
                        .interactive(false),
                );
            });
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new(s.t("gfx-diagram-source-hint"))
                .small()
                .color(theme.text_muted),
        );
    }

    fn properties(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext, theme: &Theme) {
        let s = cx.services;
        egui::ScrollArea::vertical()
            .id_salt("gfx-diagram-props-scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(6.0);
                section_title(ui, theme, &s.t("gfx-diagram-properties"));
                let mut doc_changed = false;
                props_grid(ui, "gfx-props-doc", |ui| {
                    props_label(ui, &s.t("gfx-diagram-title"));
                    doc_changed |= ui
                        .add(
                            egui::TextEdit::singleline(&mut self.doc.title)
                                .hint_text(s.t("gfx-diagram-title"))
                                .desired_width(f32::INFINITY),
                        )
                        .changed();
                    ui.end_row();
                    props_label(ui, &s.t("gfx-diagram-frame"));
                    doc_changed |= ui
                        .checkbox(&mut self.doc.show_title, s.t("gfx-diagram-show-title"))
                        .changed();
                    ui.end_row();
                    ui.add_enabled_ui(self.doc.show_title, |ui| {
                        props_label(ui, &s.t("gfx-prop-position"));
                    });
                    ui.add_enabled_ui(self.doc.show_title, |ui| {
                        doc_changed |= placement_picker(ui, s, &mut self.doc.title_placement);
                    });
                    ui.end_row();
                    ui.add_enabled_ui(self.doc.show_title, |ui| {
                        props_label(ui, &s.t("gfx-prop-text-align"));
                    });
                    ui.add_enabled_ui(self.doc.show_title, |ui| {
                        doc_changed |= align_picker(ui, s, &mut self.doc.title_align);
                    });
                    ui.end_row();
                });
                if doc_changed {
                    self.dirty = true;
                }
                ui.add_space(6.0);
                props_label_inline(ui, theme, &s.t("gfx-prop-templates"));
                ui.horizontal_wrapped(|ui| {
                    if ui.button(s.t("gfx-diagram-flow-template")).clicked() {
                        self.replace(hs_diagram::template::flow());
                    }
                    if ui.button(s.t("gfx-diagram-arch-template")).clicked() {
                        self.replace(hs_diagram::template::architecture());
                    }
                    if ui.button(s.t("gfx-diagram-erd-template")).clicked() {
                        self.replace(hs_diagram::template::erd());
                    }
                    if ui.button(s.t("gfx-diagram-sequence-template")).clicked() {
                        self.replace(hs_diagram::template::sequence());
                    }
                    if ui.button(s.t("gfx-diagram-blank")).clicked() {
                        self.replace(DiagramDoc::new(DiagramKind::Flow, "Untitled diagram"));
                    }
                });
                ui.add_space(8.0);
                if self.selected.is_some()
                    || self.selected_edge.is_some()
                    || self.selected_group.is_some()
                {
                    ui.separator();
                    ui.add_space(6.0);
                }
                if let Some(id) = self.selected.clone() {
                    let mut changed = false;
                    let mut remove = false;
                    if let Some(node) = self.doc.node_mut(&id) {
                        section_title(ui, theme, &s.t("gfx-diagram-node"));
                        props_grid(ui, "gfx-props-node", |ui| {
                            props_label(ui, &s.t("gfx-diagram-label"));
                            changed |= ui
                                .add(
                                    egui::TextEdit::singleline(&mut node.label)
                                        .desired_width(f32::INFINITY),
                                )
                                .changed();
                            ui.end_row();
                            props_label(ui, &s.t("gfx-diagram-shape"));
                            egui::ComboBox::from_id_salt("gfx-diagram-shape")
                                .width(ui.available_width())
                                .selected_text(shape_label(node.kind, s))
                                .show_ui(ui, |ui| {
                                    for kind in [
                                        NodeKind::Process,
                                        NodeKind::RoundedRectangle,
                                        NodeKind::Decision,
                                        NodeKind::StartEnd,
                                        NodeKind::InputOutput,
                                        NodeKind::Database,
                                        NodeKind::Note,
                                        NodeKind::Text,
                                        NodeKind::Frontend,
                                        NodeKind::Backend,
                                        NodeKind::Store,
                                        NodeKind::Queue,
                                    ] {
                                        if ui
                                            .selectable_value(
                                                &mut node.kind,
                                                kind,
                                                shape_label(kind, s),
                                            )
                                            .changed()
                                        {
                                            changed = true;
                                        }
                                    }
                                });
                            ui.end_row();
                            props_label(ui, &s.t("gfx-prop-outline"));
                            color_cell(ui, s, &mut node.style.stroke, &mut changed);
                            ui.end_row();
                            props_label(ui, &s.t("gfx-prop-fill"));
                            color_cell(ui, s, &mut node.style.fill, &mut changed);
                            ui.end_row();
                            props_label(ui, &s.t("gfx-prop-text"));
                            color_cell(ui, s, &mut node.style.text, &mut changed);
                            ui.end_row();
                        });
                        if node.kind == NodeKind::Entity {
                            if node.erd.is_none() {
                                node.erd = Some(hs_diagram::model::Entity::from_label(&node.label));
                            }
                            ui.add_space(6.0);
                            props_label_inline(ui, theme, &s.t("gfx-prop-columns"));
                            let mut remove_column = None;
                            let mut move_up = None;
                            let mut move_down = None;
                            if let Some(erd) = node.erd.as_mut() {
                                egui::Grid::new("gfx-entity-columns")
                                    .num_columns(4)
                                    .spacing([6.0, 4.0])
                                    .show(ui, |ui| {
                                        for (i, column) in erd.columns.iter_mut().enumerate() {
                                            changed |= ui
                                                .add(
                                                    egui::TextEdit::singleline(&mut column.name)
                                                        .desired_width(78.0)
                                                        .hint_text(s.t("gfx-col-name")),
                                                )
                                                .changed();
                                            changed |= ui
                                                .add(
                                                    egui::TextEdit::singleline(
                                                        &mut column.data_type,
                                                    )
                                                    .desired_width(64.0)
                                                    .hint_text(s.t("gfx-col-type")),
                                                )
                                                .changed();
                                            egui::ComboBox::from_id_salt(("column-key", i))
                                                .width(56.0)
                                                .selected_text(match column.key {
                                                    hs_diagram::model::ColumnKey::None => "—",
                                                    hs_diagram::model::ColumnKey::Primary => "PK",
                                                    hs_diagram::model::ColumnKey::Foreign => "FK",
                                                    hs_diagram::model::ColumnKey::Unique => "UQ",
                                                })
                                                .show_ui(ui, |ui| {
                                                    for (key, label) in [
                                                        (hs_diagram::model::ColumnKey::None, "—"),
                                                        (
                                                            hs_diagram::model::ColumnKey::Primary,
                                                            "PK",
                                                        ),
                                                        (
                                                            hs_diagram::model::ColumnKey::Foreign,
                                                            "FK",
                                                        ),
                                                        (
                                                            hs_diagram::model::ColumnKey::Unique,
                                                            "UQ",
                                                        ),
                                                    ] {
                                                        if ui
                                                            .selectable_value(
                                                                &mut column.key,
                                                                key,
                                                                label,
                                                            )
                                                            .changed()
                                                        {
                                                            changed = true;
                                                        }
                                                    }
                                                });
                                            ui.horizontal(|ui| {
                                                if ui
                                                    .add_enabled(
                                                        i > 0,
                                                        egui::Button::new("↑").small(),
                                                    )
                                                    .tip(s.t("gfx-col-up"))
                                                    .clicked()
                                                {
                                                    move_up = Some(i);
                                                }
                                                if ui
                                                    .small_button("↓")
                                                    .tip(s.t("gfx-col-down"))
                                                    .clicked()
                                                {
                                                    move_down = Some(i);
                                                }
                                                if ui
                                                    .small_button("−")
                                                    .tip(s.t("gfx-col-remove"))
                                                    .clicked()
                                                {
                                                    remove_column = Some(i);
                                                }
                                            });
                                            ui.end_row();
                                        }
                                    });
                                if let Some(i) = move_up {
                                    erd.columns.swap(i, i - 1);
                                    changed = true;
                                }
                                if let Some(i) = move_down
                                    && i + 1 < erd.columns.len()
                                {
                                    erd.columns.swap(i, i + 1);
                                    changed = true;
                                }
                                if let Some(i) = remove_column {
                                    erd.columns.remove(i);
                                    changed = true;
                                }
                                ui.add_space(2.0);
                                if ui.button("+ Add Column").clicked() {
                                    erd.columns.push(hs_diagram::model::Column {
                                        name: String::new(),
                                        data_type: "TEXT".into(),
                                        key: hs_diagram::model::ColumnKey::None,
                                    });
                                    changed = true;
                                }
                            }
                        }
                        ui.add_space(6.0);
                        if ui.button(s.t("gfx-diagram-delete")).clicked() {
                            remove = true;
                        }
                        if changed {
                            self.dirty = true;
                        }
                    }
                    if remove {
                        self.delete_selected();
                    }
                    ui.add_space(6.0);
                }

                if let Some(eid) = self.selected_edge.clone() {
                    let mut changed = false;
                    let mut remove = false;
                    let mut add = false;
                    let mut clear = false;
                    if let Some(edge) = self.doc.edges.iter_mut().find(|e| e.id == eid) {
                        section_title(ui, theme, &s.t("gfx-prop-edge"));
                        ui.label(
                            egui::RichText::new(format!("{} → {}", edge.from, edge.to))
                                .small()
                                .color(theme.text_muted),
                        );
                        ui.add_space(2.0);
                        let is_erd = self.doc.kind == DiagramKind::Erd;
                        props_grid(ui, "gfx-props-edge", |ui| {
                            props_label(ui, &s.t("gfx-diagram-label"));
                            changed |= ui
                                .add(
                                    egui::TextEdit::singleline(&mut edge.label)
                                        .desired_width(f32::INFINITY),
                                )
                                .changed();
                            ui.end_row();
                            if is_erd {
                                props_label(ui, &s.t("gfx-prop-cardinality"));
                                egui::ComboBox::from_id_salt("gfx-edge-cardinality")
                                    .width(ui.available_width())
                                    .selected_text(
                                        edge.cardinality
                                            .map(|c| c.label().to_string())
                                            .unwrap_or_else(|| s.t("gfx-card-custom")),
                                    )
                                    .show_ui(ui, |ui| {
                                        if ui
                                            .selectable_value(
                                                &mut edge.cardinality,
                                                None,
                                                s.t("gfx-card-custom"),
                                            )
                                            .changed()
                                        {
                                            changed = true;
                                        }
                                        for (value, label) in [
                                            (hs_diagram::model::Cardinality::One, "1"),
                                            (hs_diagram::model::Cardinality::Many, "*"),
                                            (hs_diagram::model::Cardinality::OneToMany, "1 .. *"),
                                            (hs_diagram::model::Cardinality::ZeroOrOne, "0 .. 1"),
                                            (hs_diagram::model::Cardinality::ZeroOrMany, "0 .. *"),
                                        ] {
                                            if ui
                                                .selectable_value(
                                                    &mut edge.cardinality,
                                                    Some(value),
                                                    label,
                                                )
                                                .changed()
                                            {
                                                edge.label = value.label().into();
                                                changed = true;
                                            }
                                        }
                                    });
                                ui.end_row();
                            }
                            props_label(ui, &s.t("gfx-prop-arrow"));
                            ui.horizontal(|ui| {
                                for (value, key) in [
                                    (Arrow::None, "gfx-arrow-none"),
                                    (Arrow::End, "gfx-arrow-end"),
                                    (Arrow::Both, "gfx-arrow-both"),
                                ] {
                                    changed |= ui
                                        .selectable_value(&mut edge.arrow, value, s.t(key))
                                        .changed();
                                }
                            });
                            ui.end_row();
                            props_label(ui, &s.t("gfx-prop-route"));
                            ui.horizontal(|ui| {
                                for (value, key) in [
                                    (EdgeRoute::Straight, "gfx-route-straight"),
                                    (EdgeRoute::Orthogonal, "gfx-route-orthogonal"),
                                    (EdgeRoute::Curved, "gfx-route-curved"),
                                ] {
                                    changed |= ui
                                        .selectable_value(&mut edge.route, value, s.t(key))
                                        .changed();
                                }
                            });
                            ui.end_row();
                            props_label(ui, &s.t("gfx-prop-line"));
                            ui.horizontal(|ui| {
                                let solid = !edge.dashed && !edge.dotted;
                                if ui.selectable_label(solid, s.t("gfx-line-solid")).clicked() {
                                    edge.dashed = false;
                                    edge.dotted = false;
                                    changed = true;
                                }
                                if ui
                                    .selectable_label(edge.dashed, s.t("gfx-line-dashed"))
                                    .clicked()
                                {
                                    edge.dashed = true;
                                    edge.dotted = false;
                                    changed = true;
                                }
                                if ui
                                    .selectable_label(edge.dotted, s.t("gfx-line-dotted"))
                                    .clicked()
                                {
                                    edge.dotted = true;
                                    edge.dashed = false;
                                    changed = true;
                                }
                            });
                            ui.end_row();
                            props_label(ui, &s.t("gfx-prop-outline"));
                            color_cell(ui, s, &mut edge.style.stroke, &mut changed);
                            ui.end_row();
                            props_label(ui, &s.t("gfx-prop-label-colour"));
                            color_cell(ui, s, &mut edge.style.text, &mut changed);
                            ui.end_row();
                        });
                        ui.add_space(6.0);
                        ui.horizontal_wrapped(|ui| {
                            if ui.button(s.t("gfx-add-waypoint")).clicked() {
                                add = true;
                            }
                            if ui
                                .add_enabled(
                                    !edge.via.is_empty(),
                                    egui::Button::new(s.t("gfx-clear-waypoints")),
                                )
                                .clicked()
                            {
                                clear = true;
                            }
                            if ui.button(s.t("gfx-diagram-delete-edge")).clicked() {
                                remove = true;
                            }
                        });
                    }
                    if add {
                        self.add_edge_waypoint(&eid);
                    }
                    if clear && self.doc.edges.iter().any(|e| e.id == eid) {
                        self.push_undo();
                        if let Some(edge) = self.doc.edges.iter_mut().find(|e| e.id == eid) {
                            edge.via.clear();
                        }
                        self.dirty = true;
                    }
                    if changed {
                        self.dirty = true;
                    }
                    if remove {
                        self.selected_edge = Some(eid);
                        self.delete_selected();
                    }
                    ui.add_space(6.0);
                }

                if let Some(gid) = self.selected_group.clone() {
                    // Only the selected frame's own fields show here - the block
                    // used to render unconditionally for every frame in the
                    // document whenever one existed, stacking a stray separator
                    // and an unrelated frame list on top of whatever node/edge
                    // section was already showing above it.
                    if self.selected.is_some() || self.selected_edge.is_some() {
                        ui.separator();
                    }
                    ui.add_space(4.0);
                    section_title(ui, theme, &s.t("gfx-diagram-frame"));
                    let mut frame_changed = false;
                    if let Some(group) = self.doc.groups.iter_mut().find(|g| g.id == gid) {
                        props_grid(ui, ("gfx-props-frame", &group.id), |ui| {
                            props_label(ui, &s.t("gfx-prop-text"));
                            frame_changed |= ui
                                .add(
                                    egui::TextEdit::singleline(&mut group.label)
                                        .desired_width(f32::INFINITY),
                                )
                                .changed();
                            ui.end_row();
                            props_label(ui, &s.t("gfx-prop-outline"));
                            color_cell(ui, s, &mut group.style.stroke, &mut frame_changed);
                            ui.end_row();
                            props_label(ui, &s.t("gfx-prop-fill"));
                            color_cell(ui, s, &mut group.style.fill, &mut frame_changed);
                            ui.end_row();
                            props_label(ui, &s.t("gfx-prop-text-colour"));
                            color_cell(ui, s, &mut group.style.text, &mut frame_changed);
                            ui.end_row();
                            props_label(ui, &s.t("gfx-prop-position"));
                            frame_changed |= placement_picker(ui, s, &mut group.label_placement);
                            ui.end_row();
                            props_label(ui, &s.t("gfx-prop-text-align"));
                            frame_changed |= align_picker(ui, s, &mut group.label_align);
                            ui.end_row();
                        });
                    }
                    if frame_changed {
                        self.dirty = true;
                    }
                    ui.add_space(6.0);
                }
                if self.doc.kind == DiagramKind::Sequence {
                    ui.separator();
                    ui.add_space(4.0);
                    section_title(ui, theme, &s.t("gfx-prop-fragments"));
                    let mut fragment_changed = false;
                    let mut remove = None;
                    let last = self.doc.edges.len().saturating_sub(1);
                    egui::Grid::new("gfx-fragments")
                        .num_columns(5)
                        .spacing([6.0, 4.0])
                        .show(ui, |ui| {
                            for (i, fragment) in self.doc.fragments.iter_mut().enumerate() {
                                egui::ComboBox::from_id_salt(("fragment-kind", i))
                                    .width(64.0)
                                    .selected_text(match fragment.kind {
                                        hs_diagram::model::FragmentKind::Alt => "alt",
                                        hs_diagram::model::FragmentKind::Loop => "loop",
                                        hs_diagram::model::FragmentKind::Opt => "opt",
                                        hs_diagram::model::FragmentKind::Note => "note",
                                    })
                                    .show_ui(ui, |ui| {
                                        for (kind, label) in [
                                            (hs_diagram::model::FragmentKind::Alt, "alt"),
                                            (hs_diagram::model::FragmentKind::Loop, "loop"),
                                            (hs_diagram::model::FragmentKind::Opt, "opt"),
                                            (hs_diagram::model::FragmentKind::Note, "note"),
                                        ] {
                                            if ui
                                                .selectable_value(&mut fragment.kind, kind, label)
                                                .changed()
                                            {
                                                fragment_changed = true;
                                            }
                                        }
                                    });
                                fragment_changed |= ui
                                    .add(
                                        egui::TextEdit::singleline(&mut fragment.label)
                                            .hint_text(s.t("gfx-frag-condition"))
                                            .desired_width(86.0),
                                    )
                                    .changed();
                                let from = ui.add(
                                    egui::DragValue::new(&mut fragment.from)
                                        .range(0..=last)
                                        .prefix(s.t("gfx-frag-from")),
                                );
                                let to = ui.add(
                                    egui::DragValue::new(&mut fragment.to)
                                        .range(0..=last)
                                        .prefix(s.t("gfx-frag-to")),
                                );
                                if from.changed() || to.changed() {
                                    fragment.to = fragment.to.max(fragment.from);
                                    fragment_changed = true;
                                }
                                if ui.small_button("−").tip(s.t("gfx-frag-remove")).clicked() {
                                    remove = Some(i);
                                }
                                ui.end_row();
                            }
                        });
                    if let Some(i) = remove {
                        self.doc.fragments.remove(i);
                        fragment_changed = true;
                    }
                    ui.add_space(2.0);
                    ui.horizontal_wrapped(|ui| {
                        for (kind, label) in [
                            (hs_diagram::model::FragmentKind::Alt, "+ Alt"),
                            (hs_diagram::model::FragmentKind::Loop, "+ Loop"),
                            (hs_diagram::model::FragmentKind::Opt, "+ Opt"),
                            (hs_diagram::model::FragmentKind::Note, "+ Note"),
                        ] {
                            if ui.button(label).clicked() {
                                self.doc.fragments.push(hs_diagram::model::Fragment {
                                    kind,
                                    label: String::new(),
                                    from: 0,
                                    to: last,
                                });
                                fragment_changed = true;
                            }
                        }
                    });
                    if fragment_changed {
                        self.dirty = true;
                    }
                }
                if self.selected.is_some()
                    || self.selected_edge.is_some()
                    || self.selected_group.is_some()
                {
                    ui.separator();
                }
                ui.add_space(4.0);
                section_title(ui, theme, &s.t("gfx-diagram-outline"));
                let nodes: Vec<(String, String, &str)> = self
                    .doc
                    .nodes
                    .iter()
                    .map(|n| (n.id.clone(), n.label.clone(), shape_icon(n.kind)))
                    .collect();
                ui.indent("gfx-outline-nodes", |ui| {
                    for (id, label, icon) in nodes {
                        let selected = self.selected.as_deref() == Some(id.as_str());
                        if ui
                            .selectable_label(selected, format!("{icon}  {label}"))
                            .clicked()
                        {
                            self.selected = Some(id);
                            self.selected_edge = None;
                            self.selected_group = None;
                            self.multi.clear();
                        }
                    }
                });
                ui.indent("gfx-outline-edges", |ui| {
                    for edge in &self.doc.edges {
                        let label = if edge.label.is_empty() {
                            format!("{} → {}", edge.from, edge.to)
                        } else {
                            format!("{} → {} ({})", edge.from, edge.to, edge.label)
                        };
                        let selected = self.selected_edge.as_deref() == Some(edge.id.as_str());
                        if ui
                            .selectable_label(
                                selected,
                                format!("{}  {}", hs_ui::icons::ph::ARROW_RIGHT, label),
                            )
                            .clicked()
                        {
                            self.selected_edge = Some(edge.id.clone());
                            self.selected = None;
                            self.selected_group = None;
                            self.multi.clear();
                        }
                    }
                });
                ui.separator();
                ui.add_space(4.0);
                section_title(ui, theme, &s.t("gfx-prop-status"));
                let issues = validate::validate(&self.doc);
                let errors = issues
                    .iter()
                    .filter(|i| i.severity == hs_diagram::Severity::Error)
                    .count();
                if errors > 0 {
                    ui.colored_label(
                        theme.error,
                        s.i18n
                            .t_args("gfx-diagram-errors", &[("count", &errors.to_string())]),
                    );
                } else {
                    ui.label(
                        egui::RichText::new(s.t("gfx-diagram-valid"))
                            .small()
                            .color(theme.text_muted),
                    );
                }
                if let Some(status) = &self.status {
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new(status).small().color(theme.text_muted));
                }
            });
    }

    fn canvas(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext, theme: &Theme) {
        let rect = ui.max_rect();
        let scene = self.scene_cached();
        let z = if self.zoom == 0.0 {
            ((rect.width() - 48.0) / scene.size.w.max(1.0))
                .min((rect.height() - 48.0) / scene.size.h.max(1.0))
                .clamp(0.15, 4.0)
        } else {
            self.zoom
        };
        let mut pan = self.pan;
        if self.zoom == 0.0 {
            pan = egui::Vec2::ZERO;
        }
        let origin = rect.center() - egui::vec2(scene.size.w, scene.size.h) * (z / 2.0) + pan;
        let to_screen =
            |p: Point| -> egui::Pos2 { egui::pos2(origin.x + p.x * z, origin.y + p.y * z) };
        let srect = |r: Rect| -> egui::Rect {
            egui::Rect::from_min_max(
                to_screen(Point { x: r.x, y: r.y }),
                to_screen(Point {
                    x: r.right(),
                    y: r.bottom(),
                }),
            )
        };

        let response = ui.allocate_rect(rect, egui::Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, theme.panel_bg);
        if self.show_grid {
            self.paint_grid(&painter, rect, origin, z, theme);
        }
        Self::paint_scene(&painter, &scene, &to_screen, &srect, z, theme);

        let node_rects = self.node_rects(&scene);
        // One polyline per document edge, in edge order, found by id: a
        // positional list drifted as soon as a lifeline or an edge with an
        // unresolved endpoint sat in between.
        let edge_lines: Vec<Vec<Point>> = self
            .doc
            .edges
            .iter()
            .map(|edge| {
                scene
                    .prims
                    .iter()
                    .find_map(|p| match p {
                        Prim::Line {
                            points,
                            edge_id: Some(id),
                            ..
                        } if id == &edge.id => Some(points.clone()),
                        _ => None,
                    })
                    .unwrap_or_default()
            })
            .collect();
        self.edge_paths = edge_lines.clone();
        let pointer = response.interact_pointer_pos();
        let space = ui.input(|i| i.key_down(egui::Key::Space));
        if response.drag_started() {
            if space || ui.input(|i| i.pointer.middle_down()) {
                self.drag_pan = Some((self.pan, pointer.unwrap_or(rect.center())));
            } else if let Some(pos) = pointer {
                let mut grip = false;
                if let Some(id) = &self.selected
                    && let Some(i) = self.doc.nodes.iter().position(|n| &n.id == id)
                    && i < node_rects.len()
                {
                    let br = srect(node_rects[i]).right_bottom();
                    if (pos - br).abs().max_elem() < 12.0 {
                        self.push_undo();
                        self.freeze();
                        if let Some(n) = self.doc.nodes.get(i) {
                            self.drag_resize = Some((n.id.clone(), n.w, n.h, pos));
                        }
                        grip = true;
                    }
                }
                if !grip && let Some(eid) = self.selected_edge.clone() {
                    let vias = self
                        .doc
                        .edges
                        .iter()
                        .find(|e| e.id == eid)
                        .map(|e| e.via.clone())
                        .unwrap_or_default();
                    for (i, via) in vias.iter().enumerate() {
                        if (pos - to_screen(*via)).length() < 11.0 {
                            self.push_undo();
                            self.drag_via = Some((eid.clone(), i, *via, pos));
                            grip = true;
                            break;
                        }
                    }
                }
                if !grip
                    && let Some(gid) = self.selected_group.clone()
                    && let Some(rect) = scene.prims.iter().find_map(|p| match p {
                        Prim::Frame { rect, group_id, .. }
                            if group_id.as_deref() == Some(&*gid) =>
                        {
                            Some(*rect)
                        }
                        _ => None,
                    })
                {
                    let r = srect(rect);
                    if (pos - r.right_bottom()).abs().max_elem() < 14.0
                        && let Some((gid, nodes)) = self
                            .selected_group
                            .as_ref()
                            .and_then(|id| self.doc.groups.iter().find(|g| &g.id == id))
                            .map(|g| {
                                let nodes = g
                                    .nodes
                                    .iter()
                                    .filter_map(|id| {
                                        self.doc.node(id).map(|n| {
                                            (id.clone(), n.x, n.y, n.w.max(40.0), n.h.max(30.0))
                                        })
                                    })
                                    .collect();
                                (g.id.clone(), nodes)
                            })
                    {
                        self.push_undo();
                        self.freeze();
                        self.drag_frame_resize = Some((gid, r, nodes, pos));
                        grip = true;
                    }
                }
                if !grip {
                    let frame_hit = scene.prims.iter().find_map(|p| {
                        let Prim::Frame { rect, group_id, .. } = p else {
                            return None;
                        };
                        let r = srect(*rect);
                        let edge_dist = (pos.x - r.left())
                            .abs()
                            .min((pos.x - r.right()).abs())
                            .min((pos.y - r.top()).abs())
                            .min((pos.y - r.bottom()).abs());
                        if r.contains(pos) && (edge_dist < 10.0 || (pos.y - r.top()).abs() < 22.0) {
                            group_id.clone()
                        } else {
                            None
                        }
                    });
                    if let Some(gid) = frame_hit
                        && let Some((gid, ids)) = self
                            .doc
                            .groups
                            .iter()
                            .find(|g| g.id == gid)
                            .map(|g| (g.id.clone(), g.nodes.clone()))
                    {
                        self.push_undo();
                        self.freeze();
                        let nodes: Vec<(String, Point)> = ids
                            .iter()
                            .filter_map(|id| {
                                self.doc
                                    .node(id)
                                    .map(|n| (id.clone(), Point { x: n.x, y: n.y }))
                            })
                            .collect();
                        self.selected_group = Some(gid.clone());
                        self.selected = None;
                        self.selected_edge = None;
                        self.multi.clear();
                        self.drag_frame = Some((gid, pos, nodes));
                        grip = true;
                    }
                }
                if !grip {
                    let hit = self.hit(pos, &node_rects, origin, z);
                    if let Some(i) = hit {
                        self.push_undo();
                        self.freeze();
                        let shift = ui.input(|i| i.modifiers.shift);
                        let id = self.doc.nodes[i].id.clone();
                        if shift {
                            if !self.multi.contains(&id)
                                && self.selected.as_deref() != Some(id.as_str())
                            {
                                self.multi.push(id.clone());
                            }
                        } else {
                            self.selected = Some(id.clone());
                            self.multi.clear();
                            self.selected_edge = None;
                        }
                        self.drag_node = Some((
                            id,
                            Point {
                                x: self.doc.nodes[i].x,
                                y: self.doc.nodes[i].y,
                            },
                            pos,
                        ));
                    } else if !ui.input(|i| i.modifiers.shift) {
                        self.selected = None;
                        self.multi.clear();
                    }
                }
            }
        }
        if response.dragged() {
            if let (Some((start, from)), Some(pos)) = (self.drag_pan, pointer) {
                self.pan = start + (pos - from);
            } else if let (Some((id, idx, start, from)), Some(pos)) = (&self.drag_via, pointer) {
                let dx = (pos.x - from.x) / z;
                let dy = (pos.y - from.y) / z;
                if let Some(edge) = self.doc.edges.iter_mut().find(|e| &e.id == id)
                    && let Some(via) = edge.via.get_mut(*idx)
                {
                    via.x = start.x + dx;
                    via.y = start.y + dy;
                }
                self.dirty = true;
            } else if let (Some((_, start, nodes, from)), Some(pos)) =
                (&self.drag_frame_resize, pointer)
            {
                let left = (start.left() - origin.x) / z;
                let top = (start.top() - origin.y) / z;
                let sx =
                    ((start.width() + (pos.x - from.x)) / start.width().max(1.0)).clamp(0.2, 5.0);
                let sy =
                    ((start.height() + (pos.y - from.y)) / start.height().max(1.0)).clamp(0.2, 5.0);
                for (id, x, y, w, h) in nodes {
                    if let Some(node) = self.doc.nodes.iter_mut().find(|n| &n.id == id) {
                        node.x = left + (x - left) * sx;
                        node.y = top + (y - top) * sy;
                        node.w = (w * sx).max(40.0);
                        node.h = (h * sy).max(30.0);
                    }
                }
                self.dirty = true;
            } else if let (Some((_, from, nodes)), Some(pos)) = (&self.drag_frame, pointer) {
                let dx = (pos.x - from.x) / z;
                let dy = (pos.y - from.y) / z;
                for (id, start) in nodes {
                    if let Some(node) = self.doc.nodes.iter_mut().find(|n| &n.id == id) {
                        node.x = start.x + dx;
                        node.y = start.y + dy;
                    }
                }
                self.dirty = true;
            } else if let (Some((id, sw, sh, from)), Some(pos)) = (&self.drag_resize, pointer) {
                let dx = (pos.x - from.x) / z;
                let dy = (pos.y - from.y) / z;
                if let Some(node) = self.doc.node_mut(id) {
                    node.w = (sw + dx).max(40.0);
                    node.h = (sh + dy).max(36.0);
                }
                self.dirty = true;
            } else if let (Some((id, start, from)), Some(pos)) = (&self.drag_node, pointer) {
                let dx = (pos.x - from.x) / z;
                let dy = (pos.y - from.y) / z;
                if let Some(node) = self.doc.node_mut(id) {
                    node.x = start.x + dx;
                    node.y = start.y + dy;
                }
                self.dirty = true;
                // Highlight whichever frame the node would drop into if the
                // user let go right now, so dragging a shape toward a frame
                // shows where it is about to land before the drop happens.
                if let Some(node) = self.doc.node(id) {
                    let center = to_screen(Point {
                        x: node.x + node.w / 2.0,
                        y: node.y + node.h / 2.0,
                    });
                    if let Some(target) = scene.prims.iter().find_map(|p| match p {
                        Prim::Frame { rect, .. } if srect(*rect).contains(center) => {
                            Some(srect(*rect))
                        }
                        _ => None,
                    }) {
                        painter.rect_filled(target, 6.0, theme.accent.gamma_multiply(0.12));
                        painter.rect_stroke(
                            target,
                            6.0,
                            egui::Stroke::new(2.0, theme.accent),
                            egui::StrokeKind::Inside,
                        );
                    }
                }
            }
        }
        if response.drag_stopped() {
            let dragged = self.drag_node.as_ref().map(|(id, _, _)| id.clone());
            if let Some(id) = dragged
                && let Some(node) = self.doc.node(&id)
            {
                let center = to_screen(Point {
                    x: node.x + node.w / 2.0,
                    y: node.y + node.h / 2.0,
                });
                let target = scene.prims.iter().find_map(|p| match p {
                    Prim::Frame { rect, group_id, .. } if srect(*rect).contains(center) => {
                        group_id.clone()
                    }
                    _ => None,
                });
                if let Some(gid) = target {
                    for group in &mut self.doc.groups {
                        group.nodes.retain(|n| n != &id);
                    }
                    if let Some(group) = self.doc.groups.iter_mut().find(|g| g.id == gid) {
                        group.nodes.push(id);
                    }
                    self.dirty = true;
                }
            }
            self.drag_node = None;
            self.drag_resize = None;
            self.drag_via = None;
            self.drag_frame = None;
            self.drag_frame_resize = None;
            self.drag_pan = None;
        }
        let clicked =
            response.clicked() || (response.hovered() && ui.input(|i| i.pointer.primary_clicked()));
        let clicked_pos = pointer.or_else(|| ui.input(|i| i.pointer.interact_pos()));
        if clicked && let Some(pos) = clicked_pos {
            if let Some(i) = self.hit(pos, &node_rects, origin, z) {
                let id = self.doc.nodes[i].id.clone();
                if self.tool == ToolMode::Edge {
                    self.edge_endpoint_clicked(id.clone());
                }
                if self.tool == ToolMode::Select && ui.input(|i| i.modifiers.shift) {
                    if !self.multi.contains(&id) && self.selected.as_deref() != Some(id.as_str()) {
                        self.multi.push(id);
                    }
                    self.selected_edge = None;
                    self.selected_group = None;
                } else {
                    self.selected = Some(id);
                    self.multi.clear();
                    self.selected_edge = None;
                    self.selected_group = None;
                }
            } else {
                let frame_hit = scene.prims.iter().find_map(|p| {
                    let Prim::Frame { rect, group_id, .. } = p else {
                        return None;
                    };
                    let r = srect(*rect);
                    let edge_dist = (pos.x - r.left())
                        .abs()
                        .min((pos.x - r.right()).abs())
                        .min((pos.y - r.top()).abs())
                        .min((pos.y - r.bottom()).abs());
                    if r.contains(pos) && (edge_dist < 10.0 || (pos.y - r.top()).abs() < 22.0) {
                        group_id.clone()
                    } else {
                        None
                    }
                });
                if let Some(gid) = frame_hit {
                    if self.tool == ToolMode::Edge {
                        // A frame is an edge endpoint like a node is.
                        self.edge_endpoint_clicked(gid);
                    } else {
                        self.selected_group = Some(gid);
                        self.selected = None;
                        self.selected_edge = None;
                        self.multi.clear();
                    }
                } else {
                    let mut hit_edge = None;
                    for (i, edge) in self.doc.edges.iter().enumerate() {
                        if edge_lines.get(i).is_some_and(|points| {
                            points.windows(2).any(|seg| {
                                point_to_segment(pos, to_screen(seg[0]), to_screen(seg[1])) < 8.0
                            })
                        }) {
                            hit_edge = Some(edge.id.clone());
                            break;
                        }
                    }
                    self.selected_edge = hit_edge;
                    self.selected = None;
                    self.selected_group = None;
                    self.multi.clear();
                }
            }
        }
        if response.double_clicked()
            && let Some(pos) = pointer
            && let Some(i) = self.hit(pos, &node_rects, origin, z)
        {
            // The properties panel edits the label; this only keeps the
            // selected node visible there.
            self.selected = Some(self.doc.nodes[i].id.clone());
            self.multi.clear();
        }
        if response.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            let ctrl = ui.input(|i| i.modifiers.ctrl);
            if scroll != 0.0 {
                if ctrl {
                    let old = z;
                    let factor = if scroll > 0.0 { 1.1 } else { 0.9 };
                    let new = (old * factor).clamp(0.1, 4.0);
                    // Keep the world point under the pointer where it is:
                    // the new origin must satisfy p - origin' = (p - origin)
                    // * new / old, and pan is origin minus the centred base.
                    let pivot = pointer.unwrap_or(rect.center());
                    let origin_new = pivot - (pivot - origin) * (new / old);
                    let base = rect.center() - egui::vec2(scene.size.w, scene.size.h) * (new / 2.0);
                    self.pan = origin_new - base;
                    self.zoom = new;
                } else {
                    // Fit mode (zoom 0) forces pan to zero every frame, so
                    // scrolling did nothing until the first Ctrl+wheel:
                    // materialise the fit first.
                    if self.zoom == 0.0 {
                        self.zoom = z;
                        self.pan = pan;
                    }
                    self.pan.y += scroll;
                }
            }
            // The properties panel's text fields have their own Delete and
            // Ctrl+Z; only act when no widget is taking the keyboard.
            if !ui.ctx().egui_wants_keyboard_input() {
                if ui.input(|i| i.key_pressed(egui::Key::Delete)) {
                    self.delete_selected();
                }
                if ui.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::Z)) {
                    self.undo();
                }
                if ui.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::Y)) {
                    self.redo();
                }
                if ui.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::S)) {
                    self.save(cx);
                }
            }
        }
        for id in self.selection() {
            if let Some(i) = self.doc.nodes.iter().position(|n| n.id == id)
                && i < node_rects.len()
            {
                let r = srect(node_rects[i]).expand(3.0);
                painter.rect_stroke(
                    r,
                    4.0,
                    egui::Stroke::new(2.0, theme.accent),
                    egui::StrokeKind::Outside,
                );
                if self.selected.as_deref() == Some(id.as_str()) {
                    painter.rect_filled(
                        egui::Rect::from_center_size(r.right_bottom(), egui::vec2(9.0, 9.0)),
                        1.0,
                        theme.accent,
                    );
                }
            }
        }
        if let Some(pos) = self
            .selected_edge
            .as_ref()
            .and_then(|id| self.doc.edges.iter().position(|e| &e.id == id))
        {
            let edge = &self.doc.edges[pos];
            if !edge.label.is_empty() {
                // The layout already resolved this edge's label position
                // (label_at override, or the midpoint of its real routed
                // path) when it built the scene - find that exact Prim by
                // edge id instead of re-deriving a position from raw
                // node.x/y (stale once a diagram is auto-laid-out) or from a
                // positional "nth labelled edge" count (desynced as soon as
                // any earlier edge's endpoints fail to resolve).
                let at = scene.prims.iter().find_map(|p| match p {
                    Prim::Text {
                        at,
                        boxed: true,
                        edge_id: Some(id),
                        ..
                    } if id == &edge.id => Some(*at),
                    _ => None,
                });
                if let Some(at) = at {
                    // Match the real label box exactly (same font size and
                    // padding as paint_scene's Prim::Text rendering), so the
                    // drag handle never drifts from what the user sees.
                    let galley = ui.painter().layout_no_wrap(
                        edge.label.clone(),
                        egui::FontId::proportional(12.0 * z),
                        theme.text,
                    );
                    let size = galley.size() + egui::vec2(10.0 * z, 4.0 * z);
                    let rect = egui::Rect::from_center_size(to_screen(at), size);
                    let r = ui.interact(
                        rect,
                        ui.id().with(("edge-label", &edge.id)),
                        egui::Sense::drag(),
                    );
                    painter.rect_filled(rect, 4.0, theme.accent.gamma_multiply(0.06));
                    painter.rect_stroke(
                        rect,
                        4.0,
                        egui::Stroke::new(
                            if r.hovered() || r.dragged() { 2.0 } else { 1.0 },
                            theme.accent,
                        ),
                        egui::StrokeKind::Outside,
                    );
                    painter.circle_filled(rect.right_top(), 4.0, theme.accent);
                    if r.drag_started() {
                        self.push_undo();
                    }
                    if r.dragged()
                        && let Some(p) = ui.input(|i| i.pointer.interact_pos())
                    {
                        let at = Point {
                            x: (p.x - origin.x) / z,
                            y: (p.y - origin.y) / z,
                        };
                        if let Some(edge) = self.doc.edges.get_mut(pos) {
                            edge.label_at = Some(at);
                        }
                        self.dirty = true;
                    }
                }
            }
        }
        if let Some(eid) = &self.selected_edge
            && let Some(pos) = self.doc.edges.iter().position(|e| &e.id == eid)
            && let Some(path) = self.edge_paths.get(pos)
        {
            let points: Vec<egui::Pos2> = path.iter().map(|p| to_screen(*p)).collect();
            if points.len() >= 2 {
                painter.add(egui::Shape::line(
                    points,
                    egui::Stroke::new(3.0, theme.accent),
                ));
            }
        }
        if let Some(eid) = &self.selected_edge
            && let Some(edge) = self.doc.edges.iter().find(|e| &e.id == eid)
        {
            for via in &edge.via {
                let p = to_screen(*via);
                painter.circle_filled(p, 5.0, theme.panel_bg);
                painter.circle_stroke(p, 5.0, egui::Stroke::new(2.0, theme.accent));
            }
        }
        if let Some(gid) = &self.selected_group {
            for prim in &scene.prims {
                if let Prim::Frame { rect, group_id, .. } = prim
                    && group_id.as_deref() == Some(gid.as_str())
                {
                    painter.rect_stroke(
                        srect(*rect).expand(2.0),
                        8.0,
                        egui::Stroke::new(2.0, theme.accent),
                        egui::StrokeKind::Outside,
                    );
                }
            }
        }
        // Keep "add at the viewport centre" honest across panning.
        self.last_center = Point {
            x: (rect.center().x - origin.x) / z,
            y: (rect.center().y - origin.y) / z,
        };
        self.status = Some(format!(
            "{}  ·  {:.0}%",
            if self.edge_from.is_some() {
                cx.services.t("gfx-canvas-pick-target")
            } else {
                cx.services.t("gfx-canvas-hint")
            },
            z * 100.0
        ));
    }

    fn paint_grid(
        &self,
        painter: &egui::Painter,
        rect: egui::Rect,
        origin: egui::Pos2,
        z: f32,
        theme: &Theme,
    ) {
        let step = 20.0 * z;
        if step < 6.0 {
            return;
        }
        let stroke = egui::Stroke::new(1.0, theme.border.linear_multiply(0.5));
        let mut x = origin.x - ((origin.x - rect.left()) / step).floor() * step;
        while x < rect.right() {
            painter.line_segment(
                [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                stroke,
            );
            x += step;
        }
        let mut y = origin.y - ((origin.y - rect.top()) / step).floor() * step;
        while y < rect.bottom() {
            painter.line_segment(
                [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
                stroke,
            );
            y += step;
        }
    }

    pub(crate) fn paint_scene(
        painter: &egui::Painter,
        scene: &Scene,
        _to_screen: &dyn Fn(Point) -> egui::Pos2,
        srect: &dyn Fn(Rect) -> egui::Rect,
        z: f32,
        theme: &Theme,
    ) {
        for prim in &scene.prims {
            match prim {
                Prim::Frame {
                    rect,
                    label,
                    style,
                    label_placement,
                    label_align,
                    ..
                } => {
                    let fill = style
                        .fill
                        .as_deref()
                        .and_then(parse_hex)
                        .unwrap_or(egui::Color32::TRANSPARENT);
                    let stroke = style
                        .stroke
                        .as_deref()
                        .and_then(parse_hex)
                        .unwrap_or(theme.text_muted);
                    let label_color = style
                        .text
                        .as_deref()
                        .and_then(parse_hex)
                        .unwrap_or(theme.text_muted);
                    painter.rect(
                        srect(*rect),
                        8.0 * z,
                        fill,
                        egui::Stroke::new(1.2, stroke),
                        egui::StrokeKind::Inside,
                    );
                    if !label.is_empty() {
                        let r = srect(*rect);
                        let (pos, anchor) = match label_align {
                            hs_diagram::model::LabelAlign::Left => (
                                egui::pos2(r.left() + 6.0, r.top() + 4.0),
                                egui::Align2::LEFT_TOP,
                            ),
                            hs_diagram::model::LabelAlign::Centre => (
                                egui::pos2(r.center().x, r.top() + 4.0),
                                egui::Align2::CENTER_TOP,
                            ),
                            hs_diagram::model::LabelAlign::Right => (
                                egui::pos2(r.right() - 6.0, r.top() + 4.0),
                                egui::Align2::RIGHT_TOP,
                            ),
                        };
                        let pos = match label_placement {
                            hs_diagram::model::LabelPlacement::Top => pos,
                            hs_diagram::model::LabelPlacement::Bottom => {
                                egui::pos2(pos.x, r.bottom() - 20.0)
                            }
                        };
                        painter.text(
                            pos,
                            anchor,
                            label,
                            egui::FontId::proportional((12.0 * z).max(9.0)),
                            label_color,
                        );
                    }
                }
                Prim::Shape {
                    rect,
                    kind,
                    label,
                    style,
                } => {
                    let r = srect(*rect);
                    let fill = color(
                        style
                            .fill
                            .as_deref()
                            .unwrap_or(hs_diagram::scene::default_fill(*kind)),
                    );
                    let stroke = egui::Stroke::new(
                        (1.4 * z).max(1.0),
                        color(
                            style
                                .stroke
                                .as_deref()
                                .unwrap_or(hs_diagram::scene::default_stroke(*kind)),
                        ),
                    );
                    paint_shape(painter, r, *kind, fill, stroke, z);
                    if !label.is_empty() {
                        // No explicit text colour: pick the one that reads on
                        // the fill, so a dark fill never hides its label.
                        let text_color = match style.text.as_deref() {
                            Some(t) => color(t),
                            None => contrast_text(fill),
                        };
                        if *kind == NodeKind::Entity {
                            let lines: Vec<&str> = label.lines().collect();
                            let header = (28.0 * z).min(r.height() * 0.45);
                            if let Some(name) = lines.first() {
                                painter.text(
                                    egui::pos2(r.center().x, r.top() + header / 2.0),
                                    egui::Align2::CENTER_CENTER,
                                    name,
                                    egui::FontId::proportional((13.0 * z).max(9.0)),
                                    text_color,
                                );
                            }
                            for (i, line) in lines.iter().skip(1).enumerate() {
                                let y = r.top() + header + 10.0 * z + i as f32 * 16.0 * z;
                                if y + 4.0 > r.bottom() {
                                    break;
                                }
                                painter.text(
                                    egui::pos2(r.left() + 8.0 * z, y),
                                    egui::Align2::LEFT_CENTER,
                                    line,
                                    egui::FontId::monospace((11.0 * z).max(8.0)),
                                    text_color,
                                );
                            }
                        } else {
                            let size = (style.font_size.unwrap_or(14.0) * z).max(8.0);
                            let lines: Vec<&str> = label.lines().collect();
                            let lh = size * 1.25;
                            let top = r.center().y - (lines.len() as f32 - 1.0) * lh / 2.0;
                            for (i, line) in lines.iter().enumerate() {
                                painter.text(
                                    egui::pos2(r.center().x, top + i as f32 * lh),
                                    egui::Align2::CENTER_CENTER,
                                    line,
                                    egui::FontId::proportional(size),
                                    text_color,
                                );
                            }
                        }
                    }
                }
                Prim::Line {
                    points,
                    dashed,
                    dotted,
                    arrow_start,
                    arrow_end,
                    style,
                    ..
                } => {
                    if points.len() < 2 {
                        continue;
                    }
                    let pts: Vec<egui::Pos2> = points.iter().map(|p| _to_screen(*p)).collect();
                    let stroke = egui::Stroke::new(
                        (1.5 * z).max(1.0),
                        color(style.stroke.as_deref().unwrap_or("#5b6472")),
                    );
                    if *dotted {
                        dashed_polyline(painter, &pts, stroke, z.max(1.0), 4.0 * z);
                    } else if *dashed || style.dashed {
                        dashed_polyline(painter, &pts, stroke, 6.0 * z, 4.0 * z);
                    } else {
                        painter.add(egui::Shape::line(pts.clone(), stroke));
                    }
                    if *arrow_end {
                        arrow_head(painter, &pts, stroke, z, false);
                    }
                    if *arrow_start {
                        arrow_head(painter, &pts, stroke, z, true);
                    }
                }
                Prim::Text {
                    at,
                    label,
                    boxed,
                    style,
                    ..
                } => {
                    let pos = _to_screen(*at);
                    let size = (style.font_size.unwrap_or(12.0) * z).max(8.0);
                    let label_color = color(style.text.as_deref().unwrap_or("#1f2328"));
                    let galley = painter.layout_no_wrap(
                        label.clone(),
                        egui::FontId::proportional(size),
                        label_color,
                    );
                    if *boxed {
                        let r = egui::Rect::from_center_size(
                            pos,
                            galley.size() + egui::vec2(10.0 * z, 4.0 * z),
                        );
                        painter.rect_filled(r, 4.0 * z, egui::Color32::WHITE);
                        painter.rect_stroke(
                            r,
                            4.0 * z,
                            egui::Stroke::new(1.0, theme.border),
                            egui::StrokeKind::Inside,
                        );
                    }
                    painter.galley(pos - galley.size() / 2.0, galley, label_color);
                }
            }
        }
    }

    fn hit(&self, pos: egui::Pos2, rects: &[Rect], origin: egui::Pos2, z: f32) -> Option<usize> {
        let world = Point {
            x: (pos.x - origin.x) / z,
            y: (pos.y - origin.y) / z,
        };
        rects.iter().rposition(|r| r.contains(world))
    }

    pub(crate) fn replace(&mut self, doc: DiagramDoc) {
        self.push_undo();
        self.doc = doc;
        self.selected = self.doc.nodes.first().map(|n| n.id.clone());
        self.selected_edge = None;
        self.selected_group = None;
        self.clear_selection();
        self.dirty = true;
        self.zoom = 0.0;
        self.pan = egui::Vec2::ZERO;
    }

    fn open_dialog(&mut self, cx: &mut ToolContext) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("HyperStack diagram", &["hdiag", "diagram.json"])
            .pick_file()
        else {
            return;
        };
        match Self::open_path(&path) {
            Ok(mut state) => {
                state.path = Some(path);
                *self = state;
                cx.toast(ToastLevel::Success, cx.services.t("gfx-diagram-opened"));
            }
            Err(e) => cx.toast(ToastLevel::Error, e),
        }
    }

    fn import(&mut self, cx: &mut ToolContext, kind: ImportKind) {
        let (title, filter) = match kind {
            ImportKind::Archify => (cx.services.t("gfx-filter-archify"), "json"),
            ImportKind::DrawIo => (cx.services.t("gfx-filter-drawio"), "drawio"),
            ImportKind::Mermaid => (cx.services.t("gfx-filter-mermaid"), "mmd"),
        };
        let Some(path) = rfd::FileDialog::new()
            .add_filter(&title, &[filter])
            .pick_file()
        else {
            return;
        };
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) => {
                cx.toast(ToastLevel::Error, e.to_string());
                return;
            }
        };
        if kind == ImportKind::DrawIo {
            match hs_diagram::import::drawio_pages(&text) {
                Ok(pages) if pages.len() > 1 => {
                    self.drawio_import = Some((text, pages, 0));
                    return;
                }
                Ok(_) => {}
                Err(e) => {
                    cx.toast(ToastLevel::Error, e);
                    return;
                }
            }
        }
        let imported = match kind {
            ImportKind::Archify => hs_diagram::import::archify_json(&text),
            ImportKind::DrawIo => hs_diagram::import::drawio_xml(&text),
            ImportKind::Mermaid => hs_diagram::import::mermaid_sequence(&text),
        };
        match imported {
            Ok(doc) => {
                self.replace(doc);
                self.path = None;
                self.status = Some(format!("Imported {}", path.display()));
            }
            Err(e) => {
                cx.toast(ToastLevel::Error, e);
            }
        }
    }

    fn save(&mut self, cx: &mut ToolContext) {
        if self.path.is_none() {
            let stem = sanitise(&self.doc.title);
            let Some(path) = rfd::FileDialog::new()
                .add_filter("HyperStack diagram", &["hdiag"])
                .set_file_name(format!("{stem}.hdiag"))
                .save_file()
            else {
                return;
            };
            self.path = Some(path);
        }
        let Some(path) = self.path.clone() else {
            return;
        };
        match hs_diagram::io::save(&path, &self.doc) {
            Ok(()) => {
                self.dirty = false;
                self.saved = Some(path.clone());
                cx.toast(
                    ToastLevel::Success,
                    cx.services
                        .i18n
                        .t_args("gfx-saved", &[("path", &path.display().to_string())]),
                );
            }
            Err(e) => cx.toast(ToastLevel::Error, e),
        }
    }

    fn export(&mut self, cx: &mut ToolContext, kind: ExportKind) {
        let stem = sanitise(&self.doc.title);
        let (filter, ext) = kind.filter();
        let Some(path) = rfd::FileDialog::new()
            .add_filter(filter, &[ext])
            .set_file_name(format!("{stem}.{ext}"))
            .save_file()
        else {
            return;
        };
        let scene = layout_approx(&self.doc);
        let bytes = match kind {
            ExportKind::Png => png(
                &scene,
                &PngOptions {
                    scale: 2.0,
                    background: Some("#ffffff".into()),
                },
            ),
            ExportKind::Svg => Ok(svg(&scene, Some("#ffffff")).into_bytes()),
            ExportKind::Pdf => pdf(&scene, Some("#ffffff")),
        };
        match bytes.and_then(|b| std::fs::write(&path, b).map_err(|e| e.to_string())) {
            Ok(()) => cx.toast(
                ToastLevel::Success,
                cx.services
                    .i18n
                    .t_args("gfx-saved", &[("path", &path.display().to_string())]),
            ),
            Err(e) => cx.toast(ToastLevel::Error, e),
        }
    }
}

#[derive(Clone, Copy)]
enum ExportKind {
    Png,
    Svg,
    Pdf,
}

impl ExportKind {
    fn filter(self) -> (&'static str, &'static str) {
        match self {
            Self::Png => ("PNG image", "png"),
            Self::Svg => ("SVG image", "svg"),
            Self::Pdf => ("PDF document", "pdf"),
        }
    }
}

fn point_to_segment(p: egui::Pos2, a: egui::Pos2, b: egui::Pos2) -> f32 {
    let ab = b - a;
    let len = ab.length_sq();
    if len <= f32::EPSILON {
        return (p - a).length();
    }
    let t = ((p - a).dot(ab) / len).clamp(0.0, 1.0);
    (p - (a + ab * t)).length()
}

pub(crate) fn sanitise(title: &str) -> String {
    let mut out: String = title
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    while out.contains("--") {
        out = out.replace("--", "-");
    }
    let out = out.trim_matches('-').to_string();
    if out.is_empty() {
        "diagram".into()
    } else {
        out
    }
}

/// A cheap, complete change detector for a document: its serialisation
/// streamed straight into a hasher (no string is built). Used to skip the
/// layout and the draft autosave on frames where nothing changed.
pub(crate) fn fingerprint(doc: &DiagramDoc) -> u64 {
    use std::hash::Hasher;
    struct HashWriter(std::collections::hash_map::DefaultHasher);
    impl std::io::Write for HashWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.write(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut w = HashWriter(std::collections::hash_map::DefaultHasher::new());
    // The document always serialises; a failure here would mean a bug in
    // the model, and hashing whatever was written is still deterministic.
    let _ = serde_json::to_writer(&mut w, doc);
    w.0.finish()
}

/// Section heading inside the properties pane: one weight and one spacing
/// for Diagram, Node, Edge, Frame, Fragments, Outline and Status.
fn section_title(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    ui.label(egui::RichText::new(text).strong().color(theme.text));
    ui.add_space(4.0);
}

/// A muted sub-heading for a block inside a section (Templates, Columns).
fn props_label_inline(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    ui.label(egui::RichText::new(text).small().color(theme.text_muted));
    ui.add_space(2.0);
}

/// The two-column label/control grid every properties section uses, so
/// labels line up in one column and controls in another whatever the
/// section. The label column is narrow: the pane is 240-430 px wide.
fn props_grid<R>(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    egui::Grid::new(id)
        .num_columns(2)
        .spacing([10.0, 6.0])
        .min_col_width(72.0)
        .show(ui, add)
        .inner
}

fn props_label(ui: &mut egui::Ui, text: &str) {
    ui.add(egui::Label::new(text).selectable(false));
}

/// Top/Bottom placement as a selectable pair (a grid cell).
fn placement_picker(
    ui: &mut egui::Ui,
    s: &Services,
    placement: &mut hs_diagram::model::LabelPlacement,
) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        for (value, key) in [
            (hs_diagram::model::LabelPlacement::Top, "gfx-place-top"),
            (
                hs_diagram::model::LabelPlacement::Bottom,
                "gfx-place-bottom",
            ),
        ] {
            if ui.selectable_label(*placement == value, s.t(key)).clicked() {
                *placement = value;
                changed = true;
            }
        }
    });
    changed
}

/// Left/Middle/Right alignment as a selectable trio (a grid cell).
fn align_picker(
    ui: &mut egui::Ui,
    s: &Services,
    align: &mut hs_diagram::model::LabelAlign,
) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        for (value, key) in [
            (hs_diagram::model::LabelAlign::Left, "gfx-text-left"),
            (hs_diagram::model::LabelAlign::Centre, "gfx-text-middle"),
            (hs_diagram::model::LabelAlign::Right, "gfx-text-right"),
        ] {
            if ui.selectable_label(*align == value, s.t(key)).clicked() {
                *align = value;
                changed = true;
            }
        }
    });
    changed
}

/// A colour swatch with a Reset beside it, for one grid cell.
fn color_cell(ui: &mut egui::Ui, s: &Services, value: &mut Option<String>, changed: &mut bool) {
    let mut color = value
        .as_deref()
        .and_then(parse_hex)
        .unwrap_or(egui::Color32::from_rgb(0x88, 0x94, 0xa6));
    ui.horizontal(|ui| {
        if ui.color_edit_button_srgba(&mut color).changed() {
            *value = Some(format!(
                "#{:02x}{:02x}{:02x}",
                color.r(),
                color.g(),
                color.b()
            ));
            *changed = true;
        }
        if ui
            .add_enabled(
                value.is_some(),
                egui::Button::new(s.t("gfx-colour-reset")).small(),
            )
            .clicked()
        {
            *value = None;
            *changed = true;
        }
    });
}

/// Dark text on a light fill, light text on a dark one (relative luminance).
fn contrast_text(fill: egui::Color32) -> egui::Color32 {
    let lum = 0.2126 * fill.r() as f32 + 0.7152 * fill.g() as f32 + 0.0722 * fill.b() as f32;
    // A transparent fill shows the canvas, which is dark in the default theme.
    if fill.a() < 64 || lum < 140.0 {
        egui::Color32::from_rgb(0xf3, 0xf4, 0xf6)
    } else {
        egui::Color32::from_rgb(0x1f, 0x23, 0x28)
    }
}

fn parse_hex(text: &str) -> Option<egui::Color32> {
    let hex = text.trim().trim_start_matches('#');
    if hex.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some(egui::Color32::from_rgb(r, g, b))
}
fn shape_label(kind: NodeKind, s: &hs_core::Services) -> String {
    let key = match kind {
        NodeKind::Process => "gfx-shape-process",
        NodeKind::RoundedRectangle => "gfx-shape-rounded",
        NodeKind::Ellipse => "gfx-shape-ellipse",
        NodeKind::Decision => "gfx-shape-decision",
        NodeKind::StartEnd => "gfx-shape-start",
        NodeKind::InputOutput => "gfx-shape-io",
        NodeKind::Database => "gfx-shape-database",
        NodeKind::Note => "gfx-shape-note",
        NodeKind::Text => "gfx-shape-text",
        NodeKind::Container => "gfx-shape-container",
        NodeKind::Entity => "gfx-shape-entity",
        NodeKind::Frontend => "gfx-shape-frontend",
        NodeKind::Backend => "gfx-shape-backend",
        NodeKind::Store => "gfx-shape-store",
        NodeKind::Queue => "gfx-shape-queue",
        NodeKind::External => "gfx-shape-external",
    };
    s.t(key)
}

fn color(hex: &str) -> egui::Color32 {
    let hex = hex.trim_start_matches('#');
    let n = u32::from_str_radix(hex, 16).unwrap_or(0x1f2328);
    let (r, g, b) = if hex.len() >= 6 {
        ((n >> 16) as u8, (n >> 8) as u8, n as u8)
    } else {
        (31, 35, 40)
    };
    egui::Color32::from_rgb(r, g, b)
}

fn paint_shape(
    painter: &egui::Painter,
    r: egui::Rect,
    kind: NodeKind,
    fill: egui::Color32,
    stroke: egui::Stroke,
    z: f32,
) {
    match kind {
        NodeKind::Text => {}
        NodeKind::Ellipse => {
            painter.add(egui::Shape::ellipse_filled(
                r.center(),
                r.size() / 2.0,
                fill,
            ));
            painter.add(egui::Shape::ellipse_stroke(
                r.center(),
                r.size() / 2.0,
                stroke,
            ));
        }
        NodeKind::Decision => {
            let points = vec![
                egui::pos2(r.center().x, r.top()),
                egui::pos2(r.right(), r.center().y),
                egui::pos2(r.center().x, r.bottom()),
                egui::pos2(r.left(), r.center().y),
            ];
            painter.add(egui::Shape::convex_polygon(points, fill, stroke));
        }
        NodeKind::StartEnd => {
            painter.rect(r, r.height() / 2.0, fill, stroke, egui::StrokeKind::Inside);
        }
        NodeKind::InputOutput => {
            let skew = (14.0 * z).min(r.width() * 0.2);
            let points = vec![
                egui::pos2(r.left() + skew, r.top()),
                egui::pos2(r.right(), r.top()),
                egui::pos2(r.right() - skew, r.bottom()),
                egui::pos2(r.left(), r.bottom()),
            ];
            painter.add(egui::Shape::convex_polygon(points, fill, stroke));
        }
        NodeKind::Database => {
            // A real cylinder: body, full top ellipse and only the *front*
            // half of the bottom ellipse. A full bottom ellipse reads as an
            // oval underneath the shape.
            let ry = (r.height() * 0.12).min(12.0 * z).max(4.0);
            let rx = r.width() / 2.0;
            let top = egui::pos2(r.center().x, r.top() + ry);
            let bottom = egui::pos2(r.center().x, r.bottom() - ry);
            let steps = 16;
            let mut body = vec![
                egui::pos2(r.left(), r.top() + ry),
                egui::pos2(r.right(), r.top() + ry),
                egui::pos2(r.right(), r.bottom() - ry),
            ];
            for i in 0..=steps {
                let angle = std::f32::consts::PI * (i as f32 / steps as f32);
                body.push(egui::pos2(
                    bottom.x + rx * angle.cos(),
                    bottom.y + ry * angle.sin(),
                ));
            }
            body.push(egui::pos2(r.left(), r.bottom() - ry));
            painter.add(egui::Shape::convex_polygon(body, fill, egui::Stroke::NONE));
            painter.add(egui::Shape::ellipse_filled(top, egui::vec2(rx, ry), fill));
            painter.add(egui::Shape::ellipse_stroke(top, egui::vec2(rx, ry), stroke));
            painter.line_segment(
                [
                    egui::pos2(r.left(), r.top() + ry),
                    egui::pos2(r.left(), r.bottom() - ry),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    egui::pos2(r.right(), r.top() + ry),
                    egui::pos2(r.right(), r.bottom() - ry),
                ],
                stroke,
            );
            let arc: Vec<egui::Pos2> = (0..=steps)
                .map(|i| {
                    let angle = std::f32::consts::PI * (i as f32 / steps as f32);
                    egui::pos2(bottom.x + rx * angle.cos(), bottom.y + ry * angle.sin())
                })
                .collect();
            painter.add(egui::Shape::line(arc, stroke));
        }
        NodeKind::Note => {
            painter.rect(r, 0.0, fill, stroke, egui::StrokeKind::Inside);
            let fold = (16.0 * z).min(r.width() * 0.25).min(r.height() * 0.25);
            painter.add(egui::Shape::convex_polygon(
                vec![
                    egui::pos2(r.right() - fold, r.top()),
                    egui::pos2(r.right(), r.top() + fold),
                    egui::pos2(r.right() - fold, r.top() + fold),
                ],
                stroke.color,
                egui::Stroke::NONE,
            ));
        }
        NodeKind::Container => {
            painter.rect(
                r,
                10.0 * z,
                egui::Color32::TRANSPARENT,
                stroke,
                egui::StrokeKind::Inside,
            );
        }
        NodeKind::Entity => {
            painter.rect(r, 6.0 * z, fill, stroke, egui::StrokeKind::Inside);
            let header = (28.0 * z).min(r.height() * 0.45);
            painter.rect_filled(
                egui::Rect::from_min_max(r.min, egui::pos2(r.max.x, r.min.y + header)),
                6.0 * z,
                stroke.color.gamma_multiply(0.14),
            );
            painter.line_segment(
                [
                    egui::pos2(r.left(), r.top() + header),
                    egui::pos2(r.right(), r.top() + header),
                ],
                stroke,
            );
        }
        _ => {
            let radius = if kind == NodeKind::RoundedRectangle {
                14.0 * z
            } else {
                6.0 * z
            };
            painter.rect(r, radius, fill, stroke, egui::StrokeKind::Inside);
        }
    }
}

fn dashed_polyline(
    painter: &egui::Painter,
    points: &[egui::Pos2],
    stroke: egui::Stroke,
    dash: f32,
    gap: f32,
) {
    let dash = dash.max(1.0);
    let gap = gap.max(1.0);
    for pair in points.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let length = (b - a).length();
        if length <= f32::EPSILON {
            continue;
        }
        let dir = (b - a) / length;
        let mut t = 0.0;
        while t < length {
            let end = (t + dash).min(length);
            painter.line_segment([a + dir * t, a + dir * end], stroke);
            t = end + gap;
        }
    }
}
fn arrow_head(
    painter: &egui::Painter,
    points: &[egui::Pos2],
    stroke: egui::Stroke,
    z: f32,
    reverse: bool,
) {
    let (a, b) = if reverse {
        (points[1], points[0])
    } else {
        (points[points.len() - 2], points[points.len() - 1])
    };
    let dir = (b - a).normalized();
    let n = egui::vec2(-dir.y, dir.x);
    let len = (10.0 * z).max(5.0);
    let half = (5.0 * z).max(3.0);
    let base = b - dir * len;
    painter.add(egui::Shape::convex_polygon(
        vec![b, base + n * half, base - n * half],
        stroke.color,
        egui::Stroke::NONE,
    ));
}

fn shape_icon(kind: NodeKind) -> &'static str {
    use hs_ui::icons::ph;
    match kind {
        NodeKind::Decision => ph::DIAMOND,
        NodeKind::StartEnd | NodeKind::Ellipse => ph::SELECTION,
        NodeKind::Note | NodeKind::Text => ph::NOTE_PENCIL,
        NodeKind::Container => ph::TREE_STRUCTURE,
        NodeKind::Entity => ph::TABLE,
        NodeKind::Database | NodeKind::Store => ph::RECTANGLE,
        NodeKind::Queue => ph::ARROW_RIGHT,
        NodeKind::Frontend | NodeKind::Backend | NodeKind::External => ph::APP_WINDOW,
        NodeKind::Process | NodeKind::RoundedRectangle | NodeKind::InputOutput => ph::RECTANGLE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn freezing_layout_gives_nodes_geometry() {
        let mut state = DiagramState::new(DiagramKind::Flow);
        assert!(state.doc.auto_layout);
        assert!(state.freeze());
        assert!(!state.doc.auto_layout);
        assert!(state.doc.nodes.iter().all(|n| n.w > 0.0 && n.h > 0.0));
    }

    #[test]
    fn add_and_undo_restore_the_document() {
        let mut state = DiagramState::new(DiagramKind::Flow);
        let before = state.doc.nodes.len();
        state.add_node_at(NodeKind::Process, Point { x: 300.0, y: 200.0 });
        assert_eq!(state.doc.nodes.len(), before + 1);
        assert!(!state.doc.auto_layout);
        state.undo();
        assert_eq!(state.doc.nodes.len(), before);
    }

    #[test]
    fn file_names_are_sanitised() {
        assert_eq!(sanitise("Checkout flow"), "Checkout-flow");
        assert_eq!(sanitise("../"), "diagram");
        assert_eq!(sanitise("A  B"), "A-B");
    }

    #[test]
    fn open_path_reads_a_saved_document() {
        let dir = std::env::temp_dir().join(format!("hs-gfx-diagram-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("flow.hdiag");
        hs_diagram::io::save(&path, &hs_diagram::template::flow()).unwrap();
        let state = DiagramState::open_path(&path).unwrap();
        assert_eq!(state.doc.title, "Checkout flow");
        assert_eq!(state.path.as_deref(), Some(path.as_path()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn adding_a_waypoint_keeps_the_edge_and_adds_a_point() {
        let mut state = DiagramState::from_doc(hs_diagram::template::flow());
        state.doc.auto_layout = false;
        let id = state.doc.edges[0].id.clone();
        let before = state.doc.edges[0].via.len();
        state.add_edge_waypoint(&id);
        assert_eq!(state.doc.edges[0].via.len(), before + 1);
        state.selected_edge = Some(id.clone());
        state.delete_selected();
        assert!(state.doc.edges.iter().all(|e| e.id != id));
    }

    #[test]
    fn align_left_moves_the_selected_nodes() {
        let mut state = DiagramState::from_doc(hs_diagram::template::flow());
        state.doc.auto_layout = false;
        assert!(state.doc.nodes.len() >= 2);
        state.doc.nodes[0].x = 100.0;
        state.doc.nodes[1].x = 300.0;
        let a = state.doc.nodes[0].id.clone();
        let b = state.doc.nodes[1].id.clone();
        state.selected = Some(a.clone());
        state.multi = vec![b.clone()];
        state.align_selection(AlignMode::Left);
        let ax = state.doc.node(&a).unwrap().x;
        let bx = state.doc.node(&b).unwrap().x;
        assert_eq!(ax, bx);
        assert_eq!(ax, 100.0);
    }

    #[test]
    fn the_edge_tool_connects_two_frames_and_a_frame_to_a_node() {
        let mut state = DiagramState::from_doc(hs_diagram::template::flow());
        assert!(state.freeze());
        let ids: Vec<String> = state.doc.nodes.iter().map(|n| n.id.clone()).collect();
        assert!(ids.len() >= 3);
        for (gid, members) in [("g1", &ids[0..1]), ("g2", &ids[1..2])] {
            state.doc.groups.push(Group {
                id: gid.into(),
                label: gid.into(),
                nodes: members.to_vec(),
                style: Style::default(),
                label_placement: Default::default(),
                label_align: Default::default(),
            });
        }
        let before = state.doc.edges.len();
        state.tool = ToolMode::Edge;
        state.edge_endpoint_clicked("g1".into());
        assert_eq!(state.edge_from.as_deref(), Some("g1"));
        state.edge_endpoint_clicked("g2".into());
        assert_eq!(state.doc.edges.len(), before + 1);
        let e = state.doc.edges.last().unwrap();
        assert_eq!((e.from.as_str(), e.to.as_str()), ("g1", "g2"));
        assert_eq!(state.selected_edge.as_deref(), Some(e.id.as_str()));
        // Clicking the same frame twice adds nothing.
        state.edge_endpoint_clicked("g2".into());
        state.edge_endpoint_clicked("g2".into());
        assert_eq!(state.doc.edges.len(), before + 1);
        // Frame to node, and the document still validates.
        state.edge_endpoint_clicked("g1".into());
        state.edge_endpoint_clicked(ids[2].clone());
        assert_eq!(state.doc.edges.len(), before + 2);
        let errors: Vec<_> = validate::validate(&state.doc)
            .into_iter()
            .filter(|i| i.severity == hs_diagram::Severity::Error)
            .collect();
        assert!(errors.is_empty(), "{errors:?}");
        // The layout draws a line for each, found by id.
        let scene = layout_approx(&state.doc);
        for e in &state.doc.edges[before..] {
            assert!(scene.prims.iter().any(|p| matches!(
                p,
                Prim::Line { edge_id: Some(id), points, .. } if id == &e.id && points.len() >= 2
            )));
        }
    }

    #[test]
    fn label_text_contrasts_with_the_fill() {
        let dark = egui::Color32::from_rgb(0x1f, 0x23, 0x28);
        let light = egui::Color32::from_rgb(0xf3, 0xf4, 0xf6);
        assert_eq!(contrast_text(egui::Color32::WHITE), dark);
        assert_eq!(
            contrast_text(egui::Color32::from_rgb(0x1e, 0x29, 0x3b)),
            light
        );
        // Transparent shows the (dark) canvas, so the text goes light.
        assert_eq!(contrast_text(egui::Color32::TRANSPARENT), light);
    }

    #[test]
    fn align_freezes_auto_layout_so_the_move_sticks() {
        // Without freezing first, the layout engine recomputes positions from
        // the graph every frame while auto_layout is on and silently discards
        // whatever align just wrote - this is what made Align/Distribute look
        // like it "sometimes does nothing" on a diagram nobody has dragged yet.
        let mut state = DiagramState::from_doc(hs_diagram::template::flow());
        assert!(state.doc.auto_layout);
        let a = state.doc.nodes[0].id.clone();
        let b = state.doc.nodes[1].id.clone();
        state.selected = Some(a.clone());
        state.multi = vec![b.clone()];
        state.align_selection(AlignMode::Left);
        assert!(!state.doc.auto_layout);
    }

    #[test]
    fn align_right_top_bottom_and_centre_use_the_far_bound() {
        let mut state = DiagramState::from_doc(hs_diagram::template::flow());
        state.doc.auto_layout = false;
        assert!(state.doc.nodes.len() >= 2);
        state.doc.nodes[0].x = 100.0;
        state.doc.nodes[0].w = 40.0;
        state.doc.nodes[0].y = 50.0;
        state.doc.nodes[0].h = 20.0;
        state.doc.nodes[1].x = 300.0;
        state.doc.nodes[1].w = 60.0;
        state.doc.nodes[1].y = 150.0;
        state.doc.nodes[1].h = 30.0;
        let a = state.doc.nodes[0].id.clone();
        let b = state.doc.nodes[1].id.clone();
        state.selected = Some(a.clone());
        state.multi = vec![b.clone()];

        state.align_selection(AlignMode::Right);
        let right_of = |s: &DiagramState, id: &str| {
            let n = s.doc.node(id).unwrap();
            n.x + n.w
        };
        assert_eq!(right_of(&state, &a), right_of(&state, &b));
        assert_eq!(right_of(&state, &a), 360.0);

        state.align_selection(AlignMode::Top);
        assert_eq!(state.doc.node(&a).unwrap().y, state.doc.node(&b).unwrap().y);
        assert_eq!(state.doc.node(&a).unwrap().y, 50.0);

        state.align_selection(AlignMode::Bottom);
        let bottom_of = |s: &DiagramState, id: &str| {
            let n = s.doc.node(id).unwrap();
            n.y + n.h
        };
        assert_eq!(bottom_of(&state, &a), bottom_of(&state, &b));

        state.align_selection(AlignMode::CentreX);
        let centre_x = |s: &DiagramState, id: &str| {
            let n = s.doc.node(id).unwrap();
            n.x + n.w / 2.0
        };
        assert!((centre_x(&state, &a) - centre_x(&state, &b)).abs() < 0.01);

        state.align_selection(AlignMode::MiddleY);
        let centre_y = |s: &DiagramState, id: &str| {
            let n = s.doc.node(id).unwrap();
            n.y + n.h / 2.0
        };
        assert!((centre_y(&state, &a) - centre_y(&state, &b)).abs() < 0.01);
    }

    #[test]
    fn distribute_x_spaces_nodes_evenly_between_the_extremes() {
        let mut state = DiagramState::from_doc(hs_diagram::template::flow());
        state.doc.auto_layout = false;
        assert!(state.doc.nodes.len() >= 3);
        state.doc.nodes[0].x = 0.0;
        state.doc.nodes[0].w = 20.0;
        state.doc.nodes[1].x = 500.0;
        state.doc.nodes[1].w = 20.0;
        state.doc.nodes[2].x = 90.0;
        state.doc.nodes[2].w = 20.0;
        let a = state.doc.nodes[0].id.clone();
        let b = state.doc.nodes[1].id.clone();
        let c = state.doc.nodes[2].id.clone();
        state.selected = Some(a.clone());
        state.multi = vec![b.clone(), c.clone()];
        state.align_selection(AlignMode::DistributeX);
        let centre_x = |s: &DiagramState, id: &str| {
            let n = s.doc.node(id).unwrap();
            n.x + n.w / 2.0
        };
        let (ca, cb, cc) = (
            centre_x(&state, &a),
            centre_x(&state, &b),
            centre_x(&state, &c),
        );
        assert_eq!(ca, 10.0);
        assert_eq!(cb, 510.0);
        assert_eq!(cc, 260.0);
        assert!((cc - ca - (cb - cc)).abs() < 0.01);
    }

    #[test]
    fn project_save_path_uses_the_subfolder_and_extension() {
        let root = std::path::Path::new("C:/work/shop");
        let path = DiagramState::project_save_path(root, "docs/diagrams", "Checkout flow");
        assert!(path.ends_with("docs/diagrams/Checkout-flow.hdiag"));
        let custom = DiagramState::project_save_path(root, "", "plan.hdiag");
        assert!(custom.ends_with("plan.hdiag"));
        assert_eq!(custom.parent(), Some(root));
    }
}
