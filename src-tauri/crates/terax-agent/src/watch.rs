//! Watching for the client: the core engine (debounce, the listing filter,
//! file stamps) on this machine's own notifications. `SetWatch` replaces the
//! whole set, so after a reconnect the client simply sends it again.

use std::collections::BTreeSet;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use terax_workspace_core::paths::{join, to_canon};
use terax_workspace_core::watch::plan::{self, FileStamp, ListingSnapshot};
use terax_workspace_core::watch::{ChangeNotes, Reporter, Watcher};
use terax_workspace_core::workspace::{EntryKind, Workspace};

use crate::roots::Roots;

/// Where `Changed` goes: the session's output.
pub type Send = Arc<dyn Fn(Vec<String>) + std::marker::Send + Sync>;

pub struct Watches {
    send: Send,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    watcher: Option<Watcher>,
    dirs: BTreeSet<String>,
    /// (canonical parent, canonical file)
    files: BTreeSet<(String, String)>,
}

/// The agent keeps no caches built from the tree.
struct NoNotes;

impl ChangeNotes for NoNotes {
    fn note_change(&self, _: &str) {}
}

struct Push(Send);

impl Reporter for Push {
    fn touched(&self, _: &[String]) {}

    fn changed(&self, paths: Vec<String>) {
        (self.0)(paths);
    }
}

impl Watches {
    pub fn new(send: Send) -> Self {
        Self {
            send,
            state: Mutex::default(),
        }
    }

    /// Replaces the watched set. Directories outside the roots, missing ones
    /// and files whose folder is outside are left out, not refused: one
    /// stale path must not cost the rest their watch.
    pub fn set(
        &self,
        ws: &dyn Workspace,
        roots: &Roots,
        dirs: &[String],
        files: &[String],
    ) -> io::Result<()> {
        let wanted_dirs: BTreeSet<String> = dirs
            .iter()
            .filter_map(|dir| {
                roots.follow(ws, dir).ok()?;
                let canonical = ws.canonicalize(dir).ok()?;
                let is_dir = ws.metadata(&canonical).ok()?.kind == EntryKind::Dir;
                (is_dir && !plan::is_skipped(Path::new(&canonical))).then_some(canonical)
            })
            .collect();
        let wanted_files: BTreeSet<(String, String)> = files
            .iter()
            .filter_map(|file| {
                let file = file.trim_end_matches('/');
                let at = file.rfind('/')?;
                let (parent, name) = (if at == 0 { "/" } else { &file[..at] }, &file[at + 1..]);
                if name.is_empty() {
                    return None;
                }
                roots.follow(ws, parent).ok()?;
                let parent = ws.canonicalize(parent).ok()?;
                let key = join(&parent, name);
                Some((parent, key))
            })
            .collect();

        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.watcher.is_none() {
            if wanted_dirs.is_empty() && wanted_files.is_empty() {
                return Ok(());
            }
            state.watcher = Some(start(self.send.clone())?);
        }
        let gone_dirs: Vec<String> = state.dirs.difference(&wanted_dirs).cloned().collect();
        let gone_files: Vec<(String, String)> =
            state.files.difference(&wanted_files).cloned().collect();
        let new_dirs: Vec<String> = wanted_dirs.difference(&state.dirs).cloned().collect();
        let new_files: Vec<(String, String)> =
            wanted_files.difference(&state.files).cloned().collect();

        let watcher = state.watcher.as_mut().expect("started above");
        if !gone_dirs.is_empty() || !gone_files.is_empty() {
            watcher.apply(|set| {
                let mut moved = false;
                for dir in &gone_dirs {
                    moved |= set.remove_dir(dir);
                }
                for (dir, key) in &gone_files {
                    set.remove_file(dir, key);
                }
                moved
            });
        }
        // Taken before the OS watch starts, so a change after it differs.
        let dirs: Vec<(PathBuf, ListingSnapshot)> = new_dirs
            .iter()
            .map(|dir| {
                let path = PathBuf::from(dir);
                let baseline = ListingSnapshot::read(&path, &plan::listing_signature);
                (path, baseline)
            })
            .collect();
        let files: Vec<(String, String, Option<FileStamp>)> = new_files
            .iter()
            .map(|(dir, key)| (dir.clone(), key.clone(), FileStamp::of(Path::new(key))))
            .collect();
        watcher.add(dirs, files);
        // The engine keys directories by `to_canon` of the path it was given.
        debug_assert!(new_dirs.iter().all(|d| to_canon(Path::new(d)) == *d));
        state.dirs = wanted_dirs;
        state.files = wanted_files;
        Ok(())
    }
}

#[cfg(not(target_os = "macos"))]
fn start(send: Send) -> io::Result<Watcher> {
    Watcher::start(Arc::new(NoNotes), Arc::new(Push(send)), |sink| {
        Ok(Box::new(
            terax_workspace_core::watch::portable::Notify::new(sink)?,
        ))
    })
    .map_err(|why| io::Error::other(format!("cannot watch on the server: {why}")))
}

#[cfg(target_os = "macos")]
fn start(_: Send) -> io::Result<Watcher> {
    let _ = (NoNotes, Push(Arc::new(|_| {})));
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "watching is not available on a macOS server",
    ))
}
