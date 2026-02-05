use adw::prelude::*;
use gtk::glib;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use engine::{EngineController, WebKitEngine};
use scheduler::{ExecutionGovernor, JSExecutionGovernor};
use tabs::{BasicTabManager, TabId, TabManager};

const APP_ID: &str = "com.owl.browser";
const APP_TITLE: &str = "OwL Browser";
const DEFAULT_URI: &str = "https://example.com";

fn main() -> glib::ExitCode {
    let app = adw::Application::builder().application_id(APP_ID).build();
    app.connect_activate(build_ui);
    app.run()
}

fn build_ui(app: &adw::Application) {
    let style_manager = adw::StyleManager::default();
    style_manager.set_color_scheme(adw::ColorScheme::Default);

    let engine = Rc::new(WebKitEngine::new());
    let governor = Rc::new(ExecutionGovernor::new(Rc::clone(&engine)));

    let tab_manager = Rc::new(RefCell::new(BasicTabManager::new()));
    let views: Rc<RefCell<HashMap<TabId, webkit6::WebView>>> =
        Rc::new(RefCell::new(HashMap::new()));

    let stack = gtk::Stack::new();
    stack.set_hexpand(true);
    stack.set_vexpand(true);

    let header = adw::HeaderBar::new();
    header.set_show_start_title_buttons(true);
    header.set_show_end_title_buttons(true);

    let new_tab_button = gtk::Button::with_label("New Tab");
    let next_tab_button = gtk::Button::with_label("Next Tab");
    header.pack_start(&new_tab_button);
    header.pack_start(&next_tab_button);

    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.append(&header);
    root.append(&stack);

    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title(APP_TITLE)
        .default_width(1280)
        .default_height(800)
        .content(&root)
        .build();
    window.present();

    let governor_for_poll = Rc::clone(&governor);
    glib::timeout_add_local(Duration::from_millis(250), move || {
        governor_for_poll.poll();
        glib::ControlFlow::Continue
    });

    create_tab(
        &engine,
        &tab_manager,
        &governor,
        &stack,
        &views,
        DEFAULT_URI,
    );

    let engine_for_new = Rc::clone(&engine);
    let tabs_for_new = Rc::clone(&tab_manager);
    let governor_for_new = Rc::clone(&governor);
    let stack_for_new = stack.clone();
    let views_for_new = Rc::clone(&views);
    new_tab_button.connect_clicked(move |_| {
        create_tab(
            &engine_for_new,
            &tabs_for_new,
            &governor_for_new,
            &stack_for_new,
            &views_for_new,
            DEFAULT_URI,
        );
    });

    let tabs_for_next = Rc::clone(&tab_manager);
    let governor_for_next = Rc::clone(&governor);
    let stack_for_next = stack.clone();
    let views_for_next = Rc::clone(&views);
    next_tab_button.connect_clicked(move |_| {
        let next = tabs_for_next.borrow().next_tab();
        if let Some(next_id) = next {
            activate_tab(
                &tabs_for_next,
                &governor_for_next,
                &stack_for_next,
                &views_for_next,
                next_id,
            );
        }
    });
}

fn create_tab(
    engine: &WebKitEngine,
    manager: &Rc<RefCell<BasicTabManager>>,
    governor: &Rc<ExecutionGovernor>,
    stack: &gtk::Stack,
    views: &Rc<RefCell<HashMap<TabId, webkit6::WebView>>>,
    uri: &str,
) -> TabId {
    let entry = manager.borrow_mut().create_tab();
    let view = engine.create_view();
    view.set_hexpand(true);
    view.set_vexpand(true);

    engine.load_uri(&view, uri);
    engine.register_view(entry.id, &view);
    attach_user_intent_handlers(&view, entry.id, Rc::clone(governor));
    let name = entry.id.to_string();
    stack.add_named(&view, Some(&name));
    views.borrow_mut().insert(entry.id, view.clone());

    activate_tab(manager, governor, stack, views, entry.id);
    entry.id
}

fn activate_tab(
    manager: &Rc<RefCell<BasicTabManager>>,
    governor: &Rc<ExecutionGovernor>,
    stack: &gtk::Stack,
    views: &Rc<RefCell<HashMap<TabId, webkit6::WebView>>>,
    id: TabId,
) {
    manager.borrow_mut().set_active(id);

    if let Some(view) = views.borrow().get(&id) {
        stack.set_visible_child(view);
    }

    // TODO: When scheduling lands, apply per-tab budgets and throttling here.
    notify_governor(governor.as_ref(), &manager.borrow());
}

fn notify_governor(governor: &dyn JSExecutionGovernor, manager: &BasicTabManager) {
    for tab in manager.tabs() {
        governor.on_tab_state_changed(tab.id, tab.state);
    }
}

fn attach_user_intent_handlers(
    view: &webkit6::WebView,
    tab: TabId,
    governor: Rc<ExecutionGovernor>,
) {
    let key_controller = gtk::EventControllerKey::new();
    let governor_for_key = Rc::clone(&governor);
    key_controller.connect_key_pressed(move |_, _, _, _| {
        governor_for_key.record_user_input(tab);
        glib::Propagation::Proceed
    });
    view.add_controller(key_controller);

    let motion_controller = gtk::EventControllerMotion::new();
    let governor_for_motion = Rc::clone(&governor);
    motion_controller.connect_motion(move |_, _, _| {
        governor_for_motion.record_user_input(tab);
    });
    view.add_controller(motion_controller);

    let scroll_controller =
        gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::BOTH_AXES);
    let governor_for_scroll = Rc::clone(&governor);
    scroll_controller.connect_scroll(move |_, _, _| {
        governor_for_scroll.record_user_input(tab);
        glib::Propagation::Proceed
    });
    view.add_controller(scroll_controller);
}
