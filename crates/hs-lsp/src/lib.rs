//! Language Server Protocol client for the editor (docs/tools/editor.md
//! §11, ADR-023). One [`Client`] per language server process: it speaks
//! JSON-RPC over the server's stdin/stdout on two plain threads, answers
//! the few requests a server sends back, and hands everything the UI needs
//! (diagnostics, progress, log lines, the server exiting) to a channel the
//! editor drains once per frame. Requests take a callback that runs on the
//! reader thread, so the UI never blocks on a server.
//!
//! Only the slice of the protocol the editor uses is typed ([`types`]);
//! everything else stays `serde_json::Value`. Servers are not bundled: the
//! [`registry`] knows the usual command for each language and whether it
//! is on `PATH`, and the user can point a language at any other server.

pub mod client;
pub mod registry;
pub mod text;
pub mod types;
pub mod uri;

pub use client::{Client, Event, State};
pub use registry::{ServerSpec, language_id};
