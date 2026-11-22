//! The operations every workspace backend provides.
//!
//! Paths are *workspace paths*: what the user sees for that workspace
//! (`C:\work\x` locally, `/home/me/x` in WSL, and later a path on a remote
//! server). Each backend maps them to something it can open. Errors are
//! `io::Error` so callers keep producing today's messages; a backend that
//! learns of an error elsewhere (the remote agent) rebuilds one with the
//! same kind and text.
//!
//! Phase 1 starts with the primitives. Grep, glob, watch and exec join this
//! trait as their engines move into the core.

use std::io;
use std::path::Path;

use crate::env::WorkspaceEnv;
use crate::grep::{GlobRequest, GlobResponse, GrepRequest, GrepResponse};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryKind {
    File,
    Dir,
    Symlink,
    /// Devices, FIFOs, sockets.
    Other,
}

/// The part of a file's metadata the app uses, in a form that can cross a
/// process boundary (unlike `std::fs::Metadata`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Meta {
    pub kind: EntryKind,
    pub len: u64,
    /// Milliseconds since the Unix epoch; 0 when the platform has no mtime.
    pub mtime_ms: u64,
    pub readonly: bool,
    /// Opaque; changes when the entry is modified or replaced. Lets a caller
    /// confirm that what the user approved is still what is on disk.
    pub token: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirEntry {
    pub name: String,
    /// The entry itself, not what a symlink points to.
    pub kind: EntryKind,
}

/// A window into a regular file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReadRequest {
    /// Byte offset to start at; clamped to the file's length.
    pub offset: u64,
    /// The caller's limit. Up to `limit + 1` bytes come back, so a caller can
    /// tell "exactly the limit" from "more than the limit" without a re-stat.
    pub limit: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReadOutcome {
    pub bytes: Vec<u8>,
    /// The file's length when it was opened.
    pub size: u64,
    pub mtime_ms: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WriteOutcome {
    /// The file did not exist before the write.
    pub created: bool,
    pub mtime_ms: u64,
}

pub trait Workspace: Send + Sync {
    fn env(&self) -> &WorkspaceEnv;

    /// Metadata of the path, following symlinks.
    fn metadata(&self, path: &str) -> io::Result<Meta>;
    /// Metadata of the path itself; a symlink reports `EntryKind::Symlink`.
    fn symlink_metadata(&self, path: &str) -> io::Result<Meta>;
    /// The canonical workspace path (symlinks and `..` resolved).
    fn canonicalize(&self, path: &str) -> io::Result<String>;
    /// The directory's entries, in no particular order.
    fn read_dir(&self, path: &str) -> io::Result<Vec<DirEntry>>;
    /// Reads from a regular file. Anything else (a directory, a FIFO, a
    /// device, also one swapped in after a check) fails with
    /// `ErrorKind::InvalidInput` and never blocks.
    fn read(&self, path: &str, request: ReadRequest) -> io::Result<ReadOutcome>;
    /// Replaces the file's content atomically: readers see the old content or
    /// the new, never a partial file. An existing file keeps its permissions.
    /// The parent directory must exist.
    fn write_atomic(&self, path: &str, bytes: &[u8]) -> io::Result<WriteOutcome>;
    /// Creates an empty file; fails with `AlreadyExists` if the path exists.
    fn create_file(&self, path: &str) -> io::Result<()>;
    fn create_dir_all(&self, path: &str) -> io::Result<()>;
    /// Moves an entry. Never replaces one: an existing destination fails with
    /// `AlreadyExists` (or `DirectoryNotEmpty`), atomically where the
    /// platform allows, and both entries are left as they were.
    fn rename(&self, from: &str, to: &str) -> io::Result<()>;
    /// Moves a file over an existing file, replacing it atomically.
    fn replace_file(&self, from: &str, to: &str) -> io::Result<()>;
    fn remove_file(&self, path: &str) -> io::Result<()>;
    fn remove_dir_all(&self, path: &str) -> io::Result<()>;
    /// Copies one file's bytes; returns the number of bytes copied.
    fn copy_file(&self, from: &str, to: &str) -> io::Result<u64>;

    // ── engines ─────────────────────────────────────────────────────────

    /// Copies a file or folder from this machine (`source` is a host path)
    /// into the workspace directory `dest_dir`, named after the source, and
    /// returns the name it got. The copy appears complete or not at all and
    /// never replaces an existing entry; `rename_on_conflict` picks
    /// "name copy", "name copy 2", ... instead of failing.
    fn import(&self, source: &Path, dest_dir: &str, rename_on_conflict: bool)
        -> io::Result<String>;

    /// Searches file contents under `request.root`, honouring `.gitignore`.
    /// Polls `cancel` and stops early when it turns true.
    fn grep(
        &self,
        request: &GrepRequest,
        cancel: &(dyn Fn() -> bool + Sync),
    ) -> Result<GrepResponse, String>;

    /// Lists files under `request.root` matching a glob, honouring `.gitignore`.
    fn glob(&self, request: &GlobRequest) -> Result<GlobResponse, String>;
}
