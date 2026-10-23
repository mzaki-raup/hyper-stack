//! Log tail viewer (docs/tools/logs.md): one tab = one source — a local
//! file followed like `tail -F`, a file on an SSH host, a command's
//! output, or pasted text — parsed into records (plain, JSON lines,
//! logfmt, syslog, nginx, Postgres), filtered by chips, painted by
//! highlight rules, searched, bookmarked, with a detail pane per record.
//!
//! All reading happens on the runtime ([`engine`]); the UI only touches
//! the record window under a short lock.

pub mod chart;
pub mod engine;
pub mod filter;
pub mod parse;
pub mod redact;
pub mod searcher;
pub mod services;
pub mod shape;
pub mod source;
pub mod ui;
pub mod views;

pub use ui::{DESCRIPTOR, GOTO_MIME, LogsFactory, LogsTool, SPEC_MIME, State};
