//! "Welcome" tool: shows version, data folder and the keys to press next.

use std::borrow::Cow;

use hs_core::Services;
use hs_core::tool::{
    Accept, CommandDescriptor, ToastLevel, Tool, ToolCategory, ToolContext, ToolDescriptor,
    ToolFactory,
};

pub static DESCRIPTOR: ToolDescriptor = ToolDescriptor {
    id: "hello",
    title_key: "tool-hello",
    icon: hs_ui::icons::HAND_WAVE,
    category: ToolCategory::Utilities,
    accepts: &[Accept::Text],
};

pub struct HelloFactory;

impl ToolFactory for HelloFactory {
    fn descriptor(&self) -> &ToolDescriptor {
        &DESCRIPTOR
    }
    fn create(&self, _services: &Services, _restore: Option<serde_json::Value>) -> Box<dyn Tool> {
        Box::new(Hello { clicks: 0 })
    }
}

pub struct Hello {
    clicks: u32,
}

impl Tool for Hello {
    fn descriptor(&self) -> &ToolDescriptor {
        &DESCRIPTOR
    }
    fn title(&self) -> Cow<'_, str> {
        Cow::Borrowed("Welcome")
    }
    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        let s = cx.services;
        ui.add_space(24.0);
        ui.vertical_centered(|ui| {
            ui.heading(s.t("app-name"));
            ui.label(s.t("app-tagline"));
            ui.add_space(12.0);
            ui.label(s.t("tool-hello-body"));
            ui.add_space(12.0);
            ui.monospace(s.version.to_string());
            ui.monospace(s.paths.root().display().to_string());
            ui.add_space(12.0);
            if ui.button(s.t("tool-hello-toast-button")).clicked() {
                self.clicks += 1;
                cx.toast(ToastLevel::Success, format!("Hello #{}", self.clicks));
            }
        });
    }
    fn commands(&self) -> Vec<CommandDescriptor> {
        vec![CommandDescriptor {
            id: "hello.toast".into(),
            title: "Welcome: show a toast".into(),
            shortcut_action: None,
        }]
    }
    fn run_command(&mut self, id: &str, cx: &mut ToolContext) -> bool {
        if id == "hello.toast" {
            cx.toast(ToastLevel::Info, "Hello from the command palette");
            true
        } else {
            false
        }
    }
}
