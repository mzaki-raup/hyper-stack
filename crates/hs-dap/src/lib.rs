//! Debug Adapter Protocol client for the editor (docs/tools/editor.md
//! §12, ADR-024). A [`client::Client`] talks to one debug adapter process
//! (CodeLLDB, lldb-dap, debugpy, delve, netcoredbg, gdb) over its stdio or
//! a TCP port with the same `Content-Length` framing LSP uses; replies run
//! callbacks on the reader thread and events go to a channel the editor
//! drains once per frame. [`adapters`] knows the usual command for each
//! adapter and how to install it, builds launch configurations for Cargo,
//! Python and Go projects, and reads `.hyperstack/debug.json` and
//! `.vscode/launch.json`. Nothing is bundled: the user installs adapters
//! like language servers.

pub mod adapters;
pub mod client;
pub mod types;

pub use adapters::{AdapterSpec, LaunchConfig, Transport};
pub use client::{Callback, Client, Event, RequestError};
