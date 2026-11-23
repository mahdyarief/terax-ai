//! The frontend's view of SSH connections. Outcomes arrive as
//! `remote://state` events; these commands only ask.

use std::path::PathBuf;
use std::sync::Arc;

use tauri::{AppHandle, Emitter, Manager};
use terax_workspace_core::env::SshHost;

use super::connector::SshConnector;
use super::registry::{self, Registry, Snapshot};
use crate::modules::control::ControlState;
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
    };
    let emitter = app.clone();
    registry::install(Registry::new(Arc::new(connector), move |snapshot| {
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
