use std::cell::RefCell;
use std::collections::HashMap;

use gtk::prelude::*;
use scheduler::{EngineScheduler, ExecutionBudget, ExecutionBudgetHints};
use tabs::{TabId, TabState};
use webkit6::prelude::*;

/// Interface to the web engine implementation.
pub trait EngineController {
    type View;

    /// Creates a new view instance for a tab.
    fn create_view(&self) -> Self::View;

    /// Loads a URI into the provided view.
    fn load_uri(&self, view: &Self::View, uri: &str);

    /// Registers a view with a tab id for scheduler control.
    fn register_view(&self, tab: TabId, view: &Self::View);

    /// Unregisters a view when a tab is destroyed.
    fn unregister_view(&self, tab: TabId);

    /// Applies a tab state transition at the engine level.
    fn apply_tab_state(&self, tab: TabId, state: TabState);

    /// Applies an execution budget to the tab.
    fn apply_execution_budget(&self, tab: TabId, budget: ExecutionBudget);

    /// Applies advisory execution hints for the tab.
    fn apply_execution_hints(&self, tab: TabId, hints: ExecutionBudgetHints);
}

/// WebKitGTK-backed engine controller.
#[derive(Debug, Default)]
pub struct WebKitEngine {
    views: RefCell<HashMap<TabId, webkit6::WebView>>,
}

impl WebKitEngine {
    pub fn new() -> Self {
        Self::default()
    }

    fn settings() -> webkit6::Settings {
        webkit6::Settings::builder()
            .enable_javascript(true)
            .build()
    }

    fn with_view<F: FnOnce(&webkit6::WebView)>(&self, tab: TabId, f: F) {
        if let Some(view) = self.views.borrow().get(&tab) {
            f(view);
        }
    }

    fn set_javascript_enabled(view: &webkit6::WebView, enabled: bool) {
        if let Some(settings) = webkit6::prelude::WebViewExt::settings(view) {
            settings.set_enable_javascript(enabled);
        }
    }
}

impl EngineController for WebKitEngine {
    type View = webkit6::WebView;

    fn create_view(&self) -> Self::View {
        let settings = Self::settings();
        webkit6::WebView::builder().settings(&settings).build()
    }

    fn load_uri(&self, view: &Self::View, uri: &str) {
        view.load_uri(uri);
    }

    fn register_view(&self, tab: TabId, view: &Self::View) {
        self.views.borrow_mut().insert(tab, view.clone());
    }

    fn unregister_view(&self, tab: TabId) {
        self.views.borrow_mut().remove(&tab);
    }

    fn apply_tab_state(&self, tab: TabId, state: TabState) {
        self.with_view(tab, |view| match state {
            TabState::Active => {
                view.set_visible(true);
                Self::set_javascript_enabled(view, true);
            }
            TabState::Background => {
                // WebKitGTK does not expose explicit timer-clamp controls. We rely on
                // widget visibility to trigger Page Visibility throttling in the engine.
                view.set_visible(false);
                Self::set_javascript_enabled(view, true);
            }
            TabState::Suspended => {
                // WebKitGTK does not currently expose a true pause/resume API for JS.
                // Disabling JavaScript is the closest safe approximation for suspension.
                view.set_visible(false);
                Self::set_javascript_enabled(view, false);
            }
        });
    }

    fn apply_execution_budget(&self, tab: TabId, _budget: ExecutionBudget) {
        // TODO: Apply per-tab CPU and scheduling budgets when cgroup integration lands.
        self.with_view(tab, |_| {});
    }

    fn apply_execution_hints(&self, tab: TabId, _hints: ExecutionBudgetHints) {
        // TODO: Map advisory hints to WebKit settings once supported.
        self.with_view(tab, |_| {});
    }
}

impl EngineScheduler for WebKitEngine {
    fn apply_tab_state(&self, tab: TabId, state: TabState) {
        <Self as EngineController>::apply_tab_state(self, tab, state);
    }

    fn apply_execution_budget(&self, tab: TabId, budget: ExecutionBudget) {
        <Self as EngineController>::apply_execution_budget(self, tab, budget);
    }

    fn apply_execution_hints(&self, tab: TabId, hints: ExecutionBudgetHints) {
        <Self as EngineController>::apply_execution_hints(self, tab, hints);
    }
}
