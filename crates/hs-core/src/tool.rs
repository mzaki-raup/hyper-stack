//! The `Tool` trait every tool crate implements (ADR-004) and the context the
//! shell hands it each frame.

use std::borrow::Cow;

use crate::events::Event;
use crate::services::Services;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ToolCategory {
    Code,
    Data,
    Network,
    Infra,
    Utilities,
}

impl ToolCategory {
    pub fn all() -> [ToolCategory; 5] {
        [
            Self::Code,
            Self::Data,
            Self::Network,
            Self::Infra,
            Self::Utilities,
        ]
    }
    pub fn label_key(self) -> &'static str {
        match self {
            Self::Code => "category-code",
            Self::Data => "category-data",
            Self::Network => "category-network",
            Self::Infra => "category-infra",
            Self::Utilities => "category-utilities",
        }
    }
}

/// What a tool can be handed through `Tool::open`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Accept {
    LocalFile,
    RemoteFile,
    Text,
    Connection,
}

#[derive(Debug, Clone)]
pub struct ToolDescriptor {
    /// Stable id used in layout files and the palette: "editor", "db", "scratchpad".
    pub id: &'static str,
    /// Fluent key for the display name.
    pub title_key: &'static str,
    /// Phosphor glyph.
    pub icon: &'static str,
    pub category: ToolCategory,
    pub accepts: &'static [Accept],
}

#[derive(Debug, Clone)]
pub struct CommandDescriptor {
    pub id: String,
    pub title: String,
    pub shortcut_action: Option<String>,
}

/// A request to open something in a tool (cross-tool integration).
#[derive(Debug, Clone, PartialEq)]
pub enum OpenRequest {
    File {
        uri: String,
        line: Option<u32>,
    },
    Text {
        mime: String,
        body: String,
    },
    Connection {
        id: String,
    },
    Terminal {
        cwd: Option<String>,
        host: Option<String>,
        profile: Option<String>,
    },
}

/// A file-like item a tool contributes to Ctrl+P. `id` is handed back to the
/// tool as `OpenRequest::File { uri: id }` when chosen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuickOpenItem {
    pub id: String,
    pub title: String,
    pub subtitle: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseDecision {
    Close,
    /// The shell shows a "discard changes?" dialog first.
    AskUnsavedChanges,
}

/// Requests a tool queues for the shell to execute after its `ui` returns.
#[derive(Debug, Clone)]
pub enum Action {
    OpenTool {
        factory_id: String,
        request: Option<OpenRequest>,
    },
    /// Like `OpenTool`, but always in a new tab (no routing to a tab that
    /// already shows or would take the item).
    OpenToolNewTab {
        factory_id: String,
        request: Option<OpenRequest>,
    },
    FocusTab {
        tab_id: u64,
    },
    CloseSelf,
    Toast {
        level: ToastLevel,
        text: String,
    },
    SetStatus {
        text: String,
    },
    SetTitle {
        text: String,
    },
    RunCommand {
        id: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastLevel {
    Info,
    Success,
    Warning,
    Error,
}

/// Per-frame context: services, the tab's id, the active environment and the
/// action queue.
pub struct ToolContext<'a> {
    pub services: &'a Services,
    pub tab_id: u64,
    pub env_id: Option<&'a str>,
    pub actions: &'a mut Vec<Action>,
}

impl ToolContext<'_> {
    pub fn toast(&mut self, level: ToastLevel, text: impl Into<String>) {
        self.actions.push(Action::Toast {
            level,
            text: text.into(),
        });
    }
    pub fn status(&mut self, text: impl Into<String>) {
        self.actions.push(Action::SetStatus { text: text.into() });
    }
    pub fn open(&mut self, factory_id: &str, request: Option<OpenRequest>) {
        self.actions.push(Action::OpenTool {
            factory_id: factory_id.to_string(),
            request,
        });
    }
    pub fn open_new_tab(&mut self, factory_id: &str, request: Option<OpenRequest>) {
        self.actions.push(Action::OpenToolNewTab {
            factory_id: factory_id.to_string(),
            request,
        });
    }
}

pub trait Tool: Send {
    fn descriptor(&self) -> &ToolDescriptor;

    /// Tab label, may change (file name, "query #3 — staging", host name).
    fn title(&self) -> Cow<'_, str>;

    fn dirty(&self) -> bool {
        false
    }

    /// Environment this tab is bound to (drives the banner colour).
    fn environment(&self) -> Option<String> {
        None
    }

    /// Main area. Called every frame the tab is visible.
    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext);

    /// Optional sidebar content. Return false when the tool has no sidebar.
    fn sidebar(&mut self, _ui: &mut egui::Ui, _cx: &mut ToolContext) -> bool {
        false
    }

    /// Optional toolbar row under the environment banner.
    fn toolbar(&mut self, _ui: &mut egui::Ui, _cx: &mut ToolContext) {}

    fn commands(&self) -> Vec<CommandDescriptor> {
        Vec::new()
    }

    fn run_command(&mut self, _id: &str, _cx: &mut ToolContext) -> bool {
        false
    }

    /// The file/connection this tab shows, so the shell can focus an existing
    /// tab instead of opening the same thing twice (`file://` path, `sftp://`
    /// URI, connection id, …).
    fn current_uri(&self) -> Option<String> {
        None
    }

    /// How well this tab would take a request for `uri` that no tab's
    /// `current_uri` matches; lower is better, `None` declines. The editor
    /// answers 0 for a file it has open or its own project folder, 1 + depth
    /// for a file inside its project, 200 (Other Files or an empty tab) for
    /// any other local file.
    fn uri_affinity(&self, _uri: &str) -> Option<u8> {
        None
    }

    /// The connection profile id this tab is bound to, so a request to open
    /// that connection (launcher chip, palette) focuses this tab instead of
    /// opening another one.
    fn current_connection(&self) -> Option<&str> {
        None
    }

    /// Items for the quick-open palette (project files, saved queries, …).
    fn quick_open_items(&self) -> Vec<QuickOpenItem> {
        Vec::new()
    }

    /// Persistence for workspace restore. Must not include secrets.
    fn save_state(&self) -> serde_json::Value {
        serde_json::Value::Null
    }

    fn on_close(&mut self) -> CloseDecision {
        if self.dirty() {
            CloseDecision::AskUnsavedChanges
        } else {
            CloseDecision::Close
        }
    }

    fn open(&mut self, _item: OpenRequest, _cx: &mut ToolContext) -> bool {
        false
    }

    fn on_event(&mut self, _ev: &Event, _cx: &mut ToolContext) {}
}

pub trait ToolFactory: Send + Sync {
    fn descriptor(&self) -> &ToolDescriptor;
    fn create(&self, services: &Services, restore: Option<serde_json::Value>) -> Box<dyn Tool>;
}
