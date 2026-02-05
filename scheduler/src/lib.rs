#![cfg_attr(
    not(feature = "diagnostics"),
    doc = r#"```compile_fail
use crate::ExecutionFeedbackSnapshot;

fn main() {}
```
Diagnostics symbols are intentionally unavailable without the `diagnostics` feature.
This is a structural contract test; do not remove.
"#
)]

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
#[cfg(feature = "diagnostics")]
use std::fmt;
use std::rc::Rc;
use std::time::{Duration, Instant};

use tabs::{TabId, TabState};

/// Simple execution budget tiers used as policy signals.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum BudgetTier {
    /// Active tab with immediate user intent.
    Foreground,
    /// Background tab that is allowed to run but at a reduced priority.
    VisibleBackground,
    /// Background tab allowed to run only in short idle bursts.
    IdleBackground,
}

impl Default for BudgetTier {
    fn default() -> Self {
        Self::Foreground
    }
}

/// Coarse memory pressure signal.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum MemoryPressure {
    Low,
    Moderate,
    Severe,
}

impl Default for MemoryPressure {
    fn default() -> Self {
        Self::Low
    }
}

/// Placeholder execution budget assigned to a tab.
///
/// Budgets are policy signals used by the scheduler to gate effective tab
/// states. They do not measure real JS CPU time yet.
#[derive(Debug, Clone, Copy, Default, Eq, PartialEq)]
pub struct ExecutionBudget {
    /// Current budget tier for policy-driven scheduling.
    pub tier: BudgetTier,
}

/// Observational execution feedback from the engine.
///
/// These signals are advisory only and do not alter execution by themselves.
/// The scheduler may use them as hints, but they are not authoritative.
#[derive(Debug, Clone, Copy, Default, Eq, PartialEq)]
pub struct EngineExecutionFeedback {
    /// Indicates recent long-running JavaScript tasks.
    ///
    /// This does not guarantee the absence of long tasks when false.
    pub has_long_tasks: bool,

    /// Approximate number of active workers (coarse, non-exact).
    ///
    /// This is a best-effort estimate and may be stale.
    pub worker_count: u16,

    /// Whether WebAssembly execution is currently active.
    ///
    /// This does not guarantee that no Wasm is running when false.
    pub wasm_active: bool,

    /// Whether JavaScript appears to be blocking rendering.
    ///
    /// This is a coarse signal and does not identify the cause of jank.
    pub js_blocking_render: bool,
}

/// Poll-based engine feedback provider.
///
/// Feedback is observational only; it must not enforce policy.
pub trait EngineFeedbackProvider {
    /// Polls execution feedback for a tab.
    ///
    /// This must be cheap, non-blocking, and allocation-free.
    fn poll_execution_feedback(&self, tab: TabId) -> EngineExecutionFeedback;
}

/// Advisory signals derived from budgets and memory pressure.
///
/// These hints do not enforce behavior and must not change JS semantics on their own.
/// The engine layer may choose to apply them or ignore them based on capability.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct ExecutionBudgetHints {
    /// Optional maximum timer frequency; `None` means no clamp requested.
    pub max_timer_frequency: Option<Duration>,
    /// Whether background JavaScript is generally allowed to run.
    pub allow_background_js: bool,
    /// Whether WebAssembly should be allowed under current policy.
    pub allow_wasm: bool,
    /// Whether workers should be allowed under current policy.
    pub allow_workers: bool,
    /// Hint that the engine may prefer to suspend if safe.
    pub prefer_suspend: bool,
}

impl ExecutionBudgetHints {
    const fn new(
        max_timer_frequency: Option<Duration>,
        allow_background_js: bool,
        allow_wasm: bool,
        allow_workers: bool,
        prefer_suspend: bool,
    ) -> Self {
        Self {
            max_timer_frequency,
            allow_background_js,
            allow_wasm,
            allow_workers,
            prefer_suspend,
        }
    }
}

/// Maps a budget + pressure signal into advisory hints.
///
/// This mapping is monotonic: Severe ⊆ Moderate ⊆ Low.
pub fn map_execution_hints(
    budget: ExecutionBudget,
    pressure: MemoryPressure,
) -> ExecutionBudgetHints {
    const TIMER_20HZ: Duration = Duration::from_millis(50);
    const TIMER_10HZ: Duration = Duration::from_millis(100);
    const TIMER_4HZ: Duration = Duration::from_millis(250);
    const TIMER_2HZ: Duration = Duration::from_millis(500);
    const TIMER_1HZ: Duration = Duration::from_millis(1000);
    const TIMER_0_5HZ: Duration = Duration::from_millis(2000);

    match budget.tier {
        BudgetTier::Foreground => match pressure {
            MemoryPressure::Low | MemoryPressure::Moderate => ExecutionBudgetHints::new(
                None,
                true,
                true,
                true,
                false,
            ),
            MemoryPressure::Severe => ExecutionBudgetHints::new(
                Some(TIMER_20HZ),
                true,
                false,
                false,
                false,
            ),
        },
        BudgetTier::VisibleBackground => match pressure {
            MemoryPressure::Low => ExecutionBudgetHints::new(
                Some(TIMER_10HZ),
                true,
                true,
                true,
                false,
            ),
            MemoryPressure::Moderate => ExecutionBudgetHints::new(
                Some(TIMER_4HZ),
                true,
                false,
                false,
                false,
            ),
            MemoryPressure::Severe => ExecutionBudgetHints::new(
                Some(TIMER_2HZ),
                false,
                false,
                false,
                true,
            ),
        },
        BudgetTier::IdleBackground => match pressure {
            MemoryPressure::Low => ExecutionBudgetHints::new(
                Some(TIMER_2HZ),
                false,
                false,
                false,
                true,
            ),
            MemoryPressure::Moderate => ExecutionBudgetHints::new(
                Some(TIMER_1HZ),
                false,
                false,
                false,
                true,
            ),
            MemoryPressure::Severe => ExecutionBudgetHints::new(
                Some(TIMER_0_5HZ),
                false,
                false,
                false,
                true,
            ),
        },
    }
}

/// Engine-facing hooks used by the scheduler without exposing engine types.
pub trait EngineScheduler: EngineFeedbackProvider {
    /// Applies a tab state transition at the engine level.
    fn apply_tab_state(&self, tab: TabId, state: TabState);

    /// Applies a budget to the engine for the given tab.
    fn apply_execution_budget(&self, tab: TabId, budget: ExecutionBudget);

    /// Applies advisory execution hints for the given tab.
    ///
    /// These are intent signals only; the engine may ignore them to preserve compatibility.
    fn apply_execution_hints(&self, tab: TabId, hints: ExecutionBudgetHints);
}

/// Internal storage for engine execution feedback.
///
/// Feedback may be stale or incomplete; absence of a signal does not imply
/// absence of activity. This is observational only, and the scheduler does
/// not derive policy or change execution based on this state yet.
#[cfg(feature = "diagnostics")]
struct ExecutionFeedbackState {
    per_tab: HashMap<TabId, FeedbackRecord>,
}

#[cfg(feature = "diagnostics")]
#[derive(Debug, Clone, Copy)]
struct FeedbackRecord {
    feedback: EngineExecutionFeedback,
    updated_in_last_sample: bool,
    last_sampled_at: Instant,
    sample_count: u32,
}

#[cfg(feature = "diagnostics")]
impl FeedbackRecord {
    fn new(feedback: EngineExecutionFeedback, sampled_at: Instant) -> Self {
        Self {
            feedback,
            updated_in_last_sample: true,
            last_sampled_at: sampled_at,
            sample_count: 1,
        }
    }

    fn update(&mut self, feedback: EngineExecutionFeedback, sampled_at: Instant) -> bool {
        let changed = self.feedback != feedback;
        if changed {
            self.feedback = feedback;
        }
        self.updated_in_last_sample = changed;
        self.last_sampled_at = sampled_at;
        self.sample_count = self.sample_count.saturating_add(1);
        changed
    }

    fn staleness_tag(&self) -> FeedbackStalenessTag {
        if self.updated_in_last_sample {
            FeedbackStalenessTag::Fresh
        } else {
            FeedbackStalenessTag::Stale
        }
    }
}

#[cfg(feature = "diagnostics")]
impl ExecutionFeedbackState {
    fn new() -> Self {
        Self {
            per_tab: HashMap::new(),
        }
    }

    fn update_for_tab(
        &mut self,
        tab: TabId,
        provider: &dyn EngineFeedbackProvider,
        sampled_at: Instant,
    ) -> bool {
        // Feedback may be stale or incomplete; absence of a signal does not
        // imply the absence of activity.
        let feedback = provider.poll_execution_feedback(tab);
        if let Some(existing) = self.per_tab.get_mut(&tab) {
            return existing.update(feedback, sampled_at);
        }

        self.per_tab.insert(tab, FeedbackRecord::new(feedback, sampled_at));
        true
    }
}

/// Read-only snapshot view of stored execution feedback.
///
/// Snapshot data is observational only, may be stale, and must not drive
/// policy or execution changes. It is safe for logging and metrics only and
/// is not a stable contract.
#[cfg(feature = "diagnostics")]
pub struct ExecutionFeedbackSnapshot<'a> {
    state: std::cell::Ref<'a, ExecutionFeedbackState>,
    tab: TabId,
}

/// Staleness tag for engine feedback snapshots.
///
/// This is a conservative, heuristic tag derived only from scheduler-side
/// sampling opportunities and may be inaccurate.
#[cfg(feature = "diagnostics")]
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum FeedbackStalenessTag {
    /// Feedback was updated during the most recent sampling opportunity.
    Fresh,
    /// Feedback was sampled recently but not updated.
    Stale,
    /// Feedback has never been sampled for this tab.
    Unknown,
}

/// Coarse age classification for execution feedback.
///
/// This is heuristic and derived solely from scheduler-side sampling events.
#[cfg(feature = "diagnostics")]
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum FeedbackAgeClass {
    Recent,
    Aging,
    Expired,
}

/// Configurable aging windows for feedback observations.
///
/// These windows are diagnostic only and must not influence scheduling.
#[cfg(feature = "diagnostics")]
#[derive(Debug, Clone, Copy)]
pub struct FeedbackAgingWindows {
    pub recent: Duration,
    pub expired: Duration,
}

#[cfg(feature = "diagnostics")]
impl FeedbackAgingWindows {
    /// Classifies a feedback age into a coarse bucket.
    pub fn classify(&self, age: Duration) -> FeedbackAgeClass {
        if age <= self.recent {
            FeedbackAgeClass::Recent
        } else if age <= self.expired {
            FeedbackAgeClass::Aging
        } else {
            FeedbackAgeClass::Expired
        }
    }
}

/// Aggregated, read-only snapshot of stored execution feedback.
///
/// Snapshot data may be stale or incomplete and must not be used to derive
/// scheduling or execution policy. This view is intended only for diagnostics,
/// logging, and metrics and is not a stable contract.
#[cfg(feature = "diagnostics")]
pub struct ExecutionFeedbackAggregate<'a> {
    state: std::cell::Ref<'a, ExecutionFeedbackState>,
}

/// Aggregate counts of feedback ages.
///
/// These metrics are heuristic and diagnostic only. They may be stale and
/// must not be used to infer execution pressure or scheduling intent.
#[cfg(feature = "diagnostics")]
#[derive(Debug, Clone, Copy, Default)]
pub struct FeedbackAgeDistribution {
    pub recent: usize,
    pub aging: usize,
    pub expired: usize,
}

/// Compact, diagnostic debug line for per-tab feedback.
///
/// Output is observational only and may be stale or incomplete. It must not be
/// interpreted as policy or execution intent and is not a stable contract.
#[cfg(feature = "diagnostics")]
pub struct ExecutionFeedbackDebugLine<'a> {
    snapshot: &'a ExecutionFeedbackSnapshot<'a>,
    include_staleness: bool,
    age_windows: Option<FeedbackAgingWindows>,
}

#[cfg(feature = "diagnostics")]
impl<'a> fmt::Display for ExecutionFeedbackDebugLine<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let feedback = self.snapshot.feedback();
        write!(
            f,
            "tab={} long_tasks={} workers={} wasm={} js_blocking_render={}",
            self.snapshot.tab(),
            feedback.has_long_tasks,
            feedback.worker_count,
            feedback.wasm_active,
            feedback.js_blocking_render
        )?;

        if self.include_staleness {
            write!(f, " staleness={:?}", self.snapshot.staleness_tag())?;
        }

        if let Some(windows) = self.age_windows {
            write!(f, " age_class={:?}", self.snapshot.age_class(windows))?;
        }

        Ok(())
    }
}

/// Structured diagnostic report for aggregated feedback.
///
/// Output is observational only, may be stale, and must not be used to infer
/// execution pressure or scheduling intent. It is not a stable contract.
#[cfg(feature = "diagnostics")]
pub struct ExecutionFeedbackAggregateReport<'a> {
    aggregate: &'a ExecutionFeedbackAggregate<'a>,
    include_staleness: bool,
    age_windows: FeedbackAgingWindows,
}

#[cfg(feature = "diagnostics")]
impl<'a> fmt::Display for ExecutionFeedbackAggregateReport<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let now = Instant::now();
        let mut dist = FeedbackAgeDistribution::default();
        let mut long_tasks = 0usize;
        let mut wasm_active = 0usize;
        let mut js_blocking = 0usize;
        let mut fresh = 0usize;
        let mut stale = 0usize;
        let mut unknown = 0usize;
        let mut total_ns: u128 = 0;
        let mut max_ns: u128 = 0;
        let mut count: u128 = 0;

        for record in self.aggregate.state.per_tab.values() {
            count += 1;
            if record.feedback.has_long_tasks {
                long_tasks += 1;
            }
            if record.feedback.wasm_active {
                wasm_active += 1;
            }
            if record.feedback.js_blocking_render {
                js_blocking += 1;
            }

            if self.include_staleness {
                match record.staleness_tag() {
                    FeedbackStalenessTag::Fresh => fresh += 1,
                    FeedbackStalenessTag::Stale => stale += 1,
                    FeedbackStalenessTag::Unknown => unknown += 1,
                }
            }

            let age = now.duration_since(record.last_sampled_at);
            total_ns += age.as_nanos();
            if age.as_nanos() > max_ns {
                max_ns = age.as_nanos();
            }

            match self.age_windows.classify(age) {
                FeedbackAgeClass::Recent => dist.recent += 1,
                FeedbackAgeClass::Aging => dist.aging += 1,
                FeedbackAgeClass::Expired => dist.expired += 1,
            }
        }

        write!(
            f,
            "sampled_tabs={} long_tasks={} wasm_active={} js_blocking_render={} ",
            count,
            long_tasks,
            wasm_active,
            js_blocking
        )?;

        if self.include_staleness {
            write!(
                f,
                "staleness{{fresh={} stale={} unknown={}}} ",
                fresh, stale, unknown
            )?;
        }

        write!(
            f,
            "age{{recent={} aging={} expired={}}} ",
            dist.recent, dist.aging, dist.expired
        )?;

        if count == 0 {
            write!(f, "max_age_ms=NA avg_age_ms=NA")?;
        } else {
            let avg_ns = total_ns / count;
            let max_ms = max_ns / 1_000_000;
            let avg_ms = avg_ns / 1_000_000;
            write!(f, "max_age_ms={} avg_age_ms={}", max_ms, avg_ms)?;
        }

        Ok(())
    }
}

#[cfg(feature = "diagnostics")]
impl<'a> ExecutionFeedbackAggregate<'a> {
    /// Returns the number of tabs with sampled feedback.
    pub fn sampled_tab_count(&self) -> usize {
        self.state.per_tab.len()
    }

    /// Counts tabs by staleness tag.
    ///
    /// Note: `Unknown` is expected to be zero because only sampled tabs are stored.
    pub fn count_by_staleness(&self, tag: FeedbackStalenessTag) -> usize {
        self.state
            .per_tab
            .values()
            .filter(|record| record.staleness_tag() == tag)
            .count()
    }

    /// Counts tabs reporting recent long-running tasks.
    pub fn count_long_tasks(&self) -> usize {
        self.state
            .per_tab
            .values()
            .filter(|record| record.feedback.has_long_tasks)
            .count()
    }

    /// Counts tabs where WebAssembly appears active.
    pub fn count_wasm_active(&self) -> usize {
        self.state
            .per_tab
            .values()
            .filter(|record| record.feedback.wasm_active)
            .count()
    }

    /// Counts tabs where JavaScript appears to be blocking rendering.
    pub fn count_js_blocking_render(&self) -> usize {
        self.state
            .per_tab
            .values()
            .filter(|record| record.feedback.js_blocking_render)
            .count()
    }

    /// Returns a diagnostic report suitable for logging or debugging.
    ///
    /// Output is observational only and must not be used to derive policy.
    pub fn debug_report(
        &'a self,
        age_windows: FeedbackAgingWindows,
        include_staleness: bool,
    ) -> ExecutionFeedbackAggregateReport<'a> {
        ExecutionFeedbackAggregateReport {
            aggregate: self,
            include_staleness,
            age_windows,
        }
    }

    /// Counts tabs by feedback age class using the provided windows.
    ///
    /// Aggregates are diagnostic only and may be stale.
    pub fn count_by_age_class(&self, windows: FeedbackAgingWindows, class: FeedbackAgeClass) -> usize {
        let now = Instant::now();
        self.state
            .per_tab
            .values()
            .filter(|record| windows.classify(now.duration_since(record.last_sampled_at)) == class)
            .count()
    }

    /// Returns a full age distribution for sampled tabs.
    ///
    /// Aggregates are diagnostic only and may be stale.
    pub fn age_distribution(&self, windows: FeedbackAgingWindows) -> FeedbackAgeDistribution {
        let now = Instant::now();
        let mut dist = FeedbackAgeDistribution::default();
        for record in self.state.per_tab.values() {
            match windows.classify(now.duration_since(record.last_sampled_at)) {
                FeedbackAgeClass::Recent => dist.recent += 1,
                FeedbackAgeClass::Aging => dist.aging += 1,
                FeedbackAgeClass::Expired => dist.expired += 1,
            }
        }
        dist
    }

    /// Returns the maximum observed feedback age across sampled tabs.
    ///
    /// Returns `None` when no feedback has been sampled.
    pub fn max_feedback_age(&self) -> Option<Duration> {
        let now = Instant::now();
        self.state
            .per_tab
            .values()
            .map(|record| now.duration_since(record.last_sampled_at))
            .max()
    }

    /// Returns the average feedback age across sampled tabs.
    ///
    /// Returns `None` when no feedback has been sampled.
    pub fn average_feedback_age(&self) -> Option<Duration> {
        let now = Instant::now();
        let mut total: u128 = 0;
        let mut count: u128 = 0;
        for record in self.state.per_tab.values() {
            total += now.duration_since(record.last_sampled_at).as_nanos();
            count += 1;
        }
        if count == 0 {
            None
        } else {
            Some(Duration::from_nanos((total / count) as u64))
        }
    }
}

#[cfg(feature = "diagnostics")]
impl<'a> ExecutionFeedbackSnapshot<'a> {
    /// Returns the tab id associated with this snapshot.
    pub fn tab(&self) -> TabId {
        self.tab
    }

    /// Returns the feedback for the tab.
    pub fn feedback(&self) -> &EngineExecutionFeedback {
        self.state
            .per_tab
            .get(&self.tab)
            .map(|record| &record.feedback)
            .expect("feedback snapshot missing tab entry")
    }

    /// Returns a conservative staleness tag for the snapshot.
    pub fn staleness_tag(&self) -> FeedbackStalenessTag {
        self.state
            .per_tab
            .get(&self.tab)
            .map(|record| record.staleness_tag())
            .unwrap_or(FeedbackStalenessTag::Unknown)
    }

    /// Returns the duration since this tab's feedback was last sampled.
    ///
    /// The age is a diagnostic signal only and must not drive policy.
    pub fn age(&self) -> Duration {
        self.state
            .per_tab
            .get(&self.tab)
            .map(|record| record.last_sampled_at.elapsed())
            .expect("feedback snapshot missing tab entry")
    }

    /// Returns the number of sampling events recorded for this tab.
    pub fn sample_count(&self) -> u32 {
        self.state
            .per_tab
            .get(&self.tab)
            .map(|record| record.sample_count)
            .expect("feedback snapshot missing tab entry")
    }

    /// Returns a conservative age classification for this tab's feedback.
    ///
    /// Classification is heuristic and must not drive scheduling decisions.
    pub fn age_class(&self, windows: FeedbackAgingWindows) -> FeedbackAgeClass {
        windows.classify(self.age())
    }

    /// Returns a compact, log-friendly debug line for this tab.
    ///
    /// Output is observational only and must not be treated as a policy signal.
    pub fn debug_line(
        &'a self,
        include_staleness: bool,
        age_windows: Option<FeedbackAgingWindows>,
    ) -> ExecutionFeedbackDebugLine<'a> {
        ExecutionFeedbackDebugLine {
            snapshot: self,
            include_staleness,
            age_windows,
        }
    }
}

#[cfg(feature = "diagnostics")]
#[derive(Debug, Clone, Copy)]
enum FeedbackSamplingEvent {
    TabStateChange,
    BudgetTierChange,
}

/// Determines when to opportunistically sample engine feedback.
///
/// Sampling frequency is intentionally low and tied to existing scheduler events.
/// This remains extensible for future rate limiting or adaptive sampling.
#[cfg(feature = "diagnostics")]
#[derive(Debug, Clone, Copy)]
struct FeedbackSamplingTrigger {
    sample_on_state_change: bool,
    sample_on_budget_change: bool,
}

#[cfg(feature = "diagnostics")]
impl FeedbackSamplingTrigger {
    fn should_sample(&self, event: FeedbackSamplingEvent) -> bool {
        match event {
            FeedbackSamplingEvent::TabStateChange => self.sample_on_state_change,
            FeedbackSamplingEvent::BudgetTierChange => self.sample_on_budget_change,
        }
    }
}

#[cfg(feature = "diagnostics")]
impl Default for FeedbackSamplingTrigger {
    fn default() -> Self {
        Self {
            sample_on_state_change: true,
            sample_on_budget_change: true,
        }
    }
}

/// Interface for governing JavaScript execution without rewriting scripts.
pub trait JSExecutionGovernor {
    /// Applies a new budget to the tab.
    fn set_budget(&self, tab: TabId, budget: ExecutionBudget);

    /// Notifies the governor that a tab's state has changed.
    fn on_tab_state_changed(&self, tab: TabId, state: TabState);
}

/// Level-1 governor that delegates state changes to the engine.
///
/// This tracks tab states and applies engine-level throttling hooks but does
/// not implement advanced scheduling logic yet.
pub struct ExecutionGovernor {
    engine: Rc<dyn EngineScheduler>,
    states: RefCell<HashMap<TabId, TabState>>,
    budgets: RefCell<HashMap<TabId, ExecutionBudget>>,
    hints: RefCell<HashMap<TabId, ExecutionBudgetHints>>,
    #[cfg(feature = "diagnostics")]
    feedback: RefCell<ExecutionFeedbackState>,
    #[cfg(feature = "diagnostics")]
    feedback_trigger: FeedbackSamplingTrigger,
    effective_states: RefCell<HashMap<TabId, TabState>>,
    last_global_input: Cell<Instant>,
    last_idle_burst: Cell<Instant>,
    last_tab_input: RefCell<HashMap<TabId, Instant>>,
    memory_pressure: Cell<MemoryPressure>,
}

impl ExecutionGovernor {
    pub fn new<E: EngineScheduler + 'static>(engine: Rc<E>) -> Self {
        let now = Instant::now();
        let engine: Rc<dyn EngineScheduler> = engine;
        Self {
            engine,
            states: RefCell::new(HashMap::new()),
            budgets: RefCell::new(HashMap::new()),
            hints: RefCell::new(HashMap::new()),
            #[cfg(feature = "diagnostics")]
            feedback: RefCell::new(ExecutionFeedbackState::new()),
            #[cfg(feature = "diagnostics")]
            feedback_trigger: FeedbackSamplingTrigger::default(),
            effective_states: RefCell::new(HashMap::new()),
            last_global_input: Cell::new(now),
            last_idle_burst: Cell::new(now),
            last_tab_input: RefCell::new(HashMap::new()),
            memory_pressure: Cell::new(MemoryPressure::Low),
        }
    }

    /// Returns the last known state for a tab, if tracked.
    pub fn state(&self, tab: TabId) -> Option<TabState> {
        self.states.borrow().get(&tab).copied()
    }

    /// Records a user interaction for the given tab.
    pub fn record_user_input(&self, tab: TabId) {
        let now = Instant::now();
        self.mark_recent_input(tab, now);
        self.reconcile(now);
    }

    /// Updates memory pressure. This only ever demotes budget tiers.
    pub fn set_memory_pressure(&self, pressure: MemoryPressure) {
        self.memory_pressure.set(pressure);
        self.reconcile(Instant::now());
    }

    /// Polls engine feedback for a tab and stores it if it changed.
    ///
    /// Sampling is opportunistic and may be stale; this is observational only
    /// and does not affect scheduling decisions.
    #[cfg(feature = "diagnostics")]
    pub fn poll_execution_feedback(&self, tab: TabId) {
        self.feedback
            .borrow_mut()
            .update_for_tab(tab, self.engine.as_ref(), Instant::now());
    }

    /// Returns a read-only snapshot of stored execution feedback for a tab.
    ///
    /// The snapshot may be stale or incomplete and must not drive policy.
    /// `None` indicates feedback has not been sampled for the tab yet.
    #[cfg(feature = "diagnostics")]
    pub fn get_execution_feedback(
        &self,
        tab: TabId,
    ) -> Option<ExecutionFeedbackSnapshot<'_>> {
        let state = self.feedback.borrow();
        if !state.per_tab.contains_key(&tab) {
            return None;
        }
        Some(ExecutionFeedbackSnapshot { state, tab })
    }

    /// Returns an aggregated, read-only snapshot of all stored feedback.
    ///
    /// This data is observational only and must not influence scheduling policy.
    #[cfg(feature = "diagnostics")]
    pub fn execution_feedback_snapshot(&self) -> ExecutionFeedbackAggregate<'_> {
        ExecutionFeedbackAggregate {
            state: self.feedback.borrow(),
        }
    }

    /// Polls the governor to refresh idle/burst state.
    pub fn poll(&self) {
        self.reconcile(Instant::now());
    }

    fn mark_recent_input(&self, tab: TabId, now: Instant) {
        self.last_global_input.set(now);
        self.last_idle_burst.set(now);
        self.last_tab_input.borrow_mut().insert(tab, now);
    }

    fn reconcile(&self, now: Instant) {
        const ACTIVE_INPUT_WINDOW: Duration = Duration::from_millis(1200);
        const IDLE_THRESHOLD: Duration = Duration::from_secs(4);
        const IDLE_BURST_INTERVAL: Duration = Duration::from_secs(5);
        const IDLE_BURST_DURATION: Duration = Duration::from_millis(500);
        const TAB_INPUT_GRACE: Duration = Duration::from_millis(800);

        // Intent is separate from tab lifecycle: tab state is owned by the tab manager,
        // while intent reflects recent user interaction and can further gate background JS.
        let since_input = now.duration_since(self.last_global_input.get());
        let user_active = since_input <= ACTIVE_INPUT_WINDOW;
        let user_idle = since_input >= IDLE_THRESHOLD;

        // When idle, allow short background bursts at a fixed interval.
        let allow_idle_burst = if user_idle {
            let since_burst = now.duration_since(self.last_idle_burst.get());
            if since_burst >= IDLE_BURST_INTERVAL {
                self.last_idle_burst.set(now);
                true
            } else {
                since_burst <= IDLE_BURST_DURATION
            }
        } else {
            false
        };

        let states_snapshot: Vec<(TabId, TabState)> =
            self.states.borrow().iter().map(|(id, state)| (*id, *state)).collect();
        let last_tab_input = self.last_tab_input.borrow();
        let mut effective_states = self.effective_states.borrow_mut();

        let pressure = self.memory_pressure.get();

        for (tab, base_state) in states_snapshot {
            // Short grace window for tabs that were just interacted with.
            let tab_recent = last_tab_input
                .get(&tab)
                .map(|ts| now.duration_since(*ts) <= TAB_INPUT_GRACE)
                .unwrap_or(false);

            let (mut effective, mut budget) = match base_state {
                TabState::Active => (
                    TabState::Active,
                    ExecutionBudget {
                        tier: BudgetTier::Foreground,
                    },
                ),
                TabState::Suspended => (
                    TabState::Suspended,
                    ExecutionBudget {
                        tier: BudgetTier::IdleBackground,
                    },
                ),
                TabState::Background => {
                    // Intent influences the budget tier, which in turn gates execution.
                    let tier = if user_active {
                        BudgetTier::VisibleBackground
                    } else if user_idle {
                        BudgetTier::IdleBackground
                    } else {
                        BudgetTier::VisibleBackground
                    };

                    let allow = if user_active {
                        // Defer non-critical background JS while the user is active.
                        false
                    } else if user_idle {
                        allow_idle_burst
                    } else {
                        true
                    };

                    let state = if allow || tab_recent {
                        TabState::Background
                    } else {
                        TabState::Suspended
                    };

                    (state, ExecutionBudget { tier })
                }
            };

            // Memory pressure only demotes budgets; it never promotes.
            // Foreground tabs stay protected unless pressure is severe.
            budget.tier = match (pressure, budget.tier) {
                (MemoryPressure::Low, tier) => tier,
                (MemoryPressure::Moderate, BudgetTier::Foreground) => BudgetTier::Foreground,
                (MemoryPressure::Moderate, BudgetTier::VisibleBackground) => BudgetTier::IdleBackground,
                (MemoryPressure::Moderate, BudgetTier::IdleBackground) => BudgetTier::IdleBackground,
                (MemoryPressure::Severe, BudgetTier::Foreground) => BudgetTier::VisibleBackground,
                (MemoryPressure::Severe, BudgetTier::VisibleBackground) => BudgetTier::IdleBackground,
                (MemoryPressure::Severe, BudgetTier::IdleBackground) => BudgetTier::IdleBackground,
            };

            // Budget tiers further gate effective state for background tabs.
            if base_state == TabState::Background {
                if budget.tier == BudgetTier::IdleBackground && !(user_idle && allow_idle_burst) && !tab_recent {
                    effective = TabState::Suspended;
                }
            }

            let budget_changed = self.apply_budget(tab, budget);
            let hints = map_execution_hints(budget, pressure);
            self.apply_hints(tab, hints);

            let state_changed = if let Some(previous) = effective_states.get(&tab) {
                *previous != effective
            } else {
                true
            };

            if state_changed {
                self.engine.apply_tab_state(tab, effective);
                effective_states.insert(tab, effective);
            }

            self.maybe_poll_feedback(tab, state_changed, budget_changed);
        }
    }

    fn apply_budget(&self, tab: TabId, budget: ExecutionBudget) -> bool {
        let mut budgets = self.budgets.borrow_mut();
        if let Some(previous) = budgets.get(&tab) {
            if *previous == budget {
                return false;
            }
        }
        budgets.insert(tab, budget);
        self.engine.apply_execution_budget(tab, budget);
        true
    }

    fn apply_hints(&self, tab: TabId, hints: ExecutionBudgetHints) {
        let mut hints_map = self.hints.borrow_mut();
        if let Some(previous) = hints_map.get(&tab) {
            if *previous == hints {
                return;
            }
        }
        hints_map.insert(tab, hints);
        self.engine.apply_execution_hints(tab, hints);
    }

    #[cfg(feature = "diagnostics")]
    fn maybe_poll_feedback(
        &self,
        tab: TabId,
        state_changed: bool,
        budget_changed: bool,
    ) {
        let should_sample = (state_changed
            && self
                .feedback_trigger
                .should_sample(FeedbackSamplingEvent::TabStateChange))
            || (budget_changed
                && self
                    .feedback_trigger
                    .should_sample(FeedbackSamplingEvent::BudgetTierChange));

        if should_sample {
            self.poll_execution_feedback(tab);
        }
    }

    #[cfg(not(feature = "diagnostics"))]
    fn maybe_poll_feedback(
        &self,
        _tab: TabId,
        _state_changed: bool,
        _budget_changed: bool,
    ) {
    }
}

impl JSExecutionGovernor for ExecutionGovernor {
    fn set_budget(&self, tab: TabId, budget: ExecutionBudget) {
        let budget_changed = self.apply_budget(tab, budget);
        let hints = map_execution_hints(budget, self.memory_pressure.get());
        self.apply_hints(tab, hints);
        self.maybe_poll_feedback(tab, false, budget_changed);
    }

    fn on_tab_state_changed(&self, tab: TabId, state: TabState) {
        self.states.borrow_mut().insert(tab, state);
        if state == TabState::Active {
            // Treat focus changes as intent signals.
            self.mark_recent_input(tab, Instant::now());
        }
        self.reconcile(Instant::now());
    }
}

#[cfg(all(test, feature = "diagnostics"))]
mod diagnostics_contract_tests {
    use super::*;

    /// Diagnostics are observational only and must not be referenced by
    /// scheduling or budget decision code. These tests validate that
    /// diagnostics helpers are read-only and deterministic.
    #[derive(Debug)]
    struct DummyEngine {
        feedback: EngineExecutionFeedback,
    }

    impl DummyEngine {
        fn new() -> Self {
            Self {
                feedback: EngineExecutionFeedback {
                    has_long_tasks: true,
                    worker_count: 2,
                    wasm_active: true,
                    js_blocking_render: false,
                },
            }
        }
    }

    impl EngineFeedbackProvider for DummyEngine {
        fn poll_execution_feedback(&self, _tab: TabId) -> EngineExecutionFeedback {
            self.feedback
        }
    }

    impl EngineScheduler for DummyEngine {
        fn apply_tab_state(&self, _tab: TabId, _state: TabState) {}

        fn apply_execution_budget(&self, _tab: TabId, _budget: ExecutionBudget) {}

        fn apply_execution_hints(&self, _tab: TabId, _hints: ExecutionBudgetHints) {}
    }

    #[test]
    fn debug_line_is_read_only_and_stable() {
        let engine = Rc::new(DummyEngine::new());
        let governor = ExecutionGovernor::new(engine);
        let tab = TabId::new(1);

        governor.poll_execution_feedback(tab);
        let snapshot = governor.get_execution_feedback(tab).expect("missing feedback");

        let before = snapshot.sample_count();
        let line1 = snapshot.debug_line(true, None).to_string();
        let line2 = snapshot.debug_line(true, None).to_string();
        let after = snapshot.sample_count();

        assert_eq!(before, after);
        assert_eq!(line1, line2);
        assert!(line1.contains("tab=1"));
        assert!(line1.contains("staleness="));
    }

    #[test]
    fn aggregate_report_is_read_only() {
        let engine = Rc::new(DummyEngine::new());
        let governor = ExecutionGovernor::new(engine);
        let tab1 = TabId::new(1);
        let tab2 = TabId::new(2);

        governor.poll_execution_feedback(tab1);
        governor.poll_execution_feedback(tab2);

        let windows = FeedbackAgingWindows {
            recent: Duration::from_secs(3600),
            expired: Duration::from_secs(7200),
        };

        {
            let aggregate = governor.execution_feedback_snapshot();
            let report = aggregate.debug_report(windows, true).to_string();

            assert!(report.contains("sampled_tabs=2"));
            assert!(report.contains("long_tasks=2"));
            assert!(report.contains("age{recent=2 aging=0 expired=0}"));
            assert!(report.contains("max_age_ms="));
        }

        let snapshot = governor.get_execution_feedback(tab1).expect("missing feedback");
        assert_eq!(snapshot.sample_count(), 1);
    }
}
