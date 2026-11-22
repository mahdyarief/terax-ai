pub mod ai;
pub mod assets;
pub mod clipboard;
pub mod copy;
pub mod file;
pub mod grep;
pub mod index;
#[cfg(windows)]
mod local_file;
pub mod mutate;
pub mod preview;
pub mod project_memory;
mod read_policy;
pub mod search;
pub mod supersede;
pub mod tree;
pub mod watch;

use std::path::Path;

use crate::modules::workspace::WorkspaceEnv;

/// Runs blocking filesystem work off the async workers and the main thread.
/// Tauri runs a non-async command inline in the IPC handler (the main thread on
/// macOS) and the async runtime has two workers, so every command that touches
/// the disk goes through here.
pub(crate) async fn blocking<T, F>(work: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|e| e.to_string())?
}

/// How a path under a search root is shown to the frontend. WSL roots keep the
/// caller's spelling so the result stays a valid path inside the distro.
pub(crate) fn display_path(
    path: &Path,
    root_path: &Path,
    root_display: &str,
    workspace: &WorkspaceEnv,
) -> String {
    if workspace.is_wsl() {
        if let Ok(rel) = path.strip_prefix(root_path) {
            let rel = to_canon(rel);
            return if rel.is_empty() {
                root_display.to_string()
            } else if root_display.ends_with('/') {
                format!("{root_display}{rel}")
            } else {
                format!("{root_display}/{rel}")
            };
        }
    }
    to_canon(path)
}

pub use terax_workspace_core::paths::to_canon;
#[cfg_attr(not(windows), allow(unused_imports))]
use terax_workspace_core::paths::strip_verbatim;
