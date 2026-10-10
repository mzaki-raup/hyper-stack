//! Built-in Phase 0 tools that prove the plumbing. Real tools live in their
//! own crates from M1 on.

pub mod hello;
pub mod scratchpad;

use hs_core::tool::ToolFactory;

pub fn factories() -> Vec<Box<dyn ToolFactory>> {
    vec![
        Box::new(hello::HelloFactory),
        Box::new(scratchpad::ScratchpadFactory),
    ]
}
