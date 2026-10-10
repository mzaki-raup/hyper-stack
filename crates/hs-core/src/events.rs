//! Typed broadcast events between services, tools and the shell. Producers
//! publish from any thread; the UI drains a queue at frame start and tools get
//! `on_event` calls.

use std::sync::Arc;

use parking_lot::Mutex;

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    EnvironmentSwitched {
        env_id: Option<String>,
    },
    ConnectionsChanged,
    EnvironmentsChanged,
    SettingsChanged,
    ThemeChanged,
    WorkspaceSwitched {
        name: String,
    },
    FileSaved {
        uri: String,
    },
    JobFinished {
        id: u64,
        ok: bool,
    },
    VaultLocked,
    VaultUnlocked,
    UpdateAvailable {
        version: String,
    },
    /// Free-form, for tools talking to themselves across threads.
    Custom {
        topic: String,
        payload: String,
    },
}

#[derive(Clone, Default)]
pub struct EventBus {
    queue: Arc<Mutex<Vec<Event>>>,
}

impl EventBus {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn publish(&self, ev: Event) {
        self.queue.lock().push(ev);
    }

    /// Take everything published since the last drain (called once per frame).
    pub fn drain(&self) -> Vec<Event> {
        std::mem::take(&mut *self.queue.lock())
    }

    pub fn pending(&self) -> usize {
        self.queue.lock().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drain_returns_in_order_and_empties() {
        let bus = EventBus::new();
        bus.publish(Event::ConnectionsChanged);
        bus.publish(Event::ThemeChanged);
        assert_eq!(
            bus.drain(),
            vec![Event::ConnectionsChanged, Event::ThemeChanged]
        );
        assert!(bus.drain().is_empty());
    }
}
