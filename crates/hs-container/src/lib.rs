//! Container drivers and the connection fields that configure them
//! (`docs/tools/containers.md`, ADR-026).
//!
//! A container host is an ordinary [`hs_core::connections::ConnectionProfile`]
//! of kind `docker`, `podman`, `k8s` or `wincontainer`; this crate owns
//! the `[container]` sub-table those profiles carry, exactly as
//! `hs-ssh` owns `[ssh]` and `tool-db` owns `[db]`.
//!
//! No egui here: the drivers must be testable without a window, and the
//! crate has to compile on Linux and macOS like every other core crate.

pub mod cli;
pub mod model;
pub mod profile;
pub mod run;

pub use cli::{Engine, Invocation, Verb};
pub use model::{Container, Health, Image, Network, Port, State, Summary, Volume};
pub use profile::{ContainerFields, Transport};
