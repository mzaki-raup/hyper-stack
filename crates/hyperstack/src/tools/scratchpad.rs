//! Scratchpad: a text buffer that persists in the layout. Proves save/restore,
//! dirty state and the environment binding.

use std::borrow::Cow;

use hs_core::Services;
use hs_core::tool::{
    Accept, CommandDescriptor, OpenRequest, ToastLevel, Tool, ToolCategory, ToolContext,
    ToolDescriptor, ToolFactory,
};
use serde::{Deserialize, Serialize};

pub static DESCRIPTOR: ToolDescriptor = ToolDescriptor {
    id: "scratchpad",
    title_key: "tool-scratchpad",
    icon: hs_ui::icons::NOTE,
    category: ToolCategory::Utilities,
    accepts: &[Accept::Text, Accept::LocalFile],
};

pub struct ScratchpadFactory;

impl ToolFactory for ScratchpadFactory {
    fn descriptor(&self) -> &ToolDescriptor {
        &DESCRIPTOR
    }
    fn create(&self, _services: &Services, restore: Option<serde_json::Value>) -> Box<dyn Tool> {
        let state: State = restore
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default();
        Box::new(Scratchpad {
            saved: state.text.clone(),
            state,
        })
    }
}

#[derive(Default, Serialize, Deserialize, Clone)]
struct State {
    text: String,
    title: String,
    environment: Option<String>,
}

pub struct Scratchpad {
    state: State,
    saved: String,
}

impl Tool for Scratchpad {
    fn descriptor(&self) -> &ToolDescriptor {
        &DESCRIPTOR
    }
    fn title(&self) -> Cow<'_, str> {
        if self.state.title.is_empty() {
            let first = self.state.text.lines().next().unwrap_or("").trim();
            if first.is_empty() {
                Cow::Borrowed("Scratchpad")
            } else {
                Cow::Owned(first.chars().take(40).collect())
            }
        } else {
            Cow::Borrowed(&self.state.title)
        }
    }
    fn dirty(&self) -> bool {
        self.state.text != self.saved
    }
    fn environment(&self) -> Option<String> {
        self.state.environment.clone()
    }
    fn toolbar(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        if ui
            .button(format!("{} {}", hs_ui::icons::CHECK, cx.services.t("save")))
            .clicked()
        {
            self.saved = self.state.text.clone();
            cx.toast(ToastLevel::Success, cx.services.t("toast-saved"));
        }
        ui.label(egui::RichText::new(cx.services.t("tool-scratchpad-hint")).weak());
        if self.state.environment.is_none()
            && let Some(env) = cx.env_id
        {
            self.state.environment = Some(env.to_string());
        }
    }
    fn ui(&mut self, ui: &mut egui::Ui, _cx: &mut ToolContext) {
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.add_sized(
                [ui.available_width(), ui.available_height().max(200.0)],
                egui::TextEdit::multiline(&mut self.state.text)
                    .code_editor()
                    .frame(egui::Frame::NONE),
            );
        });
    }
    fn commands(&self) -> Vec<CommandDescriptor> {
        vec![
            CommandDescriptor {
                id: "scratchpad.save".into(),
                title: "Scratchpad: save".into(),
                shortcut_action: Some("file.save".into()),
            },
            CommandDescriptor {
                id: "scratchpad.clear".into(),
                title: "Scratchpad: clear".into(),
                shortcut_action: None,
            },
        ]
    }
    fn run_command(&mut self, id: &str, cx: &mut ToolContext) -> bool {
        match id {
            "scratchpad.save" => {
                self.saved = self.state.text.clone();
                cx.toast(ToastLevel::Success, cx.services.t("toast-saved"));
                true
            }
            "scratchpad.clear" => {
                self.state.text.clear();
                true
            }
            _ => false,
        }
    }
    fn save_state(&self) -> serde_json::Value {
        serde_json::to_value(&self.state).unwrap_or_default()
    }
    fn open(&mut self, item: OpenRequest, _cx: &mut ToolContext) -> bool {
        match item {
            OpenRequest::Text { body, .. } => {
                self.state.text = body;
                true
            }
            OpenRequest::File { uri, .. } => {
                let path = uri.strip_prefix("file://").unwrap_or(&uri);
                match std::fs::read_to_string(path) {
                    Ok(t) => {
                        self.state.text = t;
                        self.saved = self.state.text.clone();
                        self.state.title = std::path::Path::new(path)
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_default();
                        true
                    }
                    Err(_) => false,
                }
            }
            _ => false,
        }
    }
}
