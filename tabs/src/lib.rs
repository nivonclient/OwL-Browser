use std::fmt;

use util::IdGenerator;

/// Stable identifier for a browser tab.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct TabId(u64);

impl TabId {
    /// Creates a new `TabId` from a raw numeric value.
    pub fn new(raw: u64) -> Self {
        Self(raw)
    }

    /// Returns the raw numeric value.
    pub fn get(self) -> u64 {
        self.0
    }
}

impl fmt::Display for TabId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// High-level lifecycle state for a tab.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum TabState {
    Active,
    Background,
    Suspended,
}

/// Lightweight tab record owned by the tab manager.
#[derive(Debug, Clone)]
pub struct TabEntry {
    pub id: TabId,
    pub state: TabState,
}

/// Interface for tab lifecycle and state management.
pub trait TabManager {
    /// Creates a new tab and returns its record.
    fn create_tab(&mut self) -> TabEntry;

    /// Marks the specified tab as active.
    fn set_active(&mut self, id: TabId) -> bool;

    /// Updates the state for a tab.
    fn set_state(&mut self, id: TabId, state: TabState) -> bool;

    /// Returns the currently active tab, if any.
    fn active_tab(&self) -> Option<TabId>;

    /// Returns the ordered list of tabs.
    fn tabs(&self) -> &[TabEntry];
}

/// Minimal in-memory tab manager suitable for early scaffolding.
#[derive(Debug, Default)]
pub struct BasicTabManager {
    tabs: Vec<TabEntry>,
    active: Option<TabId>,
    ids: IdGenerator,
}

impl BasicTabManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn next_tab(&self) -> Option<TabId> {
        let active = self.active?;
        let idx = self.tabs.iter().position(|tab| tab.id == active)?;
        let next_idx = (idx + 1) % self.tabs.len();
        Some(self.tabs[next_idx].id)
    }
}

impl TabManager for BasicTabManager {
    fn create_tab(&mut self) -> TabEntry {
        for tab in &mut self.tabs {
            if tab.state == TabState::Active {
                tab.state = TabState::Background;
            }
        }

        let id = TabId::new(self.ids.next());
        let entry = TabEntry {
            id,
            state: TabState::Active,
        };
        self.tabs.push(entry.clone());
        self.active = Some(id);
        entry
    }

    fn set_active(&mut self, id: TabId) -> bool {
        if !self.tabs.iter().any(|tab| tab.id == id) {
            return false;
        }

        for tab in &mut self.tabs {
            if tab.id == id {
                tab.state = TabState::Active;
            } else if tab.state != TabState::Suspended {
                tab.state = TabState::Background;
            }
        }

        self.active = Some(id);
        true
    }

    fn set_state(&mut self, id: TabId, state: TabState) -> bool {
        let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id) else {
            return false;
        };

        tab.state = state;
        if state == TabState::Active {
            self.active = Some(id);
        } else if self.active == Some(id) {
            self.active = None;
        }

        true
    }

    fn active_tab(&self) -> Option<TabId> {
        self.active
    }

    fn tabs(&self) -> &[TabEntry] {
        &self.tabs
    }
}
