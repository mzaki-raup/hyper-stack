//! HyperStack editor engine (docs/tools/editor.md): every text surface in the
//! app shares this — the editor tool, SQL and request bodies later, JSON
//! viewers, read-only buffers.
//!
//! - [`buffer`] — rope text with encoding/EOL handling and an undo stack
//! - [`cursor`] — cursors and selections (multi-cursor aware)
//! - [`languages`] — extension → syntax, comment token, indent width
//! - [`highlight`] — cached syntect highlighting per line
//! - [`view`] — the egui widget: rendering, input, find/replace, go to line
//! - [`markdown`] — a Markdown renderer for previews
//! - [`mermaid`] — native flowchart and sequence diagrams for the preview

pub mod buffer;
pub mod cursor;
pub mod highlight;
pub mod languages;
pub mod markdown;
pub mod mermaid;
pub mod view;

pub use buffer::{Buffer, Encoding, Eol};
pub use cursor::{Cursor, Cursors};
pub use view::{EditorResponse, EditorView};
