//! Live directory change notifications for the explorer and editor tabs.
//!
//! The frontend registers the directories it shows (and the files open in
//! editors) and receives debounced `fs:changed` events carrying the watched
//! directories that changed plus any registered file whose own stamp moved.
//! Backends only report directory-level activity, so a package install or a
//! build never turns into per-file work here.

#[cfg(target_os = "macos")]
mod macos;

use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

use tauri::{AppHandle, Emitter, Manager};

use super::blocking;
use super::index::FileIndexState;
use super::to_canon;
use crate::modules::workspace::{resolve_path, WorkspaceEnv, WorkspaceRegistry};
// The engine lives in the core; the macOS backend below plugs into it.
pub(crate) use terax_workspace_core::watch::plan;
#[cfg(target_os = "macos")]
pub(crate) use terax_workspace_core::watch::{Backend, Sink};
use terax_workspace_core::watch::{ChangeNotes, Reporter, Watcher};
use plan::{FileStamp, ListingSnapshot};

#[cfg(target_os = "macos")]
type PlatformBackend = macos::FsEvents;
#[cfg(not(target_os = "macos"))]
type PlatformBackend = terax_workspace_core::watch::portable::Notify;

/// Receives the watched directories a burst touched, before the explorer's
/// listing filter is applied (so in-place file edits are included). Runs on the
/// drain thread: it must only hand the paths off.
pub type Tap = Arc<dyn Fn(&[String]) + Send + Sync>;

type Taps = Arc<RwLock<Vec<(u64, Tap)>>>;

#[derive(Default)]
pub struct FsWatchState {
    inner: Mutex<Option<Watcher>>,
    taps: Taps,
    next_tap: std::sync::atomic::AtomicU64,
}

#[derive(Clone, serde::Serialize)]
struct ChangedPayload {
    paths: Vec<String>,
}

impl ChangeNotes for FileIndexState {
    fn note_change(&self, path: &str) {
        FileIndexState::note_change(self, path);
    }
}

/// Sends a burst to the internal taps and the frontend.
struct AppReporter {
    app: AppHandle,
    taps: Taps,
}

impl Reporter for AppReporter {
    fn touched(&self, dirs: &[String]) {
        let listeners: Vec<Tap> = self
            .taps
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .map(|(_, tap)| tap.clone())
            .collect();
        for tap in listeners {
            tap(dirs);
        }
    }

    fn changed(&self, paths: Vec<String>) {
        let _ = self.app.emit("fs:changed", ChangedPayload { paths });
    }
}

fn start(app: &AppHandle, taps: Taps) -> Result<Watcher, String> {
    let index = app.state::<FileIndexState>().inner().clone();
    let reporter = AppReporter {
        app: app.clone(),
        taps,
    };
    Watcher::start(Arc::new(index), Arc::new(reporter), |sink| {
        Ok(Box::new(PlatformBackend::new(sink)?))
    })
}

// Canonical keys keep add/remove symmetric regardless of how the path was spelled.
fn prepare_dirs(
    registry: &WorkspaceRegistry,
    workspace: &WorkspaceEnv,
    paths: Vec<String>,
) -> Vec<PathBuf> {
    paths
        .into_iter()
        .filter_map(|raw| {
            let resolved = resolve_path(&raw, workspace);
            let canonical = std::fs::canonicalize(&resolved).ok()?;
            if !canonical.is_dir()
                || plan::is_skipped(&canonical)
                || !registry.is_authorized(&canonical)
            {
                return None;
            }
            Some(canonical)
        })
        .collect()
}

/// A file is identified by its canonical directory plus its own name, so the
/// path we report matches what editor tabs store even when the file itself no
/// longer exists.
fn canonical_file(workspace: &WorkspaceEnv, raw: &str) -> Option<(String, String, PathBuf)> {
    let resolved = resolve_path(raw, workspace);
    let name = resolved.file_name()?.to_owned();
    let parent = std::fs::canonicalize(resolved.parent()?).ok()?;
    let file = parent.join(name);
    Some((to_canon(&parent), to_canon(&file), file))
}

impl FsWatchState {
    fn with_running<T>(
        &self,
        app: &AppHandle,
        create: bool,
        f: impl FnOnce(&mut Watcher) -> T,
    ) -> Result<Option<T>, String> {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if guard.is_none() {
            if !create {
                return Ok(None);
            }
            *guard = Some(start(app, self.taps.clone())?);
        }
        Ok(guard.as_mut().map(f))
    }

    /// Registers `tap` for every burst; the id removes it again.
    pub(crate) fn add_tap(&self, tap: Tap) -> u64 {
        let id = self
            .next_tap
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.taps
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .push((id, tap));
        id
    }

    pub(crate) fn remove_tap(&self, id: u64) {
        self.taps
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .retain(|(tap_id, _)| *tap_id != id);
    }

    /// Watches already-canonical, already-vetted directories for an internal
    /// consumer. Reference counted with the explorer's own registrations.
    pub(crate) fn watch_dirs(&self, app: &AppHandle, dirs: Vec<PathBuf>) -> Result<(), String> {
        if dirs.is_empty() {
            return Ok(());
        }
        let baselines: Vec<ListingSnapshot> = dirs
            .iter()
            .map(|dir| ListingSnapshot::read(dir, &plan::listing_signature))
            .collect();
        self.with_running(app, true, |running| {
            running.add(dirs.into_iter().zip(baselines), []);
        })?;
        Ok(())
    }

    pub(crate) fn unwatch_dirs(&self, app: &AppHandle, dirs: &[PathBuf]) {
        if dirs.is_empty() {
            return;
        }
        let _ = self.with_running(app, false, |running| {
            running.apply(|set| {
                let mut moved = false;
                for dir in dirs {
                    moved |= set.remove_dir(&to_canon(dir));
                }
                moved
            });
        });
    }
}

#[tauri::command]
pub async fn fs_watch_add(
    app: AppHandle,
    paths: Vec<String>,
    files: Option<Vec<String>>,
    workspace: Option<WorkspaceEnv>,
) -> Result<(), String> {
    blocking(move || {
        let workspace = WorkspaceEnv::from_option(workspace);
        let dirs = prepare_dirs(&app.state::<WorkspaceRegistry>(), &workspace, paths);
        let files: Vec<_> = files
            .unwrap_or_default()
            .iter()
            .filter_map(|raw| canonical_file(&workspace, raw))
            .map(|(dir, key, path)| {
                let stamp = FileStamp::of(&path);
                (dir, key, stamp)
            })
            .collect();
        if dirs.is_empty() && files.is_empty() {
            return Ok(());
        }
        // Taken before the OS watch starts (inside `apply`), so a change after it differs.
        let baselines: Vec<ListingSnapshot> = dirs
            .iter()
            .map(|dir| ListingSnapshot::read(dir, &plan::listing_signature))
            .collect();
        app.state::<FsWatchState>()
            .with_running(&app, true, |running| {
                running.add(dirs.into_iter().zip(baselines), files);
            })?;
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn fs_watch_remove(
    app: AppHandle,
    paths: Vec<String>,
    files: Option<Vec<String>>,
    workspace: Option<WorkspaceEnv>,
) -> Result<(), String> {
    blocking(move || {
        let workspace = WorkspaceEnv::from_option(workspace);
        // A removed/renamed dir no longer canonicalizes; fall back so the
        // refcount entry is still released.
        let dirs: Vec<String> = paths
            .into_iter()
            .map(|raw| {
                let resolved = resolve_path(&raw, &workspace);
                to_canon(std::fs::canonicalize(&resolved).unwrap_or(resolved))
            })
            .collect();
        let files: Vec<(String, String)> = files
            .unwrap_or_default()
            .iter()
            .filter_map(|raw| canonical_file(&workspace, raw))
            .map(|(dir, key, _)| (dir, key))
            .collect();
        app.state::<FsWatchState>()
            .with_running(&app, false, |running| {
                running.apply(|set| {
                    let mut moved = false;
                    for dir in &dirs {
                        moved |= set.remove_dir(dir);
                    }
                    for (dir, key) in &files {
                        set.remove_file(dir, key);
                    }
                    moved
                });
            })?;
        Ok(())
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn taps_are_registered_and_removed_by_id_and_cost_nothing_when_absent() {
        let state = FsWatchState::default();
        assert!(state.taps.read().unwrap().is_empty());

        let seen = Arc::new(Mutex::new(Vec::<String>::new()));
        let sink = seen.clone();
        let first = state.add_tap(Arc::new(move |dirs: &[String]| {
            sink.lock().unwrap().extend(dirs.iter().cloned());
        }));
        let second = state.add_tap(Arc::new(|_: &[String]| {}));
        assert_ne!(first, second);
        assert_eq!(state.taps.read().unwrap().len(), 2);

        for (_, tap) in state.taps.read().unwrap().iter() {
            tap(&["/watched/dir".to_string()]);
        }
        assert_eq!(*seen.lock().unwrap(), vec!["/watched/dir".to_string()]);

        state.remove_tap(first);
        state.remove_tap(first);
        let remaining = state.taps.read().unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].0, second);
    }
}
