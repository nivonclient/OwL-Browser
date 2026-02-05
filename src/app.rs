use crate::assets::Assets;
use crate::ipc::{self, IncomingMessage, NavState};
use crate::state::BrowserState;
use adw::prelude::*;
use gtk::glib;
use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;
use std::time::{Duration, Instant};
use webkit6::prelude::*;

const APP_ID: &str = "com.owl.browser";
const APP_TITLE: &str = "OwL Browser";
const SIDEBAR_EXPANDED: i32 = 300;
const SIDEBAR_COLLAPSED: i32 = 60;
const SIDEBAR_COLLAPSE_THRESHOLD: i32 = 2;
const SIDEBAR_RESIZE_IDLE_MS: u64 = 120;

#[derive(Debug)]
struct UiState {
    sidebar_collapsed: bool,
    sidebar_animation: Option<glib::SourceId>,
    sidebar_expanded: i32,
    sidebar_resize_idle: Option<glib::SourceId>,
}

pub fn run() -> glib::ExitCode {
    let app = adw::Application::builder().application_id(APP_ID).build();
    app.connect_activate(build_ui);
    app.run()
}

fn build_ui(app: &adw::Application) {
    let style_manager = adw::StyleManager::default();
    style_manager.set_color_scheme(adw::ColorScheme::Default);

    let assets = Assets::new();
    let default_favicon = assets.default_favicon_uri.clone();
    let state = Rc::new(RefCell::new(BrowserState::new()));
    let ui_state = Rc::new(RefCell::new(UiState {
        sidebar_collapsed: false,
        sidebar_animation: None,
        sidebar_expanded: SIDEBAR_EXPANDED,
        sidebar_resize_idle: None,
    }));

    let ui_manager = webkit6::UserContentManager::new();
    if !ui_manager.register_script_message_handler("owl", None) {
        eprintln!("Failed to register script message handler");
    }

    let ui_webview = create_webview(Some(&ui_manager));
    let content_webview = create_webview(None);
    let favicon_db = content_webview
        .network_session()
        .and_then(|session| session.website_data_manager())
        .and_then(|manager| {
            manager.set_favicons_enabled(true);
            manager.favicon_database()
        });

    let paned = gtk::Paned::new(gtk::Orientation::Horizontal);
    paned.set_start_child(Some(&ui_webview));
    paned.set_end_child(Some(&content_webview));
    paned.set_position(SIDEBAR_EXPANDED);

    let header = build_header_bar(&assets);
    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    content.append(&header);
    content.append(&paned);

    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title(APP_TITLE)
        .default_width(1280)
        .default_height(800)
        .content(&content)
        .build();

    if let Some(display) = gtk::gdk::Display::default() {
        if let Some(icon_name) = assets.register_icon(&display) {
            window.set_icon_name(Some(&icon_name));
        }
    }

    window.present();

    ui_webview.load_uri(&assets.ui_uri);
    load_home(&content_webview, &assets.home_uri);

    let state_for_ui = Rc::clone(&state);
    let content_for_ui = content_webview.clone();
    let loading_for_ui = Rc::new(RefCell::new(false));
    let loading_for_ui_cb = Rc::clone(&loading_for_ui);
    ui_webview.connect_load_changed(move |view, event| {
        if event == webkit6::LoadEvent::Finished {
            ipc::send_state(view, &state_for_ui.borrow());
            emit_nav_state(view, &content_for_ui, *loading_for_ui_cb.borrow());
        }
    });

    let ui_webview_for_content = ui_webview.clone();
    let state_for_content = Rc::clone(&state);
    let home_uri_for_content = assets.home_uri.clone();
    let loading_for_content = Rc::clone(&loading_for_ui);
    let favicon_db_for_content = favicon_db.clone();
    content_webview.connect_load_changed(move |view, event| {
        let is_loading = matches!(
            event,
            webkit6::LoadEvent::Started
                | webkit6::LoadEvent::Redirected
                | webkit6::LoadEvent::Committed
        );
        {
            let mut loading = loading_for_content.borrow_mut();
            *loading = is_loading;
        }
        emit_nav_state(&ui_webview_for_content, view, is_loading);

        if event == webkit6::LoadEvent::Finished {
            let title = view
                .title()
                .map(|t| t.to_string())
                .unwrap_or_else(|| "New Tab".to_string());
            let uri = view
                .uri()
                .map(|u| u.to_string())
                .unwrap_or_else(|| "owl://home".to_string());
            let display_uri = if uri == home_uri_for_content {
                "owl://home".to_string()
            } else {
                uri
            };

            let active = { state_for_content.borrow().active };
            if let Some(active) = active {
                {
                    let mut state_mut = state_for_content.borrow_mut();
                    state_mut.update_tab(active, Some(&title), Some(&display_uri));
                }
                let state_ref = state_for_content.borrow();
                ipc::send_state(&ui_webview_for_content, &state_ref);
            }

            if let Some(db) = &favicon_db_for_content {
                if let Some(actual_uri) = view.uri().map(|u| u.to_string()) {
                    refresh_favicon(db, &state_for_content, &ui_webview_for_content, &actual_uri);
                }
            }
        }
    });

    if let Some(db) = &favicon_db {
        let state_for_favicon = Rc::clone(&state);
        let ui_for_favicon = ui_webview.clone();
        db.connect_favicon_changed(move |_, page_uri, favicon_uri| {
            update_favicon_state(
                &state_for_favicon,
                &ui_for_favicon,
                page_uri,
                Some(favicon_uri.to_string()),
            );
        });
    }

    let ui_webview_for_policy = ui_webview.clone();
    let state_for_policy = Rc::clone(&state);
    let home_uri_for_policy = assets.home_uri.clone();
    content_webview.connect_decide_policy(move |view, decision, decision_type| {
        if decision_type != webkit6::PolicyDecisionType::NavigationAction {
            return false;
        }

        let Some(policy) = decision.dynamic_cast_ref::<webkit6::NavigationPolicyDecision>() else {
            return false;
        };
        let Some(mut action) = policy.navigation_action() else {
            return false;
        };
        let Some(request) = action.request() else {
            return false;
        };
        let Some(uri) = request.uri() else {
            return false;
        };
        let uri = uri.to_string();

        if uri == "owl://home" || uri == "about:home" {
            decision.ignore();
            load_home(view, &home_uri_for_policy);
            return true;
        }

        if let Some(slug) = uri.strip_prefix("owl://session/") {
            decision.ignore();
            if let Some(first_url) = open_session(&state_for_policy, slug) {
                let state_ref = state_for_policy.borrow();
                ipc::send_state(&ui_webview_for_policy, &state_ref);
                load_url(view, &first_url, &home_uri_for_policy);
            } else {
                load_home(view, &home_uri_for_policy);
            }
            return true;
        }

        false
    });

    let ui_webview_for_failure = ui_webview.clone();
    let loading_for_failure = Rc::clone(&loading_for_ui);
    content_webview.connect_load_failed(move |view, _event, _uri, _error| {
        {
            let mut loading = loading_for_failure.borrow_mut();
            *loading = false;
        }
        emit_nav_state(&ui_webview_for_failure, view, false);
        false
    });

    let ui_webview_for_messages = ui_webview.clone();
    let content_webview_for_messages = content_webview.clone();
    let state_for_messages = Rc::clone(&state);
    let home_uri_for_messages = assets.home_uri.clone();
    let paned_for_messages = paned.clone();
    let ui_state_for_messages = Rc::clone(&ui_state);
    let default_favicon_for_messages = default_favicon.clone();

    ui_manager.connect_script_message_received(Some("owl"), move |_, value| {
        let raw = value.to_str();
        let Ok(message) = serde_json::from_str::<IncomingMessage>(&raw) else {
            eprintln!("Failed to parse message: {raw}");
            return;
        };

        handle_message(
            message,
            &ui_webview_for_messages,
            &content_webview_for_messages,
            &state_for_messages,
            &home_uri_for_messages,
            &paned_for_messages,
            &ui_state_for_messages,
            &default_favicon_for_messages,
            &favicon_db,
        );
    });

    let ui_webview_for_resize = ui_webview.clone();
    let ui_state_for_resize = Rc::clone(&ui_state);
    let paned_for_resize = paned.clone();
    paned.connect_notify_local(Some("position"), move |paned, _| {
        let mut state = ui_state_for_resize.borrow_mut();

        if state.sidebar_animation.is_some() {
            return;
        }

        let position = paned.position();

        if state.sidebar_collapsed {
            if position != SIDEBAR_COLLAPSED {
                paned.set_position(SIDEBAR_COLLAPSED);
            }
            return;
        }

        if position > SIDEBAR_COLLAPSED + SIDEBAR_COLLAPSE_THRESHOLD {
            state.sidebar_expanded = position;
        }

        if let Some(idle) = state.sidebar_resize_idle.take() {
            idle.remove();
        }

        let paned = paned_for_resize.clone();
        let ui_state = Rc::clone(&ui_state_for_resize);
        let ui_view = ui_webview_for_resize.clone();
        let source = glib::timeout_add_local(Duration::from_millis(SIDEBAR_RESIZE_IDLE_MS), move || {
            let position = paned.position();
            let should_collapse =
                position <= SIDEBAR_COLLAPSED + SIDEBAR_COLLAPSE_THRESHOLD;

            if should_collapse {
                {
                    let mut state = ui_state.borrow_mut();
                    state.sidebar_collapsed = true;
                    if let Some(anim) = state.sidebar_animation.take() {
                        anim.remove();
                    }
                }
                paned.set_position(SIDEBAR_COLLAPSED);
                ipc::send_sidebar_state(&ui_view, true);
            }

            ui_state.borrow_mut().sidebar_resize_idle = None;
            glib::ControlFlow::Break
        });

        state.sidebar_resize_idle = Some(source);
    });
}

fn build_header_bar(assets: &Assets) -> adw::HeaderBar {
    let header = adw::HeaderBar::new();
    header.set_show_start_title_buttons(true);
    header.set_show_end_title_buttons(true);
    header.set_decoration_layout(Some(":minimize,maximize,close"));

    let title_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    if assets.icon_path.exists() {
        let icon = gtk::Image::from_file(&assets.icon_path);
        icon.set_pixel_size(18);
        title_box.append(&icon);
    }
    let title = gtk::Label::new(Some(APP_TITLE));
    title_box.append(&title);
    header.set_title_widget(Some(&title_box));

    header
}

fn create_webview(manager: Option<&webkit6::UserContentManager>) -> webkit6::WebView {
    let settings = webkit6::Settings::builder()
        .allow_file_access_from_file_urls(true)
        .enable_javascript(true)
        .build();

    let mut builder = webkit6::WebView::builder().settings(&settings);
    if let Some(manager) = manager {
        builder = builder.user_content_manager(manager);
    }

    let webview = builder.build();
    webview.set_hexpand(true);
    webview.set_vexpand(true);
    webview
}

fn handle_message(
    message: IncomingMessage,
    ui_webview: &webkit6::WebView,
    content_webview: &webkit6::WebView,
    state: &Rc<RefCell<BrowserState>>,
    home_uri: &str,
    paned: &gtk::Paned,
    ui_state: &Rc<RefCell<UiState>>,
    default_favicon: &str,
    favicon_db: &Option<webkit6::FaviconDatabase>,
) {
    match message.r#type.as_str() {
        "ui.ready" => {
            ipc::send_assets(ui_webview, default_favicon);
            ipc::send_state(ui_webview, &state.borrow());
            ipc::send_sidebar_state(ui_webview, ui_state.borrow().sidebar_collapsed);
            emit_nav_state(ui_webview, content_webview, false);
            if let Some(db) = favicon_db {
                prefetch_all_favicons(db, state, ui_webview);
            }
        }
        "tab.select" => {
            if let Some(id) = message.payload.get("id").and_then(|v| v.as_u64()) {
                let url = { state.borrow().tabs.get(&id).map(|t| t.url.clone()) };
                if let Some(url) = url {
                    if let Some(node) = state.borrow_mut().tabs.get_mut(&id) {
                        node.is_suspended = false;
                    }
                    state.borrow_mut().set_active(id);
                    load_url(content_webview, &url, home_uri);
                    let state_ref = state.borrow();
                    ipc::send_state(ui_webview, &state_ref);
                }
            }
        }
        "tab.toggle" => {
            if let Some(id) = message.payload.get("id").and_then(|v| v.as_u64()) {
                state.borrow_mut().toggle_expanded(id);
                let state_ref = state.borrow();
                ipc::send_state(ui_webview, &state_ref);
            }
        }
        "tab.pin" => {
            if let Some(id) = message.payload.get("id").and_then(|v| v.as_u64()) {
                state.borrow_mut().toggle_pin(id);
                let state_ref = state.borrow();
                ipc::send_state(ui_webview, &state_ref);
            }
        }
        "tab.mute" => {
            if let Some(id) = message.payload.get("id").and_then(|v| v.as_u64()) {
                state.borrow_mut().toggle_mute(id);
                let state_ref = state.borrow();
                ipc::send_state(ui_webview, &state_ref);
            }
        }
        "tab.unload" => {
            if let Some(id) = message.payload.get("id").and_then(|v| v.as_u64()) {
                state.borrow_mut().toggle_suspended(id);
                let state_ref = state.borrow();
                ipc::send_state(ui_webview, &state_ref);
            }
        }
        "tab.create" => {
            let id = state
                .borrow_mut()
                .create_tab(None, "New Tab", "owl://home");
            state.borrow_mut().set_active(id);
            load_home(content_webview, home_uri);
            let state_ref = state.borrow();
            ipc::send_state(ui_webview, &state_ref);
        }
        "tab.close" => {
            if let Some(id) = message.payload.get("id").and_then(|v| v.as_u64()) {
                state.borrow_mut().remove_tab(id);
                let active = { state.borrow().active };
                if let Some(active) = active {
                    let url = { state.borrow().tabs.get(&active).map(|t| t.url.clone()) };
                    if let Some(url) = url {
                        load_url(content_webview, &url, home_uri);
                    }
                } else {
                    load_home(content_webview, home_uri);
                }
                let state_ref = state.borrow();
                ipc::send_state(ui_webview, &state_ref);
            }
        }
        "nav.go" => {
            if let Some(url) = message.payload.get("url").and_then(|v| v.as_str()) {
                let normalized = normalize_url(url);
                if let Some(slug) = normalized.strip_prefix("owl://session/") {
                if let Some(first_url) = open_session(state, slug) {
                    let state_ref = state.borrow();
                    ipc::send_state(ui_webview, &state_ref);
                    if let Some(db) = favicon_db {
                        prefetch_all_favicons(db, state, ui_webview);
                    }
                    load_url(content_webview, &first_url, home_uri);
                } else {
                    load_home(content_webview, home_uri);
                }
                    return;
                }
                let active = { state.borrow().active };
                if let Some(active) = active {
                    state
                        .borrow_mut()
                        .update_tab(active, None, Some(&normalized));
                }
                load_url(content_webview, &normalized, home_uri);
                let state_ref = state.borrow();
                ipc::send_state(ui_webview, &state_ref);
            }
        }
        "nav.back" => {
            content_webview.go_back();
        }
        "nav.forward" => {
            content_webview.go_forward();
        }
        "nav.reload" => {
            content_webview.reload();
        }
        "nav.stop" => {
            content_webview.stop_loading();
        }
        "ui.sidebar.toggle" => {
            if let Some(collapsed) = message.payload.get("collapsed").and_then(|v| v.as_bool()) {
                animate_sidebar(paned, ui_state, collapsed);
            }
        }
        "nav.home" => {
            load_home(content_webview, home_uri);
        }
        _ => {}
    }
}

fn load_home(webview: &webkit6::WebView, home_uri: &str) {
    webview.load_uri(home_uri);
}

fn load_url(webview: &webkit6::WebView, url: &str, home_uri: &str) {
    if url.starts_with("owl://") || url == "about:home" {
        load_home(webview, home_uri);
    } else {
        webview.load_uri(url);
    }
}

fn normalize_url(input: &str) -> String {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return "owl://home".to_string();
    }

    if trimmed.contains("://") || trimmed.starts_with("about:") || trimmed.starts_with("owl://") {
        trimmed.to_string()
    } else {
        format!("https://{trimmed}")
    }
}

fn open_session(state: &Rc<RefCell<BrowserState>>, slug: &str) -> Option<String> {
    let (group_title, tabs) = session_template(slug)?;
    let mut state_mut = state.borrow_mut();
    let group_id = state_mut.create_group(group_title);
    let mut first_url: Option<String> = None;
    let mut first_id: Option<u64> = None;

    for (title, url) in tabs {
        let id = state_mut.create_tab(Some(group_id), title, url);
        if first_url.is_none() {
            first_url = Some(url.to_string());
            first_id = Some(id);
        }
    }

    if let Some(id) = first_id {
        state_mut.set_active(id);
    }

    first_url
}

fn session_template(slug: &str) -> Option<(&'static str, &'static [(&'static str, &'static str)])> {
    match slug {
        "research-notes" => Some((
            "Research Notes",
            &[
                ("WebKitGTK", "https://webkitgtk.org"),
                ("Rust Book", "https://doc.rust-lang.org/book/"),
                ("Fedora Docs", "https://docs.fedoraproject.org"),
            ],
        )),
        "release-planning" => Some((
            "Release Planning",
            &[
                ("GNOME Release", "https://release.gnome.org"),
                ("Fedora Schedule", "https://fedorapeople.org/groups/schedule/"),
                ("Issue Tracker", "https://gitlab.gnome.org"),
            ],
        )),
        "wayland-checklist" => Some((
            "Wayland Checklist",
            &[
                ("Wayland", "https://wayland.freedesktop.org"),
                ("GTK4", "https://www.gtk.org"),
                ("libadwaita", "https://gnome.pages.gitlab.gnome.org/libadwaita/"),
            ],
        )),
        _ => None,
    }
}

fn emit_nav_state(
    ui_webview: &webkit6::WebView,
    content_webview: &webkit6::WebView,
    is_loading: bool,
) {
    let nav = NavState {
        can_go_back: content_webview.can_go_back(),
        can_go_forward: content_webview.can_go_forward(),
        is_loading,
    };
    ipc::send_nav_state(ui_webview, nav);
}

fn refresh_favicon(
    favicon_db: &webkit6::FaviconDatabase,
    state: &Rc<RefCell<BrowserState>>,
    ui_webview: &webkit6::WebView,
    page_uri: &str,
) {
    queue_favicon_fetch(favicon_db, state, ui_webview, page_uri);
}

fn prefetch_all_favicons(
    favicon_db: &webkit6::FaviconDatabase,
    state: &Rc<RefCell<BrowserState>>,
    ui_webview: &webkit6::WebView,
) {
    let urls: Vec<String> = {
        let mut seen = HashSet::new();
        let state_ref = state.borrow();
        state_ref
            .tabs
            .values()
            .filter_map(|node| {
                if node.favicon_uri.is_some() {
                    return None;
                }
                let url = node.url.clone();
                if !url.starts_with("http://") && !url.starts_with("https://") {
                    return None;
                }
                if seen.insert(url.clone()) {
                    Some(url)
                } else {
                    None
                }
            })
            .collect()
    };

    for url in urls {
        queue_favicon_fetch(favicon_db, state, ui_webview, &url);
    }
}

fn queue_favicon_fetch(
    favicon_db: &webkit6::FaviconDatabase,
    state: &Rc<RefCell<BrowserState>>,
    ui_webview: &webkit6::WebView,
    page_uri: &str,
) {
    if !page_uri.starts_with("http://") && !page_uri.starts_with("https://") {
        return;
    }

    if let Some(favicon_uri) = favicon_db.favicon_uri(page_uri) {
        update_favicon_state(
            state,
            ui_webview,
            page_uri,
            Some(favicon_uri.to_string()),
        );
        return;
    }

    let page_uri = page_uri.to_string();
    let request_uri = page_uri.clone();
    let state = Rc::clone(state);
    let ui_webview = ui_webview.clone();
    let db_for_cb = favicon_db.clone();

    favicon_db.favicon(&request_uri, None::<&gtk::gio::Cancellable>, move |result| {
        if result.is_ok() {
            if let Some(favicon_uri) = db_for_cb.favicon_uri(&page_uri) {
                update_favicon_state(
                    &state,
                    &ui_webview,
                    &page_uri,
                    Some(favicon_uri.to_string()),
                );
            }
        }
    });
}

fn update_favicon_state(
    state: &Rc<RefCell<BrowserState>>,
    ui_webview: &webkit6::WebView,
    page_uri: &str,
    favicon_uri: Option<String>,
) {
    let updated = {
        let mut state_mut = state.borrow_mut();
        state_mut.set_favicon_for_url(page_uri, favicon_uri.clone())
    };

    if !updated.is_empty() {
        ipc::send_favicon(ui_webview, updated, favicon_uri);
    }
}

fn animate_sidebar(paned: &gtk::Paned, ui_state: &Rc<RefCell<UiState>>, collapsed: bool) {
    {
        let mut state = ui_state.borrow_mut();
        state.sidebar_collapsed = collapsed;
        if let Some(idle) = state.sidebar_resize_idle.take() {
            idle.remove();
        }
        if let Some(source) = state.sidebar_animation.take() {
            source.remove();
        }
    }

    let start = paned.position() as f64;
    let target = if collapsed {
        SIDEBAR_COLLAPSED as f64
    } else {
        ui_state.borrow().sidebar_expanded as f64
    };

    let duration = Duration::from_millis(140);
    let started = Instant::now();
    let paned = paned.clone();
    let ui_state_for_tick = Rc::clone(ui_state);

    let source = glib::timeout_add_local(Duration::from_millis(16), move || {
        let elapsed = started.elapsed();
        let t = (elapsed.as_secs_f64() / duration.as_secs_f64()).min(1.0);
        let eased = cubic_bezier(t, 0.2, 0.0, 0.2, 1.0);
        let value = start + (target - start) * eased;
        paned.set_position(value.round() as i32);

        if t >= 1.0 {
            ui_state_for_tick.borrow_mut().sidebar_animation = None;
            glib::ControlFlow::Break
        } else {
            glib::ControlFlow::Continue
        }
    });

    ui_state.borrow_mut().sidebar_animation = Some(source);
}

fn cubic_bezier(t: f64, _x1: f64, y1: f64, _x2: f64, y2: f64) -> f64 {
    let inv = 1.0 - t;
    let t2 = t * t;
    let inv2 = inv * inv;
    (3.0 * inv2 * t * y1) + (3.0 * inv * t2 * y2) + (t2 * t)
}
