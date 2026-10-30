//! Redis tool (docs/tools/redis.md): one tab = one `kind = "redis"`
//! connection (and so one environment), with its own session owned by a task
//! on the runtime ([`runner`]). The sidebar is the key tree built with
//! `SCAN`; the main area has Keys, Console and Info.
//!
//! Safety (docs/06-security.md §3–4): every console command is classified
//! before it runs. Read-only profiles, and protected environments until
//! writes are allowed for the tab, refuse anything that writes; FLUSHALL,
//! KEYS, CONFIG SET and the rest ask first, and deleting a whole prefix
//! counts the keys and asks for the prefix to be typed back.

pub mod client;
pub mod console;
pub mod depth;
pub mod keytree;
pub mod profile;
pub mod runner;
pub mod ui;
pub mod value;

pub use ui::{DESCRIPTOR, RedisFactory, RedisTool, State};
