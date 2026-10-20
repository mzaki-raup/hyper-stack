//! Diagram Editor: a direct-manipulation canvas over `hs-diagram`.
//!
//! Release 1 deliberately keeps one scene: the canvas paints the same
//! `hs_diagram::Scene` that SVG, PNG and PDF export, so what the user moves is
//! what the file contains.

use std::path::{Path, PathBuf};

use hs_core::tool::{ToastLevel, ToolContext};
use hs_diagram::export::{PngOptions, pdf, png, svg};
use hs_diagram::model::{
    Arrow, DiagramDoc, DiagramKind, Edge, EdgeRoute, Node, NodeKind, Point, Style,
};
use hs_diagram::{Prim, Rect, Scene, layout_approx, validate};
use hs_ui::form;
use hs_ui::theme::Theme;

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
    selected: Option<String>,
    /// Additional nodes selected with Shift-click; the primary is `selected`.
    multi: Vec<String>,
    selected_edge: Option<String>,
    /// World-space polyline of every edge from the last painted scene.
    edge_paths: Vec<Vec<Point>>,
    zoom: f32,
    pan: egui::Vec2,
    show_grid: bool,
    tool: ToolMode,
    edge_from: Option<String>,
    drag_node: Option<(String, Point, egui::Pos2)>,
    drag_resize: Option<(String, f32, f32, egui::Pos2)>,
    drag_via: Option<(String, usize, Point, egui::Pos2)>,
    drag_pan: Option<(egui::Vec2, egui::Pos2)>,
    last_center: Point,
    saved: Option<PathBuf>,
    status: Option<String>,
    save_open: bool,
    save_project: usize,
    save_subfolder: String,
    save_name: String,
    save_overwrite: bool,
    save_error: Option<String>,
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
            selected,
            multi: Vec::new(),
            selected_edge: None,
            edge_paths: Vec::new(),
            zoom: 0.0,
            pan: egui::Vec2::ZERO,
            show_grid: true,
            tool: ToolMode::Select,
            edge_from: None,
            drag_node: None,
            drag_resize: None,
            drag_via: None,
            drag_pan: None,
            last_center: Point { x: 0.0, y: 0.0 },
            saved: None,
            status: None,
            save_open: false,
            save_project: 0,
            save_subfolder: "docs/diagrams".into(),
            save_name: String::new(),
            save_overwrite: false,
            save_error: None,
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

    fn node_rects(&self, scene: &Scene) -> Vec<Rect> {
        scene
            .prims
            .iter()
            .filter_map(|p| match p {
                Prim::Shape { rect, .. } => Some(*rect),
                _ => None,
            })
            .collect()
    }

    fn push_undo(&mut self) {
        self.undo.push(self.doc.clone());
        if self.undo.len() > 80 {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.dirty = true;
    }

    fn undo(&mut self) {
        if let Some(doc) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.doc, doc));
            self.dirty = true;
            if self
                .selected
                .as_ref()
                .is_some_and(|id| self.doc.node(id).is_none())
            {
                self.selected = None;
            }
            self.multi.retain(|id| self.doc.node(id).is_some());
        }
    }

    fn redo(&mut self) {
        if let Some(doc) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.doc, doc));
            self.dirty = true;
            if self
                .selected
                .as_ref()
                .is_some_and(|id| self.doc.node(id).is_none())
            {
                self.selected = None;
            }
            self.multi.retain(|id| self.doc.node(id).is_some());
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
            _ => (160.0, 64.0),
        };
        let node = Node {
            id: format!("node{n}"),
            label: format!("Node {n}"),
            kind,
            x: at.x - size.0 / 2.0,
            y: at.y - size.1 / 2.0,
            w: size.0,
            h: size.1,
            style: Style::default(),
        };
        self.selected = Some(node.id.clone());
        self.doc.nodes.push(node);
        self.tool = ToolMode::Select;
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
        self.selected = None;
        self.multi.clear();
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
        self.freeze();
        let mut boxes = Vec::new();
        for id in &ids {
            if let Some(n) = self.doc.nodes.iter().find(|n| &n.id == id) {
                boxes.push((n.x, n.y, n.w, n.h));
            }
        }
        if boxes.len() < 2 {
            return;
        }
        let min_x = boxes.iter().map(|b| b.0).fold(f32::INFINITY, f32::min);
        let max_x = boxes
            .iter()
            .map(|b| b.0 + b.2)
            .fold(f32::NEG_INFINITY, f32::max);
        let min_y = boxes.iter().map(|b| b.1).fold(f32::INFINITY, f32::min);
        let max_y = boxes
            .iter()
            .map(|b| b.1 + b.3)
            .fold(f32::NEG_INFINITY, f32::max);
        let cy = (min_y + max_y) / 2.0;
        let cx = (min_x + max_x) / 2.0;
        for (id, (_, _, w, h)) in ids.iter().zip(boxes) {
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
        if ui
            .button(format!("{} New", hs_ui::icons::ph::FILE_PLUS))
            .clicked()
        {
            self.replace(hs_diagram::template::flow());
        }
        if ui.button(hs_ui::icons::ph::FOLDER_OPEN).clicked() {
            self.open_dialog(cx);
        }
        if ui.button(s.t("gfx-diagram-save")).clicked() {
            self.save(cx);
        }
        ui.menu_button("Import", |ui| {
            if ui.button("Archify JSON…").clicked() {
                self.import(cx, ImportKind::Archify);
                ui.close();
            }
            if ui.button("draw.io XML…").clicked() {
                self.import(cx, ImportKind::DrawIo);
                ui.close();
            }
            if ui.button("Mermaid Sequence…").clicked() {
                self.import(cx, ImportKind::Mermaid);
                ui.close();
            }
        });
        if ui.button(s.t("gfx-diagram-save-project")).clicked() {
            self.save_name = crate::diagram::sanitise(&self.doc.title);
            self.save_error = None;
            self.save_overwrite = false;
            self.save_open = true;
        }
        ui.menu_button(format!("{} Export", hs_ui::icons::ph::EXPORT), |ui| {
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
        });
        ui.separator();
        if ui
            .add_enabled(
                !self.undo.is_empty(),
                egui::Button::new(hs_ui::icons::ph::ARROW_COUNTER_CLOCKWISE),
            )
            .on_hover_text(s.t("gfx-diagram-undo"))
            .clicked()
        {
            self.undo();
        }
        if ui
            .add_enabled(
                !self.redo.is_empty(),
                egui::Button::new(hs_ui::icons::ph::ARROW_CLOCKWISE),
            )
            .on_hover_text(s.t("gfx-diagram-redo"))
            .clicked()
        {
            self.redo();
        }
        ui.separator();
        if ui.button(s.t("gfx-zoom-fit")).clicked() {
            self.zoom = 0.0;
            self.pan = egui::Vec2::ZERO;
        }
        ui.checkbox(&mut self.show_grid, s.t("gfx-diagram-grid"));
        ui.separator();
        if ui
            .selectable_label(self.tool == ToolMode::Select, hs_ui::icons::ph::CURSOR)
            .on_hover_text(s.t("gfx-diagram-select"))
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
        if ui
            .selectable_label(self.tool == ToolMode::Edge, hs_ui::icons::ph::ARROW_RIGHT)
            .on_hover_text(s.t("gfx-diagram-add-edge"))
            .clicked()
        {
            self.tool = ToolMode::Edge;
            self.edge_from = None;
        }
        ui.menu_button("Align", |ui| {
            let enough = self.selection().len() >= 2;
            for (label, mode) in [
                ("Align Left", AlignMode::Left),
                ("Align Centre", AlignMode::CentreX),
                ("Align Right", AlignMode::Right),
                ("Align Top", AlignMode::Top),
                ("Align Middle", AlignMode::MiddleY),
                ("Align Bottom", AlignMode::Bottom),
            ] {
                if ui.add_enabled(enough, egui::Button::new(label)).clicked() {
                    self.align_selection(mode);
                    ui.close();
                }
            }
            ui.separator();
            for (label, mode) in [
                ("Distribute Horizontally", AlignMode::DistributeX),
                ("Distribute Vertically", AlignMode::DistributeY),
            ] {
                if ui.add_enabled(enough, egui::Button::new(label)).clicked() {
                    self.align_selection(mode);
                    ui.close();
                }
            }
        });
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
            .on_hover_text(path.display().to_string());
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
        self.save_window(ui.ctx(), cx);
        egui::Panel::left("gfx-diagram-props")
            .default_size(290.0)
            .size_range(240.0..=430.0)
            .resizable(true)
            .show(ui, |ui| self.properties(ui, cx, theme));
        let canvas = ui.available_rect_before_wrap();
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(canvas)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        child.set_clip_rect(canvas);
        self.canvas(&mut child, cx, theme);
    }

    fn properties(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext, theme: &Theme) {
        let s = cx.services;
        egui::ScrollArea::vertical()
            .id_salt("gfx-diagram-props-scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new(s.t("gfx-diagram-properties"))
                        .strong()
                        .color(theme.text),
                );
                ui.add_space(4.0);
                if ui
                    .add(
                        egui::TextEdit::singleline(&mut self.doc.title)
                            .hint_text(s.t("gfx-diagram-title")),
                    )
                    .changed()
                {
                    self.dirty = true;
                }
                ui.add_space(8.0);

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
                ui.separator();
                ui.add_space(6.0);

                if let Some(id) = self.selected.clone() {
                    let mut changed = false;
                    let mut remove = false;
                    if let Some(node) = self.doc.node_mut(&id) {
                        ui.label(
                            egui::RichText::new(s.t("gfx-diagram-node"))
                                .strong()
                                .color(theme.text),
                        );
                        ui.horizontal(|ui| {
                            ui.label(s.t("gfx-diagram-label"));
                            changed |= ui
                                .add(
                                    egui::TextEdit::singleline(&mut node.label)
                                        .desired_width(f32::INFINITY),
                                )
                                .changed();
                        });
                        ui.horizontal(|ui| {
                            ui.label(s.t("gfx-diagram-shape"));
                            egui::ComboBox::from_id_salt("gfx-diagram-shape")
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
                        });
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
                        ui.label(egui::RichText::new("Edge").strong().color(theme.text));
                        ui.horizontal(|ui| {
                            ui.label(s.t("gfx-diagram-label"));
                            changed |= ui
                                .add(
                                    egui::TextEdit::singleline(&mut edge.label)
                                        .desired_width(f32::INFINITY),
                                )
                                .changed();
                        });
                        ui.label(
                            egui::RichText::new(format!("{} → {}", edge.from, edge.to))
                                .small()
                                .color(theme.text_muted),
                        );
                        if ui.button("Add Waypoint").clicked() {
                            add = true;
                        }
                        if !edge.via.is_empty() && ui.button("Clear Waypoints").clicked() {
                            clear = true;
                        }
                        if ui.button(s.t("gfx-diagram-delete-edge")).clicked() {
                            remove = true;
                        }
                    }
                    if add {
                        self.add_edge_waypoint(&eid);
                    }
                    if clear && let Some(edge) = self.doc.edges.iter_mut().find(|e| e.id == eid) {
                        edge.via.clear();
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

                ui.label(
                    egui::RichText::new(s.t("gfx-diagram-outline"))
                        .strong()
                        .color(theme.text),
                );
                ui.add_space(2.0);
                let nodes: Vec<(String, String)> = self
                    .doc
                    .nodes
                    .iter()
                    .map(|n| (n.id.clone(), n.label.clone()))
                    .collect();
                for (id, label) in nodes {
                    let selected = self.selected.as_deref() == Some(id.as_str());
                    if ui.selectable_label(selected, label).clicked() {
                        self.selected = Some(id);
                        self.multi.clear();
                    }
                }
                for edge in &self.doc.edges {
                    let label = if edge.label.is_empty() {
                        format!("{} → {}", edge.from, edge.to)
                    } else {
                        format!("{} → {} ({})", edge.from, edge.to, edge.label)
                    };
                    let selected = self.selected_edge.as_deref() == Some(edge.id.as_str());
                    if ui.selectable_label(selected, label).clicked() {
                        self.selected_edge = Some(edge.id.clone());
                        self.selected = None;
                        self.multi.clear();
                    }
                }
                ui.add_space(6.0);
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
        let scene = layout_approx(&self.doc);
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
        let edge_lines: Vec<Vec<Point>> = scene
            .prims
            .iter()
            .filter_map(|p| match p {
                Prim::Line { points, .. } => Some(points.clone()),
                _ => None,
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
            }
        }
        if response.drag_stopped() {
            self.drag_node = None;
            self.drag_resize = None;
            self.drag_via = None;
            self.drag_pan = None;
        }
        let clicked =
            response.clicked() || (response.hovered() && ui.input(|i| i.pointer.primary_clicked()));
        let clicked_pos = pointer.or_else(|| ui.input(|i| i.pointer.interact_pos()));
        if clicked && let Some(pos) = clicked_pos {
            if let Some(i) = self.hit(pos, &node_rects, origin, z) {
                let id = self.doc.nodes[i].id.clone();
                if self.tool == ToolMode::Edge {
                    if let Some(from) = self.edge_from.take() {
                        if from != id {
                            self.push_undo();
                            self.freeze();
                            self.doc.edges.push(Edge {
                                id: format!("e{}", self.doc.edges.len() + 1),
                                from,
                                to: id.clone(),
                                label: String::new(),
                                route: EdgeRoute::Orthogonal,
                                arrow: Arrow::End,
                                dashed: false,
                                via: Vec::new(),
                            });
                        }
                    } else {
                        self.edge_from = Some(id.clone());
                    }
                }
                if self.tool == ToolMode::Select && ui.input(|i| i.modifiers.shift) {
                    if !self.multi.contains(&id) && self.selected.as_deref() != Some(id.as_str()) {
                        self.multi.push(id);
                    }
                    self.selected_edge = None;
                } else {
                    self.selected = Some(id);
                    self.multi.clear();
                    self.selected_edge = None;
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
                self.multi.clear();
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
                    if let Some(p) = pointer {
                        let world = Point {
                            x: (p.x - origin.x) / old,
                            y: (p.y - origin.y) / old,
                        };
                        self.pan += (p - to_screen(world)) * (new / old);
                    }
                    self.zoom = new;
                } else {
                    self.pan.y += scroll;
                }
            }
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
        if let Some(eid) = &self.selected_edge
            && let Some(edge) = self.doc.edges.iter().find(|e| &e.id == eid)
        {
            for via in &edge.via {
                let p = to_screen(*via);
                painter.circle_filled(p, 5.0, theme.panel_bg);
                painter.circle_stroke(p, 5.0, egui::Stroke::new(2.0, theme.accent));
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
                "Click the target node"
            } else {
                "Drag to move · Ctrl+wheel to zoom · Space+drag to pan"
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
                Prim::Frame { rect, label } => {
                    painter.rect_stroke(
                        srect(*rect),
                        8.0 * z,
                        egui::Stroke::new(1.2, theme.text_muted),
                        egui::StrokeKind::Inside,
                    );
                    if !label.is_empty() {
                        painter.text(
                            srect(*rect).left_top() + egui::vec2(6.0, 4.0),
                            egui::Align2::LEFT_TOP,
                            label,
                            egui::FontId::proportional((12.0 * z).max(9.0)),
                            theme.text_muted,
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
                                color(style.text.as_deref().unwrap_or("#1f2328")),
                            );
                        }
                    }
                }
                Prim::Line {
                    points,
                    dashed,
                    arrow_start,
                    arrow_end,
                    style,
                } => {
                    if points.len() < 2 {
                        continue;
                    }
                    let pts: Vec<egui::Pos2> = points.iter().map(|p| _to_screen(*p)).collect();
                    let stroke = egui::Stroke::new(
                        (1.5 * z).max(1.0),
                        color(style.stroke.as_deref().unwrap_or("#5b6472")),
                    );
                    if *dashed || style.dashed {
                        for pair in pts.windows(2) {
                            painter.line_segment([pair[0], pair[1]], stroke);
                        }
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

    fn replace(&mut self, doc: DiagramDoc) {
        self.push_undo();
        self.doc = doc;
        self.selected = self.doc.nodes.first().map(|n| n.id.clone());
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
            ImportKind::Archify => ("Archify JSON", "json"),
            ImportKind::DrawIo => ("draw.io XML", "drawio"),
            ImportKind::Mermaid => ("Mermaid sequence", "mmd"),
        };
        let Some(path) = rfd::FileDialog::new()
            .add_filter(title, &[filter])
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
            painter.rect(r, 8.0 * z, fill, stroke, egui::StrokeKind::Inside);
            let ry = (r.height() * 0.12).min(10.0 * z);
            painter.add(egui::Shape::ellipse_stroke(
                egui::pos2(r.center().x, r.top() + ry),
                egui::vec2(r.width() / 2.0, ry),
                stroke,
            ));
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
    fn project_save_path_uses_the_subfolder_and_extension() {
        let root = std::path::Path::new("C:/work/shop");
        let path = DiagramState::project_save_path(root, "docs/diagrams", "Checkout flow");
        assert!(path.ends_with("docs/diagrams/Checkout-flow.hdiag"));
        let custom = DiagramState::project_save_path(root, "", "plan.hdiag");
        assert!(custom.ends_with("plan.hdiag"));
        assert_eq!(custom.parent(), Some(root));
    }
}
