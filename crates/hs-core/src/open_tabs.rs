//! What every activity has open, so a tool's home tab can list it.
//!
//! A tool only ever sees itself: it is handed a `Ui` and its own state, and
//! knows nothing about the other tabs in the window. A dashboard has to
//! show exactly that, so the shell publishes the open tabs here once a
//! frame and a tool reads back the ones belonging to its own activity.
//!
//! The thumbnail is a URI for an image already registered with egui, not
//! an image: `hs-core` has no business holding pixels, and the shell is
//! the only place that can capture them. It is the **last** picture taken
//! of that tab while it was on screen, so a tab never visited this session
//! has none. Owner's requirement, 2026-10-17.

use std::sync::Arc;

use parking_lot::RwLock;

/// One open window tab, as a dashboard sees it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TabInfo {
    pub id: u64,
    /// Which activity it belongs to (`"db"`, `"ssh"`, …).
    pub factory_id: String,
    pub title: String,
    /// A second line: the connection, the engine, the file — whatever the
    /// tool puts in its tooltip.
    pub subtitle: String,
    /// An egui image URI for the last picture taken of this tab's pane.
    pub thumbnail: Option<String>,
    /// This is the activity's home tab, so a dashboard leaves itself out
    /// of its own grid.
    pub home: bool,
    /// The tab the user is looking at.
    pub active: bool,
}

/// The shell's view of the window, shared with every tool.
#[derive(Clone, Default)]
pub struct OpenTabs {
    inner: Arc<RwLock<Vec<TabInfo>>>,
}

impl OpenTabs {
    pub fn new() -> Self {
        Self::default()
    }

    /// Replace the list. Called once a frame by the shell.
    pub fn set(&self, tabs: Vec<TabInfo>) {
        *self.inner.write() = tabs;
    }

    /// Every open tab.
    pub fn all(&self) -> Vec<TabInfo> {
        self.inner.read().clone()
    }

    /// The tabs of one activity that a dashboard should list: its own, in
    /// strip order, without the dashboard itself.
    pub fn of(&self, factory_id: &str) -> Vec<TabInfo> {
        self.inner
            .read()
            .iter()
            .filter(|t| t.factory_id == factory_id && !t.home)
            .cloned()
            .collect()
    }

    /// Remember a picture of one tab. The shell calls this after a
    /// capture; the URI points at bytes already given to egui.
    pub fn set_thumbnail(&self, id: u64, uri: String) {
        if let Some(t) = self.inner.write().iter_mut().find(|t| t.id == id) {
            t.thumbnail = Some(uri);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tab(id: u64, factory: &str, home: bool) -> TabInfo {
        TabInfo {
            id,
            factory_id: factory.into(),
            title: format!("tab {id}"),
            home,
            ..Default::default()
        }
    }

    #[test]
    fn a_dashboard_sees_its_own_activity_and_not_itself() {
        let tabs = OpenTabs::new();
        tabs.set(vec![
            tab(1, "db", true),
            tab(2, "db", false),
            tab(3, "ssh", false),
        ]);
        let db: Vec<u64> = tabs.of("db").iter().map(|t| t.id).collect();
        assert_eq!(db, vec![2], "the home tab is not in its own grid");
        assert_eq!(tabs.of("ssh").len(), 1);
        assert!(tabs.of("redis").is_empty());
    }

    #[test]
    fn a_thumbnail_attaches_to_the_tab_it_was_taken_of() {
        let tabs = OpenTabs::new();
        tabs.set(vec![tab(1, "db", false), tab(2, "db", false)]);
        tabs.set_thumbnail(2, "bytes://thumb-2".into());
        let got = tabs.of("db");
        assert_eq!(got[0].thumbnail, None);
        assert_eq!(got[1].thumbnail.as_deref(), Some("bytes://thumb-2"));
        // A tab that has gone is not an error, just nothing to attach to.
        tabs.set_thumbnail(99, "bytes://nowhere".into());
    }
}
