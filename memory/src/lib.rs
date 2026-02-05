use tabs::TabId;

pub mod pressure;

/// Snapshot of memory usage for a tab process.
#[derive(Debug, Clone, Copy, Default)]
pub struct MemorySnapshot {
    pub resident_bytes: u64,
}

/// Interface for capturing and storing memory usage data.
pub trait MemoryTracker {
    /// Records a snapshot for the given tab.
    fn record_snapshot(&mut self, tab: TabId, snapshot: MemorySnapshot);

    /// Returns the latest snapshot for the given tab.
    fn latest_snapshot(&self, tab: TabId) -> Option<MemorySnapshot>;
}

/// No-op tracker used until real telemetry is wired.
#[derive(Debug, Default)]
pub struct NoopMemoryTracker;

impl MemoryTracker for NoopMemoryTracker {
    fn record_snapshot(&mut self, _tab: TabId, _snapshot: MemorySnapshot) {
        // TODO: Store per-tab memory snapshots for policy enforcement.
    }

    fn latest_snapshot(&self, _tab: TabId) -> Option<MemorySnapshot> {
        None
    }
}
