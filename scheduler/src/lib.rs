use std::cell::{Cell, RefCell};
use std::collections::HashMap;
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
pub trait EngineScheduler {
    /// Applies a tab state transition at the engine level.
    fn apply_tab_state(&self, tab: TabId, state: TabState);

    /// Applies a budget to the engine for the given tab.
    fn apply_execution_budget(&self, tab: TabId, budget: ExecutionBudget);

    /// Applies advisory execution hints for the given tab.
    ///
    /// These are intent signals only; the engine may ignore them to preserve compatibility.
    fn apply_execution_hints(&self, tab: TabId, hints: ExecutionBudgetHints);
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

            self.apply_budget(tab, budget);
            let hints = map_execution_hints(budget, pressure);
            self.apply_hints(tab, hints);

            if let Some(previous) = effective_states.get(&tab) {
                if *previous != effective {
                    self.engine.apply_tab_state(tab, effective);
                    effective_states.insert(tab, effective);
                }
            } else {
                self.engine.apply_tab_state(tab, effective);
                effective_states.insert(tab, effective);
            }
        }
    }

    fn apply_budget(&self, tab: TabId, budget: ExecutionBudget) {
        let mut budgets = self.budgets.borrow_mut();
        if let Some(previous) = budgets.get(&tab) {
            if *previous == budget {
                return;
            }
        }
        budgets.insert(tab, budget);
        self.engine.apply_execution_budget(tab, budget);
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
}

impl JSExecutionGovernor for ExecutionGovernor {
    fn set_budget(&self, tab: TabId, budget: ExecutionBudget) {
        self.apply_budget(tab, budget);
        let hints = map_execution_hints(budget, self.memory_pressure.get());
        self.apply_hints(tab, hints);
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
