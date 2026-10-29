//! The toolchain-version probe moved to `hs_core::langver` on
//! 2026-10-29 so the Git activity's header band could share its cache;
//! the editor keeps calling it under this name.

pub use hs_core::langver::{cached, display_name, ensure};
