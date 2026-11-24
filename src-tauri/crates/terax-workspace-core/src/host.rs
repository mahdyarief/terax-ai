//! The backend for workspaces this process can open directly: the local
//! machine, and WSL distributions through their mapped host paths.

use std::fs;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use crate::confined::{ConfinedError, ConfinedOutcome, ConfinedRead};
use crate::env::{resolve_path, WorkspaceEnv};
use crate::grep::{GlobRequest, GlobResponse, GrepRequest, GrepResponse};
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

/// Opens `path` for reading without ever opening a device or blocking on a
/// FIFO. On Linux the path is pinned with `O_PATH` (which opens nothing) and
/// the same inode is reopened for reading only once it is a regular file.
fn open_pinned(path: &Path) -> Result<fs::File, ConfinedError> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        #[cfg(target_os = "linux")]
        options.custom_flags(libc::O_PATH);
        #[cfg(not(target_os = "linux"))]
        options.custom_flags(libc::O_NONBLOCK);
    }
    #[cfg(windows)]
    {
        // Opening a directory fails with a misleading "Access is denied".
        if fs::metadata(path).is_ok_and(|meta| !meta.is_file()) {
            return Err(ConfinedError::NotAFile);
        }
    }
    let file = options.open(path)?;
    #[cfg(target_os = "linux")]
    {
        reopen_regular(&file)
    }
    #[cfg(not(target_os = "linux"))]
    {
        Ok(file)
    }
}

/// Reopens the inode an `O_PATH` handle pinned, for reading, once it is a
/// regular file; a path replaced in between is not followed.
#[cfg(target_os = "linux")]
fn reopen_regular(pinned: &fs::File) -> Result<fs::File, ConfinedError> {
    use std::os::fd::AsRawFd;
    if !pinned.metadata()?.is_file() {
        return Err(ConfinedError::NotAFile);
    }
    Ok(fs::File::open(format!("/proc/self/fd/{}", pinned.as_raw_fd()))?)
}

/// Where the opened file really is, from the OS (not from the path it was
/// opened by). On Windows the lossless verbatim path.
fn opened_path(file: &fs::File) -> io::Result<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        use std::os::fd::AsRawFd;
        use std::os::unix::ffi::OsStrExt;
        let mut buffer = [0u8; libc::PATH_MAX as usize];
        if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETPATH, buffer.as_mut_ptr()) } == -1 {
            return Err(io::Error::last_os_error());
        }
        let end = buffer
            .iter()
            .position(|&b| b == 0)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid file path"))?;
        Ok(PathBuf::from(std::ffi::OsStr::from_bytes(&buffer[..end])))
    }
    #[cfg(target_os = "linux")]
    {
        use std::os::fd::AsRawFd;
        fs::read_link(format!("/proc/self/fd/{}", file.as_raw_fd()))
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStringExt;
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::GetFinalPathNameByHandleW;
        let mut buffer = vec![0u16; 32768];
        let len = unsafe {
            GetFinalPathNameByHandleW(
                file.as_raw_handle(),
                buffer.as_mut_ptr(),
                buffer.len() as u32,
                0,
            )
        } as usize;
        if len == 0 || len >= buffer.len() {
            return Err(io::Error::last_os_error());
        }
        Ok(PathBuf::from(std::ffi::OsString::from_wide(&buffer[..len])))
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
        token: conflict_token(meta),
    }
}

fn not_a_file() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, "not a regular file")
}

// ── Platform primitives, on host paths ─────────────────────────────────

#[cfg(unix)]
pub fn conflict_token(metadata: &fs::Metadata) -> String {
    use std::os::unix::fs::MetadataExt;
    format!(
        "{}:{}:{}:{}:{}:{}",
        metadata.dev(),
        metadata.ino(),
        metadata.mode(),
        metadata.size(),
        metadata.mtime(),
        metadata.mtime_nsec()
    )
}

#[cfg(target_os = "windows")]
pub fn conflict_token(metadata: &fs::Metadata) -> String {
    use std::os::windows::fs::MetadataExt;
    format!(
        "{}:{}:{}:{}",
        metadata.file_attributes(),
        metadata.creation_time(),
        metadata.last_write_time(),
        metadata.file_size()
    )
}

#[cfg(not(any(unix, target_os = "windows")))]
pub fn conflict_token(metadata: &fs::Metadata) -> String {
    use std::time::UNIX_EPOCH;
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |duration| duration.as_nanos());
    format!("{}:{modified}", metadata.len())
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn c_path(path: &Path) -> io::Result<std::ffi::CString> {
    use std::os::unix::ffi::OsStrExt;
    std::ffi::CString::new(path.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "path contains a NUL byte"))
}

#[cfg(target_os = "macos")]
pub fn rename_no_replace(from: &Path, to: &Path) -> io::Result<()> {
    let from = c_path(from)?;
    let to = c_path(to)?;
    let result = unsafe { libc::renamex_np(from.as_ptr(), to.as_ptr(), libc::RENAME_EXCL) };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// Through the system call itself: musl (the static agent binary) and glibc
/// before 2.28 have no `renameat2` wrapper, and the wrapper only forwards.
#[cfg(target_os = "linux")]
pub fn rename_no_replace(from: &Path, to: &Path) -> io::Result<()> {
    let from = c_path(from)?;
    let to = c_path(to)?;
    let result = unsafe {
        libc::syscall(
            libc::SYS_renameat2,
            libc::AT_FDCWD,
            from.as_ptr(),
            libc::AT_FDCWD,
            to.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(target_os = "windows")]
fn move_file_windows(from: &Path, to: &Path, replace: bool) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };

    let from: Vec<u16> = from.as_os_str().encode_wide().chain(Some(0)).collect();
    let to: Vec<u16> = to.as_os_str().encode_wide().chain(Some(0)).collect();
    let flags = MOVEFILE_WRITE_THROUGH
        | if replace {
            MOVEFILE_REPLACE_EXISTING
        } else {
            0
        };
    let result = unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), flags) };
    if result != 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(target_os = "windows")]
pub fn rename_no_replace(from: &Path, to: &Path) -> io::Result<()> {
    move_file_windows(from, to, false)
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
pub fn rename_no_replace(from: &Path, to: &Path) -> io::Result<()> {
    if fs::symlink_metadata(to).is_ok() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "destination already exists",
        ));
    }
    fs::rename(from, to)
}

#[cfg(unix)]
pub fn replace_file(from: &Path, to: &Path) -> io::Result<()> {
    fs::rename(from, to)
}

#[cfg(target_os = "windows")]
pub fn replace_file(from: &Path, to: &Path) -> io::Result<()> {
    move_file_windows(from, to, true)
}

#[cfg(not(any(unix, target_os = "windows")))]
pub fn replace_file(from: &Path, to: &Path) -> io::Result<()> {
    fs::rename(from, to)
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

    fn read_link(&self, path: &str) -> io::Result<String> {
        let host = self.host(path);
        match fs::read_link(&host) {
            Ok(target) => Ok(target.to_string_lossy().into_owned()),
            // Windows reports a plain file as "not a reparse point"; say what
            // every backend says.
            Err(error) => match fs::symlink_metadata(&host) {
                Ok(meta) if !meta.file_type().is_symlink() => Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "not a symbolic link",
                )),
                _ => Err(error),
            },
        }
    }

    fn canonicalize(&self, path: &str) -> io::Result<String> {
        let canonical = fs::canonicalize(self.host(path))?;
        // A lossy spelling could equal another, real path's; callers compare
        // canonical paths to authorize, so refuse rather than approximate.
        if canonical.to_str().is_none() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "canonical path is not valid Unicode",
            ));
        }
        let canon = to_canon(canonical);
        match &self.env {
            WorkspaceEnv::Local => Ok(canon),
            #[cfg(windows)]
            WorkspaceEnv::Wsl { distro } => {
                crate::paths::canonical_wsl_path(&canon, distro).map_err(io::Error::other)
            }
            #[cfg(not(windows))]
            WorkspaceEnv::Wsl { .. } => Ok(canon),
            WorkspaceEnv::Ssh { .. } => Err(crate::env::not_on_this_machine()),
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

    fn read_confined(
        &self,
        path: &str,
        request: &ConfinedRead,
    ) -> Result<ConfinedOutcome, ConfinedError> {
        let file = open_pinned(&self.host(path))?;
        let meta = file.metadata()?;
        if !meta.is_file() {
            return Err(ConfinedError::NotAFile);
        }
        let opened = opened_path(&file)?;
        // Compared as text below, so it must be spelled exactly.
        if opened.to_str().is_none() {
            return Err(ConfinedError::Inexact);
        }
        let canon = to_canon(&opened);
        let real_path = match &self.env {
            WorkspaceEnv::Local => canon,
            #[cfg(windows)]
            WorkspaceEnv::Wsl { distro } => crate::paths::canonical_wsl_path(&canon, distro)
                .map_err(|_| ConfinedError::Outside)?,
            #[cfg(not(windows))]
            WorkspaceEnv::Wsl { .. } => canon,
            WorkspaceEnv::Ssh { .. } => {
                return Err(ConfinedError::Io(crate::env::not_on_this_machine()))
            }
        };
        if request.secret_policy {
            if let Some(why) = crate::confined::refusal(&real_path) {
                return Err(ConfinedError::Refused(why.to_string()));
            }
        }
        if !crate::confined::allowed(&real_path, &request.roots) {
            return Err(ConfinedError::Outside);
        }
        let size = meta.len();
        let mut file = file;
        let start = request.offset.min(size);
        if start > 0 {
            file.seek(SeekFrom::Start(start))?;
        }
        let mut bytes = Vec::new();
        file.take(request.limit.saturating_add(1))
            .read_to_end(&mut bytes)?;
        Ok(ConfinedOutcome {
            real_path,
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
        rename_no_replace(&self.host(from), &self.host(to))
    }

    fn replace_file(&self, from: &str, to: &str) -> io::Result<()> {
        replace_file(&self.host(from), &self.host(to))
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

    fn import(&self, source: &Path, dest_dir: &str, rename_on_conflict: bool) -> io::Result<String> {
        let dest = fs::canonicalize(self.host(dest_dir))?;
        crate::import::import_item(source, &dest, rename_on_conflict)
            .map(|name| name.to_string_lossy().into_owned())
    }

    fn grep(
        &self,
        request: &GrepRequest,
        cancel: &(dyn Fn() -> bool + Sync),
    ) -> Result<GrepResponse, String> {
        crate::grep::grep(&self.host(&request.root), &self.env, request, cancel)
    }

    fn glob(&self, request: &GlobRequest) -> Result<GlobResponse, String> {
        crate::grep::glob(&self.host(&request.root), &self.env, request)
    }

    fn list_files(
        &self,
        root: &str,
        show_hidden: bool,
        max_depth: Option<usize>,
        cancelled: &dyn Fn() -> bool,
    ) -> Option<crate::index::FileIndex> {
        crate::index::walk(&self.host(root), show_hidden, max_depth, cancelled)
    }

    fn exec(&self, spec: crate::exec::ExecSpec) -> io::Result<crate::exec::capture::Captured> {
        crate::exec::run_on_host(&self.env, spec)
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
        assert_eq!(ws.read_link(&link).unwrap(), target);
        std::os::unix::fs::symlink("missing", format!("{root}/dangling")).unwrap();
        assert_eq!(ws.read_link(&format!("{root}/dangling")).unwrap(), "missing");
        let listed = ws.read_dir(&root).unwrap();
        assert!(listed.contains(&DirEntry { name: "link.txt".into(), kind: EntryKind::Symlink }));
    }

    #[test]
    #[cfg_attr(
        target_os = "macos",
        ignore = "APFS refuses a name that is not valid UTF-8"
    )]
    fn a_canonical_path_that_is_not_unicode_is_refused() {
        use std::os::unix::ffi::OsStrExt;
        let (_dir, ws, root) = setup();
        let odd = std::path::Path::new(&root).join(std::ffi::OsStr::from_bytes(b"repo\xff"));
        fs::create_dir(&odd).unwrap();
        std::os::unix::fs::symlink(&odd, format!("{root}/plain")).unwrap();
        let err = ws.canonicalize(&format!("{root}/plain")).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    fn confined_to(root: &str) -> ConfinedRead {
        ConfinedRead {
            roots: vec![crate::confined::Confine {
                path: root.to_string(),
                subtree: true,
            }],
            secret_policy: true,
            offset: 0,
            limit: 1024,
        }
    }

    /// The decision follows the opened file, not the path it was opened by:
    /// a link inside the root to a file outside it, or to a secret, is refused.
    #[test]
    fn confined_reads_follow_the_opened_file() {
        let (dir, ws, _) = setup();
        let root = ws.canonicalize(&dir.path().to_string_lossy()).unwrap();
        let inside = format!("{root}/inside");
        fs::create_dir_all(&inside).unwrap();
        fs::create_dir_all(format!("{root}/outside")).unwrap();
        fs::write(format!("{root}/outside/b.txt"), b"nope").unwrap();
        fs::write(format!("{inside}/.env"), b"SECRET").unwrap();
        fs::write(format!("{inside}/a.txt"), b"ok").unwrap();
        std::os::unix::fs::symlink(format!("{root}/outside/b.txt"), format!("{inside}/escape")).unwrap();
        std::os::unix::fs::symlink(format!("{inside}/.env"), format!("{inside}/innocent.txt")).unwrap();
        std::os::unix::fs::symlink(format!("{inside}/a.txt"), format!("{inside}/alias.txt")).unwrap();

        let request = confined_to(&inside);
        assert!(matches!(
            ws.read_confined(&format!("{inside}/escape"), &request),
            Err(ConfinedError::Outside)
        ));
        assert!(matches!(
            ws.read_confined(&format!("{inside}/innocent.txt"), &request),
            Err(ConfinedError::Refused(_))
        ));
        let read = ws.read_confined(&format!("{inside}/alias.txt"), &request).unwrap();
        assert_eq!(read.real_path, format!("{inside}/a.txt"));
        assert_eq!(read.bytes, b"ok");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn reopening_a_pinned_regular_file_does_not_follow_a_replaced_path() {
        use std::os::unix::fs::OpenOptionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("file.txt");
        fs::write(&path, "original").unwrap();
        let pinned = fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_PATH)
            .open(&path)
            .unwrap();
        fs::remove_file(&path).unwrap();
        let replacement = dir.path().join("replacement.txt");
        fs::write(&replacement, "replacement").unwrap();
        std::os::unix::fs::symlink(&replacement, &path).unwrap();
        let mut actual = String::new();
        reopen_regular(&pinned)
            .expect("the pinned inode is a regular file")
            .read_to_string(&mut actual)
            .unwrap();
        assert_eq!(actual, "original");
    }

    #[test]
    fn confined_reads_never_open_a_fifo_or_a_device() {
        let (dir, ws, _) = setup();
        let root = ws.canonicalize(&dir.path().to_string_lossy()).unwrap();
        let fifo = format!("{root}/pipe");
        let c = std::ffi::CString::new(fifo.clone()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
        let mut request = confined_to(&root);
        assert!(matches!(ws.read_confined(&fifo, &request), Err(ConfinedError::NotAFile)));
        request.roots.push(crate::confined::Confine {
            path: "/dev".into(),
            subtree: true,
        });
        assert!(matches!(
            ws.read_confined("/dev/zero", &request),
            Err(ConfinedError::NotAFile)
        ));
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

    #[test]
    fn does_not_follow_legacy_staging_symlink() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().unwrap();
        let outside = dir.path().join("outside.txt");
        std::fs::write(&outside, b"untouched").unwrap();

        let target = dir.path().join("note.txt");
        // Pre-stage a symlink at the legacy deterministic staging path.
        let legacy = dir.path().join(".note.txt.terax.tmp");
        symlink(&outside, &legacy).unwrap();

        HostWorkspace::new(WorkspaceEnv::Local)
            .write_atomic(target.to_str().unwrap(), b"payload")
            .unwrap();

        assert_eq!(std::fs::read(&target).unwrap(), b"payload");
        // The pre-staged symlink target must not have been written through.
        assert_eq!(std::fs::read(&outside).unwrap(), b"untouched");
    }
}

#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;
    use crate::confined::Confine;
    use std::os::windows::process::CommandExt;

    /// `mklink /J` makes a junction without the symlink privilege. Paths are
    /// quoted for cmd; a fixture path must not need more than quotes.
    fn junction(link: &Path, target: &Path) {
        for path in [link, target] {
            assert!(!path.to_str().unwrap().contains(['%', '"', '\r', '\n']));
        }
        let result = std::process::Command::new("cmd.exe")
            .args(["/D", "/V:OFF", "/C"])
            .raw_arg(format!(
                "mklink /J \"{}\" \"{}\"",
                link.display(),
                target.display()
            ))
            .output()
            .unwrap();
        assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
    }

    /// A name with an unpaired surrogate spells as `repo\u{FFFD}`, the same
    /// text as a real sibling; neither canonicalize nor a confined read may
    /// hand that spelling out, since callers compare it to authorize.
    #[test]
    fn a_path_that_cannot_be_spelled_exactly_is_refused() {
        use std::os::windows::ffi::OsStringExt;
        let dir = tempfile::tempdir().unwrap();
        let ws = HostWorkspace::new(WorkspaceEnv::Local);
        let granted = dir.path().join("repo\u{FFFD}");
        let lossy = dir
            .path()
            .join(std::ffi::OsString::from_wide(&[0x72, 0x65, 0x70, 0x6f, 0xD800]));
        fs::create_dir(&granted).unwrap();
        fs::create_dir(&lossy).unwrap();
        fs::write(lossy.join("private.txt"), b"SECRET").unwrap();
        // cmd cannot be handed this name, so not a junction.
        if let Err(error) = std::os::windows::fs::symlink_dir(&lossy, granted.join("link")) {
            eprintln!("skipped: no symlink privilege ({error})");
            return;
        }
        let root = ws.canonicalize(&granted.to_string_lossy()).unwrap();
        let through_link = format!("{root}/link/private.txt");

        let canonical = ws.canonicalize(&through_link);
        assert_eq!(
            canonical.map_err(|e| e.kind()),
            Err(io::ErrorKind::InvalidData),
            "the lossy spelling would read as inside {root}"
        );
        let request = ConfinedRead {
            roots: vec![Confine {
                path: root,
                subtree: true,
            }],
            secret_policy: false,
            offset: 0,
            limit: 1024,
        };
        assert!(matches!(
            ws.read_confined(&through_link, &request),
            Err(ConfinedError::Inexact)
        ));
    }

    /// A junction inside the root that leads out of it, or into a protected
    /// directory, is refused by the opened file's real path.
    #[test]
    fn confined_reads_follow_the_opened_file_through_a_junction() {
        let dir = tempfile::tempdir().unwrap();
        let ws = HostWorkspace::new(WorkspaceEnv::Local);
        let root = ws.canonicalize(&dir.path().to_string_lossy()).unwrap();
        let inside = dir.path().join("inside");
        let outside = dir.path().join("outside");
        let protected = inside.join("keys").join(".ssh");
        fs::create_dir_all(&inside).unwrap();
        fs::create_dir_all(&outside).unwrap();
        fs::create_dir_all(&protected).unwrap();
        fs::write(outside.join("b.txt"), b"nope").unwrap();
        fs::write(protected.join("config"), b"SECRET").unwrap();
        junction(&inside.join("escape"), &outside);
        junction(&inside.join("innocent"), &protected);

        let request = ConfinedRead {
            roots: vec![Confine {
                path: format!("{root}/inside"),
                subtree: true,
            }],
            secret_policy: true,
            offset: 0,
            limit: 1024,
        };
        let path = |rel: &str| format!("{root}/inside/{rel}");
        assert!(matches!(
            ws.read_confined(&path("escape/b.txt"), &request),
            Err(ConfinedError::Outside)
        ));
        assert!(matches!(
            ws.read_confined(&path("innocent/config"), &request),
            Err(ConfinedError::Refused(_))
        ));
        fs::write(inside.join("a.txt"), b"ok").unwrap();
        let read = ws.read_confined(&path("a.txt"), &request).unwrap();
        assert_eq!(read.real_path, path("a.txt"), "the stripped, forward-slash spelling");
        assert_eq!(read.bytes, b"ok");
    }
}
