//! NoSQL tool (docs/tools/nosql.md): one tab per document-store connection,
//! with its own session owned by a task on the runtime.
//!
//! Safety (docs/06-security.md §3-4): a read-only profile, and a protected
//! environment until the tab unlocks writes, refuse every write; deleting
//! what a filter matches counts first and asks for the environment typed.

pub mod couch;
pub mod flatten;
pub mod mongo;
pub mod profile;
pub mod runner;
pub mod store;
pub mod ui;

pub use ui::{DESCRIPTOR, NoSqlFactory, NoSqlTool, State};
