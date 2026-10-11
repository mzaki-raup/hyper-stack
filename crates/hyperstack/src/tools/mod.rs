//! Built-in Phase 0 tools that prove the plumbing. Real tools live in their
//! own crates from M1 on.

pub mod hello;
pub mod scratchpad;
pub mod settings;

use hs_core::tool::ToolFactory;

use crate::settings_ui::SharedSettings;

pub fn factories(settings: SharedSettings) -> Vec<Box<dyn ToolFactory>> {
    vec![
        Box::new(tool_editor::EditorFactory),
        Box::new(tool_terminal::TerminalFactory),
        Box::new(tool_ssh::SshFactory),
        Box::new(scratchpad::ScratchpadFactory),
        Box::new(settings::SettingsFactory { hub: settings }),
        Box::new(hello::HelloFactory),
    ]
}
