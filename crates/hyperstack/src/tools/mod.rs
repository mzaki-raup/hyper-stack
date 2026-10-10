//! Built-in Phase 0 tools that prove the plumbing. Real tools live in their
//! own crates from M1 on.

pub mod hello;
pub mod scratchpad;

use hs_core::tool::ToolFactory;

pub fn factories() -> Vec<Box<dyn ToolFactory>> {
    vec![
        Box::new(tool_editor::EditorFactory),
        Box::new(tool_terminal::TerminalFactory),
        Box::new(scratchpad::ScratchpadFactory),
        Box::new(hello::HelloFactory),
    ]
}
