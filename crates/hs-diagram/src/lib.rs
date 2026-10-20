//! Diagram document model, deterministic layout, scene and exports for the
//! Graphics activity. No egui dependency: the editor and the generator share
//! this crate, and every export goes through the same scene as the screen.

pub mod convert;
pub mod export;
pub mod import;
pub mod io;
pub mod layout;
pub mod model;
pub mod route;
pub mod scene;
pub mod schema;
pub mod svg;
pub mod template;
pub mod text;
pub mod validate;

pub use model::{
    Arrow, DiagramDoc, DiagramKind, Edge, EdgeRoute, Group, Meta, Node, NodeKind, Page, PageSize,
    Point, SCHEMA_VERSION, Style,
};
pub use scene::{Prim, Scene};
pub use text::{Rect, Size};
pub use validate::{Issue, Severity};

/// Lay out a document with the deterministic approximate measure. The UI
/// passes its real font measure to [`layout::layout`].
pub fn layout_approx(doc: &DiagramDoc) -> Scene {
    layout::layout(doc, &text::approx_measure)
}
