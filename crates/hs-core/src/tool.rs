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
        /// Typed into the new shell once it runs (the editor's Run).
        command: Option<String>,
    },
    /// A web address for the browser tool (Shift+click on a link elsewhere).
    Url {
        url: String,
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

/// A piece of text a tool can offer to be read aloud (Accessibility → Read
/// Aloud): the editor's selection, the terminal screen, a table cell, the
/// assistant's last answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Speakable {
    pub label: String,
    pub text: String,
}

impl Speakable {
    pub fn new(label: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            text: text.into(),
        }
    }
}

/// What a tool hands the shell for Print… (docs/04 §11): a title, a
/// subtitle (connection, path, environment) and an HTML body built with
/// `crate::html`. The shell wraps it in a page and opens it in the browser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Printable {
    pub title: String,
    pub subtitle: String,
    pub html: String,
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

    /// An image for this tool's tab, as an egui image URI, instead of the
    /// descriptor's glyph. The browser returns the page's favicon; every
    /// other tool leaves it `None`.
    fn tab_icon_uri(&self) -> Option<String> {
        None
    }

    /// A short piece drawn before the tab's title in a colour of its own —
    /// the API client's HTTP method. The title keeps the normal colour.
    fn tab_title_prefix(&self) -> Option<(String, egui::Color32)> {
        None
    }

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

    /// Whether a second window tab of this tool is meaningful at all.
    /// False for a tool whose tab *is* the thing it holds rather than a
    /// view onto it — Graphics, where a tab is one open image, and the
    /// browser, where a tab is the workspace's browser. Duplicating those
    /// produces a second tab claiming to be the same thing. Owner's
    /// requirement, 2026-10-17.
    fn can_duplicate_tab(&self) -> bool {
        true
    }

    /// How many window tabs one connection may have. A tab bound to a
    /// connection is a view of a live session, so a second one is a second
    /// session against the same server rather than another look at the
    /// same thing — which is why Duplicate is refused for them. SSH is the
    /// exception: separate shells on one host are the normal way to work.
    ///
    /// A tool that does not implement [`Tool::current_connection`] is not
    /// connection-bound and is unaffected, so a new activity type gets the
    /// rule by implementing that one method. Owner's requirement,
    /// 2026-10-17.
    fn tabs_per_connection(&self) -> usize {
        1
    }

    /// Items for the quick-open palette (project files, saved queries, …).
    fn quick_open_items(&self) -> Vec<QuickOpenItem> {
        Vec::new()
    }

    /// Texts this tab can read aloud, most specific first (selection before
    /// whole file). Empty when nothing is readable.
    fn speakable(&self) -> Vec<Speakable> {
        Vec::new()
    }

    /// What Print… exports for this tab: code, a Markdown preview, a query
    /// with its result table, a console screen, a listing. `None` when the
    /// tab has nothing printable (Snip still captures it as a picture).
    fn printable(&self) -> Option<Printable> {
        None
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
