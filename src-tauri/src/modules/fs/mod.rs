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

pub(crate) use terax_workspace_core::paths::display_path;
pub use terax_workspace_core::paths::to_canon;
#[cfg_attr(not(windows), allow(unused_imports))]
use terax_workspace_core::paths::strip_verbatim;
