use std::io::ErrorKind;

use serde::Serialize;
use base64::Engine;
use tauri::{Emitter, Manager};

use super::blocking;
use super::index::FileIndexState;
use super::mutate::note_changed_in;
use crate::modules::workspace::{backend_for, WorkspaceEnv};
use terax_workspace_core::workspace::{EntryKind, ReadRequest, Workspace};
#[cfg(windows)]
use terax_workspace_core::paths::canonical_wsl_path;

const MAX_READ_BYTES: u64 = 10 * 1024 * 1024; // 10 MB
/// Binary previews (DOCX/XLSX) carry base64 over IPC, so this matches
/// MAX_OFFICE_PREVIEW_BYTES in the renderer rather than the text limit.
const MAX_BINARY_BYTES: u64 = 20 * 1024 * 1024; // 20 MB
/// Ceiling for explicit "open anyway"; mirrored as FORCE_READ_LIMIT in useDocument.ts.
const FORCE_MAX_READ_BYTES: u64 = 50 * 1024 * 1024;
const BINARY_SNIFF_BYTES: usize = 8 * 1024;
/// Ceiling for `fs_read_head`: plenty for a few hundred table rows.
const MAX_HEAD_BYTES: u64 = 1024 * 1024;

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum ReadResult {
    Text {
        content: String,
        size: u64,
        mtime: u64,
    },
    Binary {
        size: u64,
    },
    /// File exceeds MAX_READ_BYTES. UI decides whether to offer "open anyway".
    TooLarge {
        size: u64,
        limit: u64,
    },
}

#[derive(Serialize)]
#[serde(rename_all = "lowercase")]
pub enum StatKind {
    File,
    Dir,
    Symlink,
}

#[derive(Serialize)]
pub struct FileStat {
    pub size: u64,
    pub mtime: u64,
    pub kind: StatKind,
}

#[derive(Serialize, Clone)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum ReadResultWithBytes {
    Binary {
        content: String,
        size: u64,
    },
    TooLarge {
        size: u64,
        limit: u64,
    },
}

#[tauri::command]
pub async fn fs_read_binary(
    path: String,
    workspace: Option<WorkspaceEnv>,
) -> Result<ReadResultWithBytes, String> {
    blocking(move || {
        let ws = backend_for(&WorkspaceEnv::from_option(workspace));
        let meta = ws.metadata(&path).map_err(|e| e.to_string())?;
        if meta.kind != EntryKind::File {
            return Err("only regular files can be read".into());
        }
        let size = meta.len;
        if size > MAX_BINARY_BYTES {
            return Ok(ReadResultWithBytes::TooLarge { size, limit: MAX_BINARY_BYTES });
        }
        let bytes = read_regular(&*ws, &path, 0, MAX_BINARY_BYTES, "only regular files can be read")?;
        if bytes.len() > MAX_BINARY_BYTES as usize {
            return Ok(ReadResultWithBytes::TooLarge { size, limit: MAX_BINARY_BYTES });
        }
        Ok(ReadResultWithBytes::Binary {
            content: base64::engine::general_purpose::STANDARD.encode(&bytes),
            size,
        })
    })
    .await
}

/// Same read as [`fs_read_binary`], but the bytes cross IPC raw (an
/// `ArrayBuffer` in the renderer) instead of as a base64 string inside JSON,
/// which cost a third more memory plus a decode pass for a 20 MB document.
#[tauri::command]
pub async fn fs_read_bytes(
    path: String,
    workspace: Option<WorkspaceEnv>,
) -> Result<tauri::ipc::Response, String> {
    blocking(move || {
        let ws = backend_for(&WorkspaceEnv::from_option(workspace));
        let meta = ws.metadata(&path).map_err(|e| e.to_string())?;
        if meta.kind != EntryKind::File {
            return Err("only regular files can be read".into());
        }
        let too_large = || format!("file is larger than {} MB", MAX_BINARY_BYTES / 1024 / 1024);
        if meta.len > MAX_BINARY_BYTES {
            return Err(too_large());
        }
        let bytes = read_regular(&*ws, &path, 0, MAX_BINARY_BYTES, "only regular files can be read")?;
        if bytes.len() > MAX_BINARY_BYTES as usize {
            return Err(too_large());
        }
        Ok(tauri::ipc::Response::new(bytes))
    })
    .await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HeadResult {
    pub(crate) content: String,
    /// The file continues past `content`.
    truncated: bool,
    size: u64,
    /// Byte offset the next chunk starts at. Null-byte sniff failures and
    /// partial-line trims change the byte length, so the renderer must not
    /// compute this from the encoded content length itself.
    next_offset: Option<u64>,
}

/// The start of a text file of any size, cut at a line boundary. Lets a table
/// preview show the first rows of a file too large to open in the editor.
///
/// `partial_line` allows a cut in the middle of a line. A table needs whole
/// rows, but minified JSON is one enormous line and would otherwise have no
/// preview at all.
#[tauri::command]
pub async fn fs_read_head(
    path: String,
    workspace: Option<WorkspaceEnv>,
    max_bytes: Option<u64>,
    partial_line: Option<bool>,
    offset: Option<u64>,
) -> Result<HeadResult, String> {
    blocking(move || {
        let ws = backend_for(&WorkspaceEnv::from_option(workspace));
        let meta = ws.metadata(&path).map_err(|e| e.to_string())?;
        if meta.kind != EntryKind::File {
            return Err("only regular files can be read".into());
        }
        let limit = max_bytes.unwrap_or(MAX_HEAD_BYTES).clamp(1, MAX_HEAD_BYTES);
        let start = offset.unwrap_or(0).min(meta.len);
        let bytes = read_regular(&*ws, &path, start, limit, "only regular files can be read")?;
        head_of(bytes, limit, meta.len, partial_line.unwrap_or(false), start)
    })
    .await
}

fn head_of(
    mut bytes: Vec<u8>,
    limit: u64,
    size: u64,
    partial_line: bool,
    offset: u64,
) -> Result<HeadResult, String> {
    let truncated = bytes.len() as u64 > limit;
    if bytes[..bytes.len().min(BINARY_SNIFF_BYTES)].contains(&0) {
        return Err("binary file".into());
    }
    if truncated {
        bytes.truncate(limit as usize);
        // Drop the partial last line (and any split UTF-8 sequence with it).
        // When a mid-line cut is allowed, only do so if it keeps most of what
        // was read; otherwise cut after the last complete character.
        match bytes.iter().rposition(|byte| *byte == b'\n') {
            Some(end) if !partial_line || end + 1 >= bytes.len() / 2 => bytes.truncate(end + 1),
            _ if partial_line => {
                if let Err(error) = std::str::from_utf8(&bytes) {
                    // `None` means the bytes simply stop inside a character.
                    if error.error_len().is_none() {
                        bytes.truncate(error.valid_up_to());
                    }
                }
            }
            _ => return Err("the first line is too long to preview".into()),
        }
        // A trim that consumes nothing would make the next page start where
        // this one did; fall back to the raw window instead of stalling.
        if bytes.is_empty() {
            return Err("the first line is too long to preview".into());
        }
    }
    let consumed = bytes.len() as u64;
    let content = String::from_utf8(bytes).map_err(|_| "binary file".to_string())?;
    Ok(HeadResult {
        content,
        truncated,
        size,
        next_offset: truncated.then(|| offset.saturating_add(consumed)),
    })
}

#[tauri::command]
pub async fn fs_read_file(
    path: String,
    workspace: Option<WorkspaceEnv>,
    force: Option<bool>,
) -> Result<ReadResult, String> {
    blocking(move || {
        let ws = backend_for(&WorkspaceEnv::from_option(workspace));
        read_file_sync(&*ws, &path, force.unwrap_or(false))
    })
    .await
}

const NOT_EDITABLE: &str = "only regular files can be opened in the editor";

pub(crate) fn read_file_sync(ws: &dyn Workspace, path: &str, force: bool) -> Result<ReadResult, String> {
    let before = ws.metadata(path).map_err(|e| {
        log::debug!("fs_read_file stat({path}) failed: {e}");
        e.to_string()
    })?;
    if before.kind != EntryKind::File {
        return Err(NOT_EDITABLE.into());
    }
    let limit = if force {
        FORCE_MAX_READ_BYTES
    } else {
        MAX_READ_BYTES
    };
    if before.len > limit {
        return Ok(ReadResult::TooLarge { size: before.len, limit });
    }

    // The backend refuses anything that is not a regular file once opened
    // (a FIFO or device swapped in after the check), without blocking.
    let read = ws
        .read(path, ReadRequest { offset: 0, limit })
        .map_err(|e| match e.kind() {
            ErrorKind::InvalidInput => NOT_EDITABLE.to_string(),
            _ => {
                log::debug!("fs_read_file read({path}) failed: {e}");
                e.to_string()
            }
        })?;
    if read.size > limit {
        return Ok(ReadResult::TooLarge { size: read.size, limit });
    }
    let bytes = read.bytes;
    let size = bytes.len() as u64;
    if size > limit {
        return Ok(ReadResult::TooLarge { size, limit });
    }

    // Null-byte sniff on the first chunk. Not perfect (misses UTF-16 BOM
    // cases) but catches the common "this is a PNG" mistake cheaply.
    let sniff_len = bytes.len().min(BINARY_SNIFF_BYTES);
    if bytes[..sniff_len].contains(&0) {
        return Ok(ReadResult::Binary { size });
    }

    match String::from_utf8(bytes) {
        Ok(content) => Ok(ReadResult::Text {
            content,
            size,
            mtime: read.mtime_ms,
        }),
        Err(_) => Ok(ReadResult::Binary { size }),
    }
}

/// Reads up to `limit + 1` bytes of a regular file, so callers can tell a
/// file that grew past the limit after it was checked.
fn read_regular(
    ws: &dyn Workspace,
    path: &str,
    offset: u64,
    limit: u64,
    not_regular: &str,
) -> Result<Vec<u8>, String> {
    ws.read(path, ReadRequest { offset, limit })
        .map(|read| read.bytes)
        .map_err(|e| match e.kind() {
            ErrorKind::InvalidInput => not_regular.to_string(),
            _ => e.to_string(),
        })
}

#[derive(Serialize, Clone)]
struct FileWrittenEvent {
    path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    source: Option<String>,
}

/// Returns the new mtime so the editor can track disk state for conflict
/// detection without a follow-up stat.
#[tauri::command]
pub async fn fs_write_file(
    path: String,
    content: String,
    workspace: Option<WorkspaceEnv>,
    source: Option<String>,
    app: tauri::AppHandle,
    index: tauri::State<'_, FileIndexState>,
) -> Result<u64, String> {
    let index = index.inner().clone();
    let workspace = WorkspaceEnv::from_option(workspace);
    let written_in = workspace.clone();
    let (path, mtime) = blocking(move || {
        let written = backend_for(&workspace)
            .write_atomic(&path, content.as_bytes())
            .map_err(|e| {
                log::warn!("fs_write_file({path}) failed: {e}");
                e.to_string()
            })?;
        if written.created {
            // The file index is host-side state keyed by host paths.
            note_changed_in(&index, &workspace, &path);
        }
        Ok((path, written.mtime_ms))
    })
    .await?;
    // A live server of the folder reloads its pages, also on a server where
    // nothing else watches the folder.
    app.state::<crate::modules::live_server::LiveServers>()
        .note_written(&path, &written_in);
    let _ = app.emit("fs:file-written", FileWrittenEvent { path, source });

    Ok(mtime)
}

#[tauri::command]
pub async fn fs_canonicalize(
    path: String,
    workspace: Option<WorkspaceEnv>,
    local_only: Option<bool>,
) -> Result<String, String> {
    validate_link_path(&path, local_only)?;
    blocking(move || {
        let workspace = WorkspaceEnv::from_option(workspace);
        // A link the user clicked must be a file on this machine: on every
        // platform, not only where the local branch below is compiled.
        if local_only == Some(true) && matches!(workspace, WorkspaceEnv::Ssh { .. }) {
            return Err(terax_workspace_core::env::not_on_this_machine().to_string());
        }
        #[cfg(windows)]
        if local_only == Some(true) {
            let canon =
                super::local_file::canonicalize(&super::local_file::open(&path, &workspace)?)?;
            return match &workspace {
                WorkspaceEnv::Wsl { distro } => canonical_wsl_path(&canon, distro),
                WorkspaceEnv::Local => Ok(canon),
                WorkspaceEnv::Ssh { .. } => {
                    Err(terax_workspace_core::env::not_on_this_machine().to_string())
                }
            };
        }
        backend_for(&workspace)
            .canonicalize(&path)
            .map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub async fn fs_stat(
    path: String,
    workspace: Option<WorkspaceEnv>,
    local_only: Option<bool>,
) -> Result<FileStat, String> {
    validate_link_path(&path, local_only)?;
    blocking(move || {
        #[cfg(windows)]
        if local_only == Some(true) {
            let file = super::local_file::open(&path, &WorkspaceEnv::from_option(workspace))?;
            let meta = file.metadata().map_err(|e| e.to_string())?;
            if !meta.is_file() && !meta.is_dir() {
                return Err("only regular files and directories can be opened".into());
            }
            return Ok(FileStat {
                size: meta.len(),
                mtime: terax_workspace_core::host::mtime_millis(&meta),
                kind: if meta.is_dir() {
                    StatKind::Dir
                } else {
                    StatKind::File
                },
            });
        }
        stat_path(path, workspace)
    })
    .await
}

fn validate_link_path(path: &str, local_only: Option<bool>) -> Result<(), String> {
    if local_only == Some(true)
        && path
            .as_bytes()
            .get(..2)
            .is_some_and(|prefix| prefix.iter().all(|byte| matches!(byte, b'/' | b'\\')))
    {
        return Err("network and device paths are not local file links".into());
    }
    Ok(())
}

fn stat_path(path: String, workspace: Option<WorkspaceEnv>) -> Result<FileStat, String> {
    let ws = backend_for(&WorkspaceEnv::from_option(workspace));
    let meta = ws.metadata(&path).map_err(|e| e.to_string())?;
    if meta.kind != EntryKind::File && meta.kind != EntryKind::Dir {
        return Err("only regular files and directories can be opened".into());
    }
    // metadata follows symlinks, so the link check needs symlink_metadata.
    let kind = if ws
        .symlink_metadata(&path)
        .is_ok_and(|m| m.kind == EntryKind::Symlink)
    {
        StatKind::Symlink
    } else if meta.kind == EntryKind::Dir {
        StatKind::Dir
    } else {
        StatKind::File
    };
    Ok(FileStat {
        size: meta.len,
        mtime: meta.mtime_ms,
        kind,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn read_path(path: &Path, force: bool) -> Result<ReadResult, String> {
        let ws = backend_for(&WorkspaceEnv::Local);
        read_file_sync(&*ws, &path.to_string_lossy(), force)
    }

    #[test]
    fn a_head_is_cut_at_the_last_whole_line() {
        // read_bounded hands over one byte past the limit when there is more.
        let head = head_of(b"a,b\nc,d\ne,f".to_vec(), 10, 11, false, 0).unwrap();
        assert_eq!(head.content, "a,b\nc,d\n");
        assert!(head.truncated);
        assert_eq!(head.size, 11);
    }

    #[test]
    fn a_head_that_holds_the_whole_file_is_not_truncated() {
        let head = head_of(b"a,b\nc,d".to_vec(), 100, 7, false, 0).unwrap();
        assert_eq!(head.content, "a,b\nc,d");
        assert!(!head.truncated);
    }

    #[test]
    fn a_head_never_splits_a_multibyte_character() {
        // The limit lands inside the second "é" (two bytes each).
        let head = head_of("x\né\né".as_bytes().to_vec(), 6, 7, false, 0).unwrap();
        assert_eq!(head.content, "x\né\n");
    }

    #[test]
    fn a_head_refuses_binary_and_single_line_overflow() {
        assert!(head_of(vec![b'a', 0, b'b'], 10, 3, false, 0).is_err());
        assert!(head_of(vec![b'a'; 11], 10, 11, false, 0).is_err());
    }

    #[test]
    fn head_may_cut_inside_a_line_when_asked() {
        // Minified JSON: one line, no newline to cut at.
        let head = head_of(vec![b'a'; 11], 10, 11, true, 0).unwrap();
        assert_eq!(head.content, "a".repeat(10));
        assert!(head.truncated);

        // The cut never splits a multi-byte character: "é" is two bytes.
        let head = head_of("aaaaé".as_bytes().to_vec(), 5, 6, true, 0).unwrap();
        assert_eq!(head.content, "aaaa");

        // A newline near the end is still preferred over a mid-line cut...
        let head = head_of(b"aaaaaaa\nbbb".to_vec(), 10, 11, true, 0).unwrap();
        assert_eq!(head.content, "aaaaaaa\n");
        // ...but not one that would throw away most of what was read.
        let head = head_of(b"a\nbbbbbbbbb".to_vec(), 10, 11, true, 0).unwrap();
        assert_eq!(head.content, "a\nbbbbbbbb");

        // Binary content is still refused.
        assert!(head_of(vec![b'a', 0, b'b'], 2, 3, true, 0).is_err());
    }

    #[test]
    fn a_truncated_head_reports_where_the_next_page_starts() {
        // The 11-byte source is cut to 10, then trimmed back to the last whole
        // line ("a,b\nc,d\n" = 8 bytes) before the page is returned.
        let head = head_of(b"a,b\nc,d\ne,f".to_vec(), 10, 11, false, 0).unwrap();
        assert_eq!(head.next_offset, Some(8));

        // An offset is carried into the next page, not reset to the file start.
        let head = head_of(b"a,b\nc,d\ne,f".to_vec(), 10, 11, false, 50).unwrap();
        assert_eq!(head.next_offset, Some(58));

        // A whole-file read has nothing to continue.
        let head = head_of(b"a,b\nc,d".to_vec(), 100, 7, false, 0).unwrap();
        assert_eq!(head.next_offset, None);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn devices_and_fifos_never_reach_editor_reads() {
        let dir = tempfile::tempdir().unwrap();
        let fifo = dir.path().join("pipe");
        let name = std::ffi::CString::new(fifo.as_os_str().as_encoded_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        let linked_device = dir.path().join("zero-link");
        std::os::unix::fs::symlink("/dev/zero", &linked_device).unwrap();
        for path in [
            Path::new("/dev/zero"),
            Path::new("/dev/null"),
            &fifo,
            &linked_device,
        ] {
            assert!(read_path(path, false)
                .err()
                .unwrap()
                .contains("regular"));
            assert!(read_path(path, true)
                .err()
                .unwrap()
                .contains("regular"));
            assert!(fs_stat(path.to_string_lossy().into_owned(), None, None)
                .await
                .err()
                .unwrap()
                .contains("regular"));
        }
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn local_file_links_reject_leaf_and_parent_reparse_points() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target");
        std::fs::create_dir(&target).unwrap();
        std::fs::write(target.join("file"), "safe").unwrap();
        let local = dir.path().join("local-link");
        std::os::windows::fs::symlink_dir(&target, &local).unwrap();
        let remote = dir.path().join("remote-link");
        std::os::windows::fs::symlink_dir(r"\\terax-unreachable.invalid\share", &remote).unwrap();
        for path in [&local, &local.join("file"), &remote, &remote.join("file")] {
            let path = super::super::to_canon(path);
            assert!(fs_stat(path.clone(), None, Some(true))
                .await
                .err()
                .unwrap()
                .contains("reparse"));
            assert!(fs_canonicalize(path, None, Some(true))
                .await
                .err()
                .unwrap()
                .contains("reparse"));
        }
        let plain = super::super::to_canon(target.join("file"));
        assert_eq!(
            fs_stat(plain.clone(), None, Some(true)).await.unwrap().size,
            4
        );
        assert_eq!(
            fs_canonicalize(plain.clone(), None, Some(true))
                .await
                .unwrap(),
            super::super::to_canon(std::fs::canonicalize(&plain).unwrap())
        );
    }

    #[tokio::test]
    async fn local_file_links_reject_network_paths_before_filesystem_access() {
        for path in [
            r"\\server\share\file",
            "//server/share/file",
            r"/\server/share/file",
            r"\/server\share\file",
            r"\\?\C:\file",
            r"\\.\pipe\name",
            r"/\?\UNC\server\share",
            r"\/.\pipe\name",
        ] {
            let expected = "network and device paths are not local file links";
            assert_eq!(
                fs_stat(path.into(), None, Some(true)).await.err().unwrap(),
                expected
            );
            assert_eq!(
                fs_canonicalize(path.into(), None, Some(true))
                    .await
                    .unwrap_err(),
                expected
            );
            assert!(validate_link_path(path, None).is_ok());
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn link_parent_segments_resolve_after_symlinks() {
        let dir = tempfile::tempdir().unwrap();
        let workspace = dir.path().join("workspace");
        let external = dir.path().join("external");
        std::fs::create_dir_all(&workspace).unwrap();
        std::fs::create_dir_all(external.join("build")).unwrap();
        std::fs::write(external.join("main.ts"), "outside").unwrap();
        std::fs::write(workspace.join("main.ts"), "inside").unwrap();
        std::os::unix::fs::symlink(external.join("build"), workspace.join("build")).unwrap();
        let path = workspace
            .join("build/../main.ts")
            .to_string_lossy()
            .into_owned();
        assert_eq!(
            fs_stat(path.clone(), None, Some(true)).await.unwrap().size,
            7
        );
        assert_eq!(
            fs_canonicalize(path, None, Some(true)).await.unwrap(),
            super::super::to_canon(std::fs::canonicalize(external.join("main.ts")).unwrap())
        );
    }

    #[tokio::test]
    async fn binary_reads_return_base64_not_a_byte_array() {
        use base64::Engine;
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("a.docx");
        std::fs::write(&f, b"PK\x03\x04binary").unwrap();
        match fs_read_binary(f.to_string_lossy().into_owned(), None)
            .await
            .unwrap()
        {
            ReadResultWithBytes::Binary { content, size } => {
                assert_eq!(size, 10);
                assert_eq!(
                    base64::engine::general_purpose::STANDARD.decode(content).unwrap(),
                    b"PK\x03\x04binary"
                );
            }
            ReadResultWithBytes::TooLarge { .. } => panic!("expected binary"),
        }
    }

    #[test]
    fn binary_reads_serialize_with_a_kind_tag() {
        // The renderer discriminates on `kind`; a bare `{ "binary": ... }`
        // object silently reads as null and the preview shows nothing.
        let json = serde_json::to_value(ReadResultWithBytes::Binary {
            content: "QQ==".into(),
            size: 1,
        })
        .unwrap();
        assert_eq!(json["kind"], "binary");
        assert_eq!(json["content"], "QQ==");
        assert_eq!(json["size"], 1);

        let json = serde_json::to_value(ReadResultWithBytes::TooLarge { size: 4, limit: 2 }).unwrap();
        assert_eq!(json["kind"], "toolarge");
        assert_eq!(json["limit"], 2);
    }

    #[tokio::test]
    async fn binary_reads_reject_directories() {
        let dir = tempfile::tempdir().unwrap();
        assert!(fs_read_binary(dir.path().to_string_lossy().into_owned(), None)
            .await
            .err()
            .unwrap()
            .contains("regular"));
    }

    #[test]
    fn read_file_classifies_utf8_as_text() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("a.txt");
        std::fs::write(&f, b"hello world").unwrap();
        match read_path(&f, false).unwrap() {
            ReadResult::Text {
                content,
                size,
                mtime,
            } => {
                assert_eq!(content, "hello world");
                assert_eq!(size, 11);
                assert!(mtime > 0);
            }
            _ => panic!("expected text"),
        }
    }

    #[test]
    fn read_file_detects_binary_via_null_byte() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("a.bin");
        std::fs::write(&f, b"PNG\0\x89image").unwrap();
        assert!(matches!(
            read_path(&f, false).unwrap(),
            ReadResult::Binary { .. }
        ));
    }

    #[test]
    fn read_file_detects_binary_via_invalid_utf8() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("a.bin");
        // Invalid UTF-8 with no null byte: must still classify as binary.
        std::fs::write(&f, [0xff, 0xfe, 0xfd, 0xfc]).unwrap();
        assert!(matches!(
            read_path(&f, false).unwrap(),
            ReadResult::Binary { .. }
        ));
    }

    #[test]
    fn force_lifts_the_default_size_limit() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("big.txt");
        std::fs::write(&f, vec![b'a'; (MAX_READ_BYTES + 1) as usize]).unwrap();
        assert!(matches!(
            read_path(&f, false).unwrap(),
            ReadResult::TooLarge { .. }
        ));
        assert!(matches!(
            read_path(&f, true).unwrap(),
            ReadResult::Text { .. }
        ));
    }

}
