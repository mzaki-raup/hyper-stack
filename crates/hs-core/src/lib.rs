//! HyperStack core: everything the tools share and nothing that draws.
//!
//! See `docs/03-architecture.md`. Modules:
//! - [`paths`] — the portable data folder (ADR-017)
//! - [`storage`] — atomic file IO and TOML helpers
//! - [`settings`] — typed settings with defaults
//! - [`runtime`] / [`jobs`] / [`events`] — tokio runtime, background jobs, event bus
//! - [`tool`] / [`services`] — the `Tool` trait and the shared `Services` handle
//! - [`workspace`] — workspaces and layout persistence
//! - [`connections`] / [`environments`] / [`secrets`] — shared profiles, variables, vault
//! - [`shortcuts`] / [`i18n`] / [`fuzzy`] / [`logging`] / [`version`] / [`update`]

pub mod connections;
pub mod environments;
pub mod events;
pub mod fuzzy;
pub mod i18n;
pub mod jobs;
pub mod logging;
pub mod paths;
pub mod runtime;
pub mod secrets;
pub mod services;
pub mod settings;
pub mod shortcuts;
pub mod storage;
pub mod tool;
pub mod update;
pub mod version;
pub mod workspace;

pub use services::Services;
pub use tool::{Tool, ToolContext, ToolDescriptor, ToolFactory};

/// A stable identifier for any persisted entity (ULID, 26 chars).
pub fn new_id() -> String {
    ulid::Ulid::new().to_string()
}
