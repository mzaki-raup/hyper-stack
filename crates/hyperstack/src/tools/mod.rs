//! Built-in Phase 0 tools that prove the plumbing. Real tools live in their
//! own crates from M1 on.

pub mod home;
pub mod scratchpad;
pub mod settings;

use hs_core::tool::ToolFactory;

use crate::settings_ui::SharedSettings;
use home::SharedHome;

/// Every tool, in default rail order: Home first, Settings last.
pub fn factories(
    settings: SharedSettings,
    home: SharedHome,
    browser: tool_browser::SharedBrowser,
    graphics: tool_graphics::SharedGraphics,
) -> Vec<Box<dyn ToolFactory>> {
    vec![
        Box::new(home::HomeFactory { hub: home }),
        Box::new(tool_editor::EditorFactory),
        // Right after the Editor, as the owner placed it (2026-10-29);
        // `rail_order` appends it to a saved order, where Settings ->
        // Appearance can move it.
        Box::new(tool_git::GitFactory),
        Box::new(tool_terminal::TerminalFactory),
        Box::new(tool_ssh::SshFactory),
        Box::new(tool_s3::S3Factory),
        // Between S3 and Database, which is "between SSH and Database"
        // as the owner described it. The rail is per workspace and
        // user-ordered, so this is only the default (ADR-026).
        Box::new(tool_containers::ContainersFactory),
        Box::new(tool_db::DbFactory),
        Box::new(tool_redis::RedisFactory),
        Box::new(tool_nosql::NoSqlFactory),
        Box::new(tool_logs::LogsFactory),
        Box::new(tool_api::ApiFactory),
        Box::new(tool_browser::BrowserFactory { hub: browser }),
        Box::new(tool_graphics::GraphicsFactory { hub: graphics }),
        Box::new(tool_devtools::DevtoolsFactory),
        Box::new(scratchpad::ScratchpadFactory),
        Box::new(settings::SettingsFactory { hub: settings }),
    ]
}
