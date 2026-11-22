//! The backend for workspaces this process can open directly: the local
//! machine, and WSL distributions through their mapped host paths.

use std::fs;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::time::UNIX_EPOCH;

use crate::env::{resolve_path, WorkspaceEnv};
use crate::paths::to_canon;
use crate::workspace::{
    DirEntry, EntryKind, Meta, ReadOutcome, ReadRequest, Workspace, WriteOutcome,
};

pub struct HostWorkspace {
    env: WorkspaceEnv,
}

impl HostWorkspace {
    pub fn new(env: WorkspaceEnv) -> Self {
        Self { env }
    }

    fn host(&self, path: &str) -> PathBuf {
        resolve_path(path, &self.env)
    }
}

pub fn mtime_millis(meta: &fs::Metadata) -> u64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn kind_of(file_type: fs::FileType) -> EntryKind {
    if file_type.is_symlink() {
        EntryKind::Symlink
    } else if file_type.is_dir() {
        EntryKind::Dir
    } else if file_type.is_file() {
        EntryKind::File
    } else {
        EntryKind::Other
    }
}

fn meta_of(meta: &fs::Metadata) -> Meta {
    Meta {
        kind: kind_of(meta.file_type()),
        len: meta.len(),
        mtime_ms: mtime_millis(meta),
        readonly: meta.permissions().readonly(),
    }
}

fn not_a_file() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, "not a regular file")
}

impl Workspace for HostWorkspace {
    fn env(&self) -> &WorkspaceEnv {
        &self.env
    }

    fn metadata(&self, path: &str) -> io::Result<Meta> {
        fs::metadata(self.host(path)).map(|m| meta_of(&m))
    }

    fn symlink_metadata(&self, path: &str) -> io::Result<Meta> {
        fs::symlink_metadata(self.host(path)).map(|m| meta_of(&m))
    }

    fn canonicalize(&self, path: &str) -> io::Result<String> {
        let canon = to_canon(fs::canonicalize(self.host(path))?);
        match &self.env {
            WorkspaceEnv::Local => Ok(canon),
            #[cfg(windows)]
            WorkspaceEnv::Wsl { distro } => {
                crate::paths::canonical_wsl_path(&canon, distro).map_err(io::Error::other)
            }
            #[cfg(not(windows))]
            WorkspaceEnv::Wsl { .. } => Ok(canon),
        }
    }

    fn read_dir(&self, path: &str) -> io::Result<Vec<DirEntry>> {
        fs::read_dir(self.host(path))?
            .map(|entry| {
                let entry = entry?;
                Ok(DirEntry {
                    name: entry.file_name().to_string_lossy().into_owned(),
                    kind: kind_of(entry.file_type()?),
                })
            })
            .collect()
    }

    fn read(&self, path: &str, request: ReadRequest) -> io::Result<ReadOutcome> {
        let host = self.host(path);
        // Checked before opening: on Windows, opening a directory fails with
        // a misleading "Access is denied".
        if !fs::metadata(&host)?.is_file() {
            return Err(not_a_file());
        }
        let mut options = fs::OpenOptions::new();
        options.read(true);
        // A FIFO swapped in after the check must not block open().
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NONBLOCK);
        }
        let mut file = options.open(&host)?;
        let meta = file.metadata()?;
        if !meta.is_file() {
            return Err(not_a_file());
        }
        let size = meta.len();
        let start = request.offset.min(size);
        if start > 0 {
            file.seek(SeekFrom::Start(start))?;
        }
        // Bounded: the file may grow between the stat and the read.
        let mut bytes = Vec::new();
        file.take(request.limit.saturating_add(1))
            .read_to_end(&mut bytes)?;
        Ok(ReadOutcome {
            bytes,
            size,
            mtime_ms: mtime_millis(&meta),
        })
    }

    fn write_atomic(&self, path: &str, bytes: &[u8]) -> io::Result<WriteOutcome> {
        let target = self.host(path);
        let original = fs::metadata(&target).ok();
        let parent = target.parent().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "path has no parent")
        })?;
        // O_EXCL temp file with a random name in the target's directory, then
        // rename: the random suffix is what blocks pre-staged symlink attacks.
        let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
        tmp.as_file_mut().write_all(bytes)?;
        tmp.as_file_mut().sync_all()?;
        tmp.persist(&target).map_err(|e| e.error)?;
        if let Some(perms) = original.as_ref().map(|m| m.permissions()) {
            let _ = fs::set_permissions(&target, perms);
        }
        let mtime_ms = fs::metadata(&target).map(|m| mtime_millis(&m)).unwrap_or(0);
        Ok(WriteOutcome {
            created: original.is_none(),
            mtime_ms,
        })
    }

    fn create_file(&self, path: &str) -> io::Result<()> {
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(self.host(path))
            .map(drop)
    }

    fn create_dir_all(&self, path: &str) -> io::Result<()> {
        fs::create_dir_all(self.host(path))
    }

    fn rename(&self, from: &str, to: &str) -> io::Result<()> {
        fs::rename(self.host(from), self.host(to))
    }

    fn remove_file(&self, path: &str) -> io::Result<()> {
        fs::remove_file(self.host(path))
    }

    fn remove_dir_all(&self, path: &str) -> io::Result<()> {
        fs::remove_dir_all(self.host(path))
    }

    fn copy_file(&self, from: &str, to: &str) -> io::Result<u64> {
        fs::copy(self.host(from), self.host(to))
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn setup() -> (tempfile::TempDir, HostWorkspace, String) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_str().unwrap().to_owned();
        (dir, HostWorkspace::new(WorkspaceEnv::Local), root)
    }

    #[test]
    fn a_rewrite_keeps_the_file_mode() {
        let (_dir, ws, root) = setup();
        let path = format!("{root}/script.sh");
        ws.write_atomic(&path, b"#!/bin/sh").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o750)).unwrap();
        ws.write_atomic(&path, b"#!/bin/sh\necho hi").unwrap();
        assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o750);
    }

    #[test]
    fn symlinks_report_themselves_or_their_target() {
        let (_dir, ws, root) = setup();
        let target = format!("{root}/target.txt");
        let link = format!("{root}/link.txt");
        ws.write_atomic(&target, b"x").unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert_eq!(ws.symlink_metadata(&link).unwrap().kind, EntryKind::Symlink);
        assert_eq!(ws.metadata(&link).unwrap().kind, EntryKind::File);
        let listed = ws.read_dir(&root).unwrap();
        assert!(listed.contains(&DirEntry { name: "link.txt".into(), kind: EntryKind::Symlink }));
    }

    #[test]
    fn a_fifo_is_refused_without_blocking() {
        let (_dir, ws, root) = setup();
        let fifo = format!("{root}/pipe");
        let c = std::ffi::CString::new(fifo.clone()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
        let err = ws.read(&fifo, ReadRequest { offset: 0, limit: 10 }).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        assert_eq!(ws.metadata(&fifo).unwrap().kind, EntryKind::Other);
    }
}
