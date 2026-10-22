//! Shared state for the Graphics activity: the function catalogue, the
//! recent-work list and the image -> icon handoff.
//!
//! Every Graphics tab draws the same sidebar from this state, following the
//! Devtools pattern. The data lives in the portable data folder, not in the
//! workspace, because a workspace may be an external `.hyperstack/` folder in
//! a git repository (`docs/tools/diagrams.md` §8).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use hs_core::Services;
use hs_ui::icons::ph;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

pub type SharedGraphics = Arc<Mutex<GraphicsHub>>;

/// What a Graphics tab holds. `Home` is the activity's permanent dashboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Image,
    Icon,
    Diagram,
    Generator,
    Home,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Image => "image",
            Self::Icon => "icon",
            Self::Diagram => "diagram",
            Self::Generator => "generator",
            Self::Home => "home",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "image" => Some(Self::Image),
            "icon" => Some(Self::Icon),
            "diagram" => Some(Self::Diagram),
            "generator" => Some(Self::Generator),
            "home" => Some(Self::Home),
            _ => None,
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::Image => ph::IMAGE,
            Self::Icon => ph::APP_WINDOW,
            Self::Diagram => ph::TREE_STRUCTURE,
            Self::Generator => ph::SPARKLE,
            Self::Home => ph::HOUSE,
        }
    }

    pub fn label_key(self) -> &'static str {
        match self {
            Self::Image => "gfx-fn-image",
            Self::Icon => "gfx-fn-icon",
            Self::Diagram => "gfx-fn-diagram",
            Self::Generator => "gfx-fn-generator",
            Self::Home => "tool-graphics",
        }
    }
}

/// One row in the Recent section.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Recent {
    pub kind: Kind,
    pub title: String,
    pub path: Option<String>,
    pub touched: u64,
}

/// The four tools the activity offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Function {
    Image,
    Icon,
    Diagram,
    Generator,
}

impl Function {
    pub const ALL: [Function; 4] = [
        Function::Image,
        Function::Icon,
        Function::Diagram,
        Function::Generator,
    ];

    pub fn kind(self) -> Kind {
        match self {
            Self::Image => Kind::Image,
            Self::Icon => Kind::Icon,
            Self::Diagram => Kind::Diagram,
            Self::Generator => Kind::Generator,
        }
    }

    pub fn icon(self) -> &'static str {
        self.kind().icon()
    }

    pub fn title_key(self) -> &'static str {
        self.kind().label_key()
    }

    pub fn description_key(self) -> &'static str {
        match self {
            Self::Image => "gfx-fn-image-desc",
            Self::Icon => "gfx-fn-icon-desc",
            Self::Diagram => "gfx-fn-diagram-desc",
            Self::Generator => "gfx-fn-generator-desc",
        }
    }

    /// The glyph of the action that opens this tool, shared by the
    /// dashboard's What To Do tiles and its Functions row.
    pub fn action_icon(self) -> &'static str {
        match self {
            Self::Image => ph::IMAGE,
            Self::Icon => ph::APP_WINDOW,
            Self::Diagram => ph::TREE_STRUCTURE,
            Self::Generator => ph::SPARKLE,
        }
    }

    /// The label of the action that opens this tool.
    pub fn action_key(self) -> &'static str {
        match self {
            Self::Image => "gfx-open",
            Self::Icon => "gfx-open-icon",
            Self::Diagram => "gfx-new-diagram",
            Self::Generator => "gfx-new-generation",
        }
    }

    /// All four functions are live in Release 1.
    pub fn enabled(self) -> bool {
        matches!(
            self,
            Self::Image | Self::Icon | Self::Diagram | Self::Generator
        )
    }
}

/// The activity's shared state. `Default` is an in-memory hub for tests and
/// for an empty tab; `shared` loads the persisted recents.
#[derive(Default)]
pub struct GraphicsHub {
    store: Option<PathBuf>,
    recents: Vec<Recent>,
    pending_icon: Option<image::DynamicImage>,
}

impl GraphicsHub {
    /// An in-memory hub for an empty tab or a test.
    pub fn memory() -> SharedGraphics {
        Arc::new(Mutex::new(Self::default()))
    }

    pub fn shared(services: &Services) -> SharedGraphics {
        let store = recents_path(services);
        let recents = hs_core::storage::read_json::<Vec<Recent>>(&store).unwrap_or_default();
        Arc::new(Mutex::new(Self {
            store: Some(store),
            recents,
            pending_icon: None,
        }))
    }

    pub fn recents(&self) -> Vec<Recent> {
        let mut out = self.recents.clone();
        out.sort_by_key(|r| std::cmp::Reverse(r.touched));
        out
    }

    /// Remember a saved picture or icon. Unsaved work has no path yet, so it
    /// does not enter Recent until it is saved.
    pub fn touch(&mut self, kind: Kind, title: &str, path: Option<String>) {
        let Some(path) = path else { return };
        let touched = now();
        if let Some(i) = self
            .recents
            .iter()
            .position(|r| r.path.as_deref() == Some(path.as_str()))
        {
            self.recents.remove(i);
        }
        self.recents.insert(
            0,
            Recent {
                kind,
                title: title.to_string(),
                path: Some(path),
                touched,
            },
        );
        self.recents.retain(|r| r.kind != Kind::Home);
        self.recents.truncate(50);
        self.save();
    }

    pub fn remove_path(&mut self, path: &str) {
        self.recents.retain(|r| r.path.as_deref() != Some(path));
        self.save();
    }

    pub fn set_pending_icon(&mut self, img: image::DynamicImage) {
        self.pending_icon = Some(img);
    }

    pub fn take_pending_icon(&mut self) -> Option<image::DynamicImage> {
        self.pending_icon.take()
    }

    fn save(&self) {
        let Some(store) = &self.store else { return };
        if let Err(e) = hs_core::storage::write_json(store, &self.recents) {
            tracing::warn!("graphics recents not saved: {e}");
        }
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn recents_path(services: &Services) -> PathBuf {
    let name = services.workspace.read().meta.name.clone();
    services
        .paths
        .root()
        .join("graphics")
        .join(slug(&name))
        .join("recents.json")
}

fn slug(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for c in name.chars() {
        if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
            out.push(c.to_ascii_lowercase());
        } else if c.is_whitespace() {
            out.push('-');
        }
    }
    let out = out.trim_matches('-').to_string();
    if out.is_empty() {
        "workspace".into()
    } else {
        out.chars().take(64).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recents_dedupe_by_path_and_keep_the_newest_first() {
        let mut hub = GraphicsHub::default();
        hub.touch(Kind::Image, "a.png", Some("C:/tmp/a.png".into()));
        hub.touch(Kind::Icon, "b.ico", Some("C:/tmp/b.ico".into()));
        hub.touch(Kind::Image, "a2.png", Some("C:/tmp/a.png".into()));
        let list = hub.recents();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].title, "a2.png");
        assert_eq!(list[1].title, "b.ico");
        hub.remove_path("C:/tmp/a.png");
        assert_eq!(hub.recents().len(), 1);
    }

    #[test]
    fn a_recent_without_a_path_is_not_stored() {
        let mut hub = GraphicsHub::default();
        hub.touch(Kind::Image, "unsaved", None);
        assert!(hub.recents().is_empty());
    }
}
