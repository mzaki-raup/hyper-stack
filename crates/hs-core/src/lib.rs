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
//! - [`tunnels`] — the SSH port-forward service interface
//! - [`objstore`] — SigV4 and the S3 client
//! - [`ai`] — the optional AI assistant client (1min.AI, OpenAI-compatible)
//! - [`tts`] — read-aloud through the OS voice; [`sniff`] — what a blob is
//! - [`fmt`] — pretty-printing JSON and XML for the tools that show them
//! - [`portable`] — settings export/import bundles; [`html`] — Print… pages
//! - [`shortcuts`] / [`i18n`] / [`fuzzy`] / [`logging`] / [`version`] / [`update`] / [`updater`]

pub mod ai;
pub mod ai_local;
pub mod archive;
pub mod clipboard_files;
pub mod connections;
pub mod download;
pub mod env_path;
pub mod environments;
pub mod events;
pub mod favicon;
pub mod fmt;
pub mod fuzzy;
pub mod html;
pub mod i18n;
pub mod jobs;
pub mod logging;
pub mod objstore;
pub mod open_tabs;
pub mod paths;
pub mod portable;
pub mod problems;
pub mod projects;
pub mod prompts;
pub mod runtime;
pub mod search;
pub mod secrets;
pub mod services;
pub mod settings;
pub mod shortcuts;
pub mod sniff;
pub mod storage;
pub mod stt;
pub mod tool;
pub mod tts;
pub mod tunnels;
pub mod update;
pub mod updater;
pub mod version;
pub mod workspace;

pub use services::Services;
pub use tool::{Tool, ToolContext, ToolDescriptor, ToolFactory};

/// A stable identifier for any persisted entity (ULID, 26 chars).
pub fn new_id() -> String {
    ulid::Ulid::new().to_string()
}
