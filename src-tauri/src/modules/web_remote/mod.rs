//! Remote access: watch and drive every terminal tab from a browser.
//!
//! Off until the user turns it on. Each paired device gets its own token and
//! grant; the HTTP server (`server.rs`) enforces both on every request.

pub mod devices;
pub mod http_rules;
pub mod server;
pub mod sse;
pub mod stream;
pub mod tabs;

use std::path::PathBuf;
use std::sync::atomic::AtomicUsize;
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::{Emitter, Manager};
use tokio::sync::watch;

pub use devices::{DeviceInfo, Grant};
pub use server::{RemoteAccessInfo, RemoteBind};
pub use tabs::{RemoteTab, TabRegistry};

use devices::{DeviceStore, Pairing, PAIRING_TTL};
use server::{Deps, Running};

/// The port the Settings UI offers and the server uses when none is given.
pub const DEFAULT_PORT: u16 = 7681;

/// Everything remote access keeps while Terax runs.
pub struct WebRemoteState {
    pub(crate) tabs: Arc<TabRegistry>,
    pub(crate) running: Mutex<Option<Running>>,
    devices: Mutex<Arc<DeviceStore>>,
    pairing: Arc<Pairing>,
}

impl Default for WebRemoteState {
    fn default() -> Self {
        Self {
            tabs: Arc::new(TabRegistry::new()),
            running: Mutex::new(None),
            devices: Mutex::new(Arc::new(DeviceStore::memory())),
            pairing: Arc::new(Pairing::default()),
        }
    }
}

impl WebRemoteState {
    pub(crate) fn status(&self) -> RemoteAccessInfo {
        self.running
            .lock()
            .unwrap()
            .as_ref()
            .map(Running::info)
            .unwrap_or_else(RemoteAccessInfo::off)
    }

    pub(crate) fn devices(&self) -> Arc<DeviceStore> {
        self.devices.lock().unwrap().clone()
    }

    /// Loads the paired devices from app data. Called once at setup, before
    /// any listener starts.
    pub fn attach_device_store(&self, path: Option<PathBuf>) {
        *self.devices.lock().unwrap() = Arc::new(DeviceStore::load(path));
    }
}

fn emit_status(app: &tauri::AppHandle, info: &RemoteAccessInfo) {
    let _ = app.emit("terax:remote-access", info);
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

/// Turns remote access on. The listener only binds where the chosen policy
/// allows; binding every interface over plain HTTP needs `allowPlainLan`.
#[tauri::command]
pub async fn remote_access_start(
    app: tauri::AppHandle,
    state: tauri::State<'_, WebRemoteState>,
    bind: RemoteBind,
    port: Option<u16>,
    allow_plain_lan: Option<bool>,
) -> Result<RemoteAccessInfo, String> {
    let port = port.unwrap_or(DEFAULT_PORT);
    let allow_plain_lan = allow_plain_lan.unwrap_or(false);
    if let Some(running) = state.running.lock().unwrap().as_ref() {
        if running.bind == bind
            && running.port == port
            && running.allow_plain_lan == allow_plain_lan
        {
            return Ok(running.info());
        }
    }
    // Restarting: every existing stream ends with the old listener.
    if let Some(old) = state.running.lock().unwrap().take() {
        let _ = old.stop.send(true);
    }
    let bound = server::bind(bind, port, allow_plain_lan, Vec::new(), false).await?;
    let (stop, stopped) = watch::channel(false);
    let viewers = Arc::new(AtomicUsize::new(0));
    let deps = Deps {
        assets: Arc::new(server::BundledAssets(app.clone())),
        tabs: state.tabs.clone(),
        hosts: bound.hosts.clone(),
        viewers: viewers.clone(),
        devices: state.devices(),
        pairing: state.pairing.clone(),
        sessions: Arc::new(stream::AppSessions(app.clone())),
        limits: Arc::new(stream::StreamLimits::default()),
        events: Arc::new(server::AppTabEvents(app.clone())),
        https: false,
    };
    let running = Running {
        bind,
        port: bound.port,
        urls: bound.urls,
        allow_plain_lan,
        https: false,
        viewers,
        stop,
        _serve: None,
    };
    let info = running.info();
    tauri::async_runtime::spawn(server::accept_loop(bound.listener, deps, stopped));
    *state.running.lock().unwrap() = Some(running);
    log::info!("remote access listening on port {}", info.port.unwrap_or(0));
    emit_status(&app, &info);
    Ok(info)
}

#[tauri::command]
pub fn remote_access_stop(app: tauri::AppHandle, state: tauri::State<'_, WebRemoteState>) {
    if let Some(running) = state.running.lock().unwrap().take() {
        let _ = running.stop.send(true);
    }
    let info = state.status();
    emit_status(&app, &info);
}

#[tauri::command]
pub fn remote_access_status(state: tauri::State<'_, WebRemoteState>) -> RemoteAccessInfo {
    state.status()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PairingInfo {
    pub code: String,
    /// The URL to open on the new device; the code rides the fragment.
    pub url: String,
    pub expires_in_sec: u64,
}

/// A one-time code, valid for ten minutes and five attempts. Remote access
/// must be on: the URL points at the running listener.
#[tauri::command]
pub fn remote_pairing_start(
    state: tauri::State<'_, WebRemoteState>,
) -> Result<PairingInfo, String> {
    let url = state
        .running
        .lock()
        .unwrap()
        .as_ref()
        .and_then(|running| running.urls.first().cloned())
        .ok_or("Turn remote access on first.")?;
    let code = state.pairing.start();
    Ok(PairingInfo {
        url: format!("{url}#pair={code}"),
        code,
        expires_in_sec: PAIRING_TTL.as_secs(),
    })
}

#[tauri::command]
pub fn remote_devices_list(state: tauri::State<'_, WebRemoteState>) -> Vec<DeviceInfo> {
    state.devices().list()
}

#[tauri::command]
pub fn remote_device_update(
    state: tauri::State<'_, WebRemoteState>,
    id: String,
    grant: Grant,
) -> Result<(), String> {
    if state.devices().update(&id, grant) {
        Ok(())
    } else {
        Err("No such device.".into())
    }
}

#[tauri::command]
pub fn remote_device_revoke(
    state: tauri::State<'_, WebRemoteState>,
    id: String,
) -> Result<(), String> {
    if state.devices().revoke(&id) {
        Ok(())
    } else {
        Err("No such device.".into())
    }
}

/// Forgets a window's tabs when it closes.
pub fn forget_window(app: &tauri::AppHandle, label: &str) {
    if let Some(state) = app.try_state::<WebRemoteState>() {
        state.tabs.remove_window(label);
        state.tabs.note_pty_change();
    }
}
