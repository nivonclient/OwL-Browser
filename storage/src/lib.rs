use tabs::{TabId, TabState};

/// Snapshot of a single tab for session restore.
#[derive(Debug, Clone)]
pub struct TabSnapshot {
    pub id: TabId,
    pub uri: String,
    pub state: TabState,
}

/// Snapshot of a browsing session.
#[derive(Debug, Clone, Default)]
pub struct SessionSnapshot {
    pub tabs: Vec<TabSnapshot>,
    pub active: Option<TabId>,
}

/// Interface for session and tab persistence.
pub trait SessionStore {
    /// Loads the latest stored session, if available.
    fn load(&self) -> Option<SessionSnapshot>;

    /// Persists the provided session snapshot.
    fn save(&self, session: &SessionSnapshot);
}

/// No-op session store used during scaffolding.
#[derive(Debug, Default)]
pub struct NoopSessionStore;

impl SessionStore for NoopSessionStore {
    fn load(&self) -> Option<SessionSnapshot> {
        None
    }

    fn save(&self, _session: &SessionSnapshot) {
        // TODO: Persist session state to disk.
    }
}
