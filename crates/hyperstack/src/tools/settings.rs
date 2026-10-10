//! Settings as a regular tab (docs/04 §9): the category list lives in the
//! sidebar, the page in the tab. State and the effects the pages produce are
//! shared with the shell through `SharedSettings`; the shell drains the
//! effects once per frame and applies them (theme, saves, updater, vault).

use std::borrow::Cow;

use hs_core::Services;
use hs_core::tool::{
    Accept, CommandDescriptor, Tool, ToolCategory, ToolContext, ToolDescriptor, ToolFactory,
};
use hs_ui::theme::Theme;

use crate::settings_ui::{self, Page, SharedSettings};

pub static DESCRIPTOR: ToolDescriptor = ToolDescriptor {
    id: "settings",
    title_key: "settings",
    icon: hs_ui::icons::SETTINGS,
    category: ToolCategory::Utilities,
    accepts: &[Accept::Text],
};

pub struct SettingsFactory {
    pub hub: SharedSettings,
}

impl ToolFactory for SettingsFactory {
    fn descriptor(&self) -> &ToolDescriptor {
        &DESCRIPTOR
    }
    fn create(&self, _services: &Services, restore: Option<serde_json::Value>) -> Box<dyn Tool> {
        if let Some(key) = restore
            .as_ref()
            .and_then(|v| v.get("page"))
            .and_then(|v| v.as_str())
            && let Some(page) = Page::by_key(key)
        {
            self.hub.lock().state.page = page;
        }
        Box::new(SettingsTool {
            hub: self.hub.clone(),
        })
    }
}

pub struct SettingsTool {
    hub: SharedSettings,
}

impl Tool for SettingsTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &DESCRIPTOR
    }

    fn title(&self) -> Cow<'_, str> {
        Cow::Borrowed("Settings")
    }

    fn sidebar(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) -> bool {
        let theme = Theme::by_name(&cx.services.settings.read().appearance.theme);
        let mut hub = self.hub.lock();
        settings_ui::page_list(ui, &theme, cx.services, &mut hub.state);
        true
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        let theme = Theme::by_name(&cx.services.settings.read().appearance.theme);
        let mut hub = self.hub.lock();
        let hub = &mut *hub;
        egui::ScrollArea::vertical()
            .id_salt("hs-settings-page")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Frame::new()
                    .inner_margin(egui::Margin::symmetric(16, 12))
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        settings_ui::page(
                            ui,
                            &theme,
                            cx.services,
                            &mut hub.state,
                            &mut hub.effects,
                        );
                    });
            });
    }

    fn commands(&self) -> Vec<CommandDescriptor> {
        Page::all()
            .iter()
            .map(|(_, key)| CommandDescriptor {
                id: format!("settings.page:{key}"),
                title: format!("Settings: {}", key.trim_start_matches("settings-")),
                shortcut_action: None,
            })
            .collect()
    }

    fn run_command(&mut self, id: &str, _cx: &mut ToolContext) -> bool {
        if let Some(key) = id.strip_prefix("settings.page:")
            && let Some(page) = Page::by_key(key)
        {
            self.hub.lock().state.page = page;
            return true;
        }
        false
    }

    fn save_state(&self) -> serde_json::Value {
        let page = self.hub.lock().state.page;
        serde_json::json!({ "page": page.key() })
    }
}
