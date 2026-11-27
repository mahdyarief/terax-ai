//! Remote access: watch and drive every terminal tab from a browser.
//!
//! Off until the user turns it on. Each paired device gets its own token and
//! grant; the HTTP server (`server.rs`) enforces both on every request.

pub mod tabs;

use tauri::Manager;

pub use tabs::{RemoteTab, TabRegistry};

/// Everything remote access keeps while Terax runs.
pub struct WebRemoteState {
    pub(crate) tabs: TabRegistry,
}

impl Default for WebRemoteState {
    fn default() -> Self {
        Self {
            tabs: TabRegistry::new(),
        }
    }
}

/// A window reports its tabs. The window label is the caller's, never the
/// payload's, so one window cannot publish as another.
#[tauri::command]
pub fn remote_publish_tabs(
    webview: tauri::Webview,
    state: tauri::State<'_, WebRemoteState>,
    tabs: Vec<RemoteTab>,
) {
    let before = state.tabs.pty_ids();
    state.tabs.publish(webview.label(), tabs);
    if state.tabs.pty_ids() != before {
        state.tabs.note_pty_change();
    }
}

/// Forgets a window's tabs when it closes.
pub fn forget_window(app: &tauri::AppHandle, label: &str) {
    if let Some(state) = app.try_state::<WebRemoteState>() {
        state.tabs.remove_window(label);
        state.tabs.note_pty_change();
    }
}
