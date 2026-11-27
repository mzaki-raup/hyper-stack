//! The Analytics (BI) activity (`docs/tools/analytics.md`): BI servers
//! through one provider trait, Metabase first. A tab per server, drawn
//! natively from the server's REST API with the signed-in account's own
//! permissions; read-only.

pub mod chart;
pub mod grid;
pub mod profile;
pub mod provider;
pub mod session;
mod ui;

pub use ui::{AnalyticsFactory, DESCRIPTOR};
