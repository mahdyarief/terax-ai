//! The frontend's view of SSH connections. Outcomes arrive as
//! `remote://state` events; these commands only ask.

use std::path::PathBuf;
use std::sync::Arc;

use tauri::{AppHandle, Emitter, Manager};
use terax_workspace_core::env::SshHost;

use super::connector::SshConnector;
use super::machine::State;
use super::registry::{self, Registry, Snapshot};
use super::watch::RemoteWatches;
use crate::modules::control::ControlState;
use crate::modules::fs::index::FileIndexState;
use crate::modules::remote_agent::{dev_root, AgentBundle};

pub const STATE_EVENT: &str = "remote://state";

/// Installs the app's registry; called once from `setup`.
pub fn install(app: &AppHandle, control: ControlState) {
    let connector = SshConnector {
        ssh: "ssh".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        agents: AgentBundle::new(
            app.path().resource_dir().ok(),
            dev_root().map(PathBuf::from),
        ),
        control,
        ssh_options: Vec::new(),
    };
    let emitter = app.clone();
    registry::install(Registry::new(Arc::new(connector), move |snapshot| {
        if matches!(snapshot.state, State::Ready) {
            if let Ok(host) = SshHost::try_from(snapshot.host.clone()) {
                // Files may have changed while nothing was watching.
                if let Some(index) = emitter.try_state::<FileIndexState>() {
                    index.forget_host(&host);
                }
                resume_watching(&emitter, host);
            }
        }
        let _ = emitter.emit(STATE_EVENT, snapshot);
    }));
}

fn parse(host: String) -> Result<SshHost, String> {
    SshHost::try_from(host)
}

fn registry() -> Result<&'static Registry, String> {
    registry::global().ok_or_else(|| "remote connections are not available".to_string())
}

#[tauri::command]
pub fn remote_connect(host: String) -> Result<(), String> {
    registry()?.connect(&parse(host)?);
    Ok(())
}

#[tauri::command]
pub fn remote_disconnect(host: String) -> Result<(), String> {
    registry()?.disconnect(&parse(host)?);
    Ok(())
}

#[tauri::command]
pub fn remote_state() -> Result<Vec<Snapshot>, String> {
    Ok(registry()?.states())
}

#[tauri::command]
pub fn remote_log(host: String) -> Result<Vec<String>, String> {
    Ok(registry()?.log(&parse(host)?))
}

/// Host names from the user's `~/.ssh/config`, for the picker.
#[tauri::command]
pub fn remote_ssh_hosts() -> Vec<String> {
    dirs::home_dir()
        .map(|home| super::ssh_config::hosts(&home))
        .unwrap_or_default()
}

fn live(host: &SshHost) -> Result<Arc<terax_remote_protocol::client::RemoteWorkspace>, String> {
    registry()?
        .workspace(host)
        .ok_or_else(|| registry::not_connected(host).to_string())
}

/// The server user's home directory, where the folder picker starts.
#[tauri::command]
pub async fn remote_home(host: String) -> Result<String, String> {
    let host = parse(host)?;
    crate::modules::fs::blocking(move || live(&host)?.home().map_err(|e| e.to_string())).await
}

/// A server directory's canonical path and its subdirectories, for the
/// folder picker (names only; allowed before the folder is authorized).
#[tauri::command]
pub async fn remote_browse_dirs(
    host: String,
    path: String,
) -> Result<terax_remote_protocol::message::Browse, String> {
    let host = parse(host)?;
    crate::modules::fs::blocking(move || live(&host)?.browse_dirs(&path).map_err(|e| e.to_string()))
        .await
}

/// A new connection gets the host's watched set again, off the registry's
/// thread (it waits for the agent).
fn resume_watching(app: &AppHandle, host: SshHost) {
    let app = app.clone();
    std::thread::spawn(move || {
        let Some(ws) = registry::global().and_then(|r| r.workspace(&host)) else {
            return;
        };
        let set = app.state::<RemoteWatches>().set(&host);
        let deliver = crate::modules::fs::watch::remote_deliver(&app);
        if let Err(error) = super::watch::resume(&ws, &host, set, deliver) {
            log::warn!("watching on {}: {error}", host.as_str());
        }
    });
}
