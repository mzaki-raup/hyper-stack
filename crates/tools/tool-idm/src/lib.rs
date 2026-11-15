//! HyperStack Identity tool (docs/tools/idm.md, ADR-032): identity
//! servers behind one [`provider::IdmProvider`] trait, Keycloak first,
//! one tab per server. Every tab talks to its provider through
//! [`guarded::Guarded`] (read-only refusal and the audit log) on a
//! [`runner::Session`] task.

pub mod audit;
pub mod guarded;
pub mod mask;
pub mod profile;
pub mod provider;
pub mod runner;
pub mod ui;

pub use ui::{DESCRIPTOR, IdmFactory};
