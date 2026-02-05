use crate::state::BrowserState;
use serde::{Deserialize, Serialize};
use serde_json::json;
use webkit6::prelude::*;

#[derive(Debug, Deserialize)]
pub struct IncomingMessage {
    pub r#type: String,
    pub payload: serde_json::Value,
}

#[derive(Debug, Serialize)]
pub struct OutgoingMessage<'a, T> {
    pub r#type: &'a str,
    pub payload: T,
}

#[derive(Debug, Serialize)]
pub struct NavState {
    pub can_go_back: bool,
    pub can_go_forward: bool,
    pub is_loading: bool,
}

#[derive(Debug, Serialize)]
pub struct AssetsState<'a> {
    pub default_favicon: &'a str,
}

#[derive(Debug, Serialize)]
pub struct SidebarState {
    pub collapsed: bool,
}

#[derive(Debug, Serialize)]
pub struct FaviconState {
    pub ids: Vec<u64>,
    pub favicon_uri: Option<String>,
}

pub fn send_state(view: &webkit6::WebView, state: &BrowserState) {
    let payload = json!({
        "tabs": state.to_ui_tree(),
        "active": state.active,
    });
    let message = OutgoingMessage {
        r#type: "state.tabs",
        payload,
    };
    send_to_ui(view, &message);
}

pub fn send_nav_state(view: &webkit6::WebView, nav: NavState) {
    let message = OutgoingMessage {
        r#type: "state.nav",
        payload: nav,
    };
    send_to_ui(view, &message);
}

pub fn send_assets(view: &webkit6::WebView, default_favicon: &str) {
    let payload = AssetsState { default_favicon };
    let message = OutgoingMessage {
        r#type: "state.assets",
        payload,
    };
    send_to_ui(view, &message);
}

pub fn send_sidebar_state(view: &webkit6::WebView, collapsed: bool) {
    let payload = SidebarState { collapsed };
    let message = OutgoingMessage {
        r#type: "state.sidebar",
        payload,
    };
    send_to_ui(view, &message);
}

pub fn send_favicon(view: &webkit6::WebView, ids: Vec<u64>, favicon_uri: Option<String>) {
    if ids.is_empty() {
        return;
    }
    let payload = FaviconState { ids, favicon_uri };
    let message = OutgoingMessage {
        r#type: "state.favicon",
        payload,
    };
    send_to_ui(view, &message);
}

fn send_to_ui<T: Serialize>(view: &webkit6::WebView, message: &OutgoingMessage<T>) {
    let Ok(json) = serde_json::to_string(message) else {
        return;
    };
    let script = format!("window.__owl_receive({json});");
    view.evaluate_javascript(&script, None, None, None::<&gtk::gio::Cancellable>, |_| {});
}
