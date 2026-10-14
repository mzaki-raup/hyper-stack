//! Built-in Phase 0 tools that prove the plumbing. Real tools live in their
//! own crates from M1 on.

pub mod home;
pub mod scratchpad;
pub mod settings;

use hs_core::tool::ToolFactory;

use crate::settings_ui::SharedSettings;
use home::SharedHome;

/// Every tool, in default rail order: Home first, Settings last.
pub fn factories(settings: SharedSettings, home: SharedHome) -> Vec<Box<dyn ToolFactory>> {
    vec![
        Box::new(home::HomeFactory { hub: home }),
        Box::new(tool_editor::EditorFactory),
        Box::new(tool_terminal::TerminalFactory),
        Box::new(tool_ssh::SshFactory),
        Box::new(tool_s3::S3Factory),
        Box::new(tool_db::DbFactory),
        Box::new(scratchpad::ScratchpadFactory),
        Box::new(settings::SettingsFactory { hub: settings }),
    ]
}
