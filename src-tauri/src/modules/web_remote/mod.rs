//! Remote access: watch and drive every terminal tab from a browser.
//!
//! Off until the user turns it on. Each paired device gets its own token and
//! grant; the HTTP server (`server.rs`) enforces both on every request.

pub mod app;
pub mod devices;
pub mod http_rules;
pub mod https;
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
    relay_sessions: Mutex<Option<Arc<app::sessions::SessionManager>>>,
}

impl Default for WebRemoteState {
    fn default() -> Self {
        Self {
            tabs: Arc::new(TabRegistry::new()),
            running: Mutex::new(None),
            devices: Mutex::new(Arc::new(DeviceStore::memory())),
            pairing: Arc::new(Pairing::default()),
            relay_sessions: Mutex::new(None),
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

    pub fn relay_sessions(&self) -> Option<Arc<app::sessions::SessionManager>> {
        self.relay_sessions.lock().unwrap().clone()
    }

    /// Loads the paired devices from app data. Called once at setup, before
    /// any listener starts.
    pub fn attach_device_store(&self, path: Option<PathBuf>) {
        *self.devices.lock().unwrap() = Arc::new(DeviceStore::load(path));
    }
}

/// What `--remote-serve` asked for. Unknown flags keep their defaults.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ServeOptions {
    pub enabled: bool,
    pub bind: RemoteBind,
    pub port: u16,
}

impl Default for ServeOptions {
    fn default() -> Self {
        Self {
            enabled: false,
            bind: RemoteBind::Loopback,
            port: DEFAULT_PORT,
        }
    }
}

fn bind_from(value: &str) -> Option<RemoteBind> {
    match value {
        "loopback" | "this-computer" => Some(RemoteBind::Loopback),
        "tailscale" => Some(RemoteBind::Tailscale),
        "all" | "lan" => Some(RemoteBind::All),
        _ => None,
    }
}

/// `--remote-serve [--bind loopback|tailscale|all] [--port N]`, also accepting
/// the `--flag=value` spelling.
pub fn parse_serve_args(args: impl IntoIterator<Item = String>) -> ServeOptions {
    let mut options = ServeOptions::default();
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        if arg == "--remote-serve" {
            options.enabled = true;
        } else if arg == "--bind" {
            if let Some(bind) = args.next().as_deref().and_then(bind_from) {
                options.bind = bind;
            }
        } else if let Some(value) = arg.strip_prefix("--bind=") {
            if let Some(bind) = bind_from(value) {
                options.bind = bind;
            }
        } else if arg == "--port" {
            if let Some(port) = args.next().and_then(|value| value.parse().ok()) {
                options.port = port;
            }
        } else if let Some(port) = arg
            .strip_prefix("--port=")
            .and_then(|value| value.parse().ok())
        {
            options.port = port;
        }
    }
    options
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
    https: Option<bool>,
) -> Result<RemoteAccessInfo, String> {
    start_listener(
        &app,
        &state,
        bind,
        port.unwrap_or(DEFAULT_PORT),
        allow_plain_lan.unwrap_or(false),
        https.unwrap_or(false),
    )
    .await
}

/// Starts (or restarts) the listener. Shared by the Settings command and the
/// `--remote-serve` flag, which has no window in front of it.
pub async fn start_listener(
    app: &tauri::AppHandle,
    state: &WebRemoteState,
    bind: RemoteBind,
    port: u16,
    allow_plain_lan: bool,
    https: bool,
) -> Result<RemoteAccessInfo, String> {
    // `tailscale serve` terminates TLS on 443 and proxies to the loopback
    // listener, so HTTPS only makes sense where a tailnet address exists.
    let https = https && bind == RemoteBind::Tailscale;
    if let Some(running) = state.running.lock().unwrap().as_ref() {
        if running.bind == bind
            && running.port == port
            && running.allow_plain_lan == allow_plain_lan
            && running.https == https
        {
            return Ok(running.info());
        }
    }
    let mut extra_names = Vec::new();
    let mut public_urls = None;
    if https {
        let dns = https::magic_dns().await?;
        https::claim(&format!("http://127.0.0.1:{port}")).await?;
        public_urls = Some(vec![format!("https://{dns}/web-index.html")]);
        extra_names.push(dns);
    }
    // Restarting: every existing stream ends with the old listener.
    if let Some(old) = state.running.lock().unwrap().take() {
        let _ = old.stop.send(true);
    }
    let bound = server::bind(bind, port, allow_plain_lan, extra_names, https).await?;
    let urls = public_urls.unwrap_or(bound.urls);
    let (stop, stopped) = watch::channel(false);
    let viewers = Arc::new(AtomicUsize::new(0));
    let relay = Arc::new(app::sessions::SessionManager::new(Arc::new(
        app::sessions::AppRelayHost(app.clone()),
    )));
    if let Some(old_relay) = state.relay_sessions.lock().unwrap().replace(relay.clone()) {
        old_relay.close_all();
    }
    app::sessions::spawn_reaper(relay.clone(), state.devices(), stopped.clone());
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
        https,
        relay: Some(relay),
    };
    let running = Running {
        bind,
        port: bound.port,
        urls,
        allow_plain_lan,
        https,
        viewers,
        stop,
        _serve: None,
    };
    let info = running.info();
    tauri::async_runtime::spawn(server::accept_loop(bound.listener, deps, stopped));
    *state.running.lock().unwrap() = Some(running);
    log::info!("remote access listening on port {}", info.port.unwrap_or(0));
    emit_status(app, &info);
    Ok(info)
}

#[tauri::command]
pub fn remote_access_stop(app: tauri::AppHandle, state: tauri::State<'_, WebRemoteState>) {
    if let Some(relay) = state.relay_sessions.lock().unwrap().take() {
        relay.close_all();
    }
    if let Some(running) = state.running.lock().unwrap().take() {
        let _ = running.stop.send(true);
        if running.https {
            tauri::async_runtime::spawn(async {
                if let Err(error) = https::release().await {
                    log::warn!("could not remove the tailscale serve mount: {error}");
                }
            });
        }
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

impl WebRemoteState {
    /// A one-time code, valid for ten minutes and five attempts. Remote
    /// access must be on: the URL points at the running listener.
    pub fn start_pairing(&self) -> Result<PairingInfo, String> {
        let url = self
            .running
            .lock()
            .unwrap()
            .as_ref()
            .and_then(|running| running.urls.first().cloned())
            .ok_or("Turn remote access on first.")?;
        let code = self.pairing.start();
        Ok(PairingInfo {
            url: format!("{url}#pair={code}"),
            code,
            expires_in_sec: PAIRING_TTL.as_secs(),
        })
    }
}

#[tauri::command]
pub fn remote_pairing_start(
    state: tauri::State<'_, WebRemoteState>,
) -> Result<PairingInfo, String> {
    state.start_pairing()
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
        if !grant.app {
            if let Some(relay) = state.relay_sessions() {
                relay.close_for_device(&id);
            }
        }
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
        if let Some(relay) = state.relay_sessions() {
            relay.close_for_device(&id);
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|arg| arg.to_string()).collect()
    }

    #[test]
    fn remote_serve_defaults_to_loopback_on_the_usual_port() {
        let options = parse_serve_args(args(&["--remote-serve"]));
        assert!(options.enabled);
        assert_eq!(options.bind, RemoteBind::Loopback);
        assert_eq!(options.port, DEFAULT_PORT);
    }

    #[test]
    fn the_bind_and_port_follow_the_flags() {
        let options = parse_serve_args(args(&[
            "--remote-serve",
            "--bind",
            "tailscale",
            "--port",
            "7681",
        ]));
        assert_eq!(options.bind, RemoteBind::Tailscale);
        assert_eq!(options.port, 7681);

        let joined = parse_serve_args(args(&["--remote-serve", "--bind=all", "--port=9000"]));
        assert_eq!(joined.bind, RemoteBind::All);
        assert_eq!(joined.port, 9000);
    }

    #[test]
    fn pairing_needs_a_running_listener() {
        let state = WebRemoteState::default();
        assert!(state.start_pairing().is_err());
    }

    #[test]
    fn a_missing_or_unknown_value_keeps_the_default() {
        let options = parse_serve_args(args(&["--bind", "carrier-pigeon", "--port", "many"]));
        assert!(!options.enabled);
        assert_eq!(options.bind, RemoteBind::Loopback);
        assert_eq!(options.port, DEFAULT_PORT);
    }
}
