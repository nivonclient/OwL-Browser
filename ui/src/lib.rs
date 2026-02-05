use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Message exchanged over the UI IPC bridge.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiMessage {
    pub name: String,
    pub payload: Value,
}

/// Interface for the HTML/CSS UI bridge.
pub trait UiBridge {
    /// Sends a message to the UI WebView.
    fn send(&self, message: UiMessage);

    /// Registers a handler for messages from the UI WebView.
    fn set_handler(&mut self, handler: Box<dyn Fn(UiMessage) + 'static>);
}

/// No-op bridge used during early scaffolding.
#[derive(Default)]
pub struct NoopUiBridge;

impl UiBridge for NoopUiBridge {
    fn send(&self, _message: UiMessage) {
        // TODO: Serialize and deliver messages over the WebKitGTK bridge.
    }

    fn set_handler(&mut self, _handler: Box<dyn Fn(UiMessage) + 'static>) {
        // TODO: Store handler for UI -> core messages.
    }
}
