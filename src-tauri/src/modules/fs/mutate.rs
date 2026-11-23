use super::blocking;
use super::index::FileIndexState;
use crate::modules::workspace::{
    backend_for, resolve_path, same_spelling, within, WorkspaceEnv, WorkspaceRegistry,
};
use serde::Serialize;
use std::io;
use std::path::Path;
use tauri::{AppHandle, Manager, State};
use terax_workspace_core::workspace::{EntryKind, Meta, Workspace};

/// Tells the file-list cache that something under `path` changed. The watcher
/// only covers expanded directories, so our own mutations report themselves.
pub(super) fn note_changed(index: &FileIndexState, entry: &Path) {
    note_dir_changed(index, entry.parent().unwrap_or(entry));
}

pub(super) fn note_dir_changed(index: &FileIndexState, dir: &Path) {
    let canonical = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    index.note_change(&super::to_canon(canonical));
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(
    tag = "status",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum FsMoveResult {
    Moved,
    Conflict { replaceable: bool, token: String },
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FsDeleteBatchResult {
    deleted: Vec<String>,
    failed: usize,
}

fn metadata_if_exists(ws: &dyn Workspace, path: &str) -> io::Result<Option<Meta>> {
    match ws.symlink_metadata(path) {
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn exists(ws: &dyn Workspace, path: &str) -> bool {
    ws.metadata(path).is_ok()
}

fn is_conflict(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::AlreadyExists | io::ErrorKind::DirectoryNotEmpty
    )
}

/// The root's canonical workspace path, when the registry covers it.
fn resolve_authorized_root(
    ws: &dyn Workspace,
    root: &str,
    registry: &WorkspaceRegistry,
) -> Result<String, String> {
    let canonical = registry
        .canonicalize_in(ws, root)
        .map_err(|error| format!("workspace root is not accessible: {error}"))?;
    if registry.covers(ws.env(), &canonical) {
        Ok(canonical)
    } else {
        Err(format!("workspace root is not authorized: {canonical}"))
    }
}

/// The entry itself may be a symlink or not exist yet; its parent is what
/// must resolve inside the root.
fn authorize_mutation_entry(ws: &dyn Workspace, path: &str, root: &str) -> Result<(), String> {
    let parent = Path::new(path)
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or_else(|| format!("path has no parent: {path}"))?;
    let canonical = ws
        .canonicalize(&parent.to_string_lossy())
        .map_err(|error| format!("path parent is not accessible: {error}"))?;
    if within(ws.env(), &canonical, root) {
        Ok(())
    } else {
        Err(format!("path is outside the authorized workspace: {path}"))
    }
}

fn move_conflict(metadata: &Meta, replaceable: bool) -> FsMoveResult {
    FsMoveResult::Conflict {
        replaceable,
        token: metadata.token.clone(),
    }
}

pub(crate) fn remove_path(ws: &dyn Workspace, path: &str) -> io::Result<()> {
    // The entry itself, never a symlink's target: deleting a link to a
    // directory removes the link only.
    if ws.symlink_metadata(path)?.kind == EntryKind::Dir {
        ws.remove_dir_all(path)
    } else {
        ws.remove_file(path)
    }
}

/// Creates a new empty file. Fails if the file already exists.
#[tauri::command]
pub async fn fs_create_file(
    index: State<'_, FileIndexState>,
    path: String,
    workspace: Option<WorkspaceEnv>,
) -> Result<(), String> {
    let index = index.inner().clone();
    blocking(move || {
        let workspace = WorkspaceEnv::from_option(workspace);
        create_file(&*backend_for(&workspace), &path)?;
        note_changed(&index, &resolve_path(&path, &workspace));
        Ok(())
    })
    .await
}

pub(crate) fn create_file(ws: &dyn Workspace, path: &str) -> Result<(), String> {
    if exists(ws, path) {
        return Err(format!("already exists: {path}"));
    }
    ws.create_file(path).map_err(|e| {
        log::debug!("fs_create_file({path}) failed: {e}");
        e.to_string()
    })
}

/// Creates a new directory. Fails if the directory already exists.
/// Parents are created as needed — matches the common "new folder" UX
/// where typing "a/b/c" creates the full chain.
#[tauri::command]
pub async fn fs_create_dir(
    index: State<'_, FileIndexState>,
    path: String,
    workspace: Option<WorkspaceEnv>,
) -> Result<(), String> {
    let index = index.inner().clone();
    blocking(move || {
        let workspace = WorkspaceEnv::from_option(workspace);
        create_dir(&*backend_for(&workspace), &path)?;
        note_changed(&index, &resolve_path(&path, &workspace));
        Ok(())
    })
    .await
}

pub(crate) fn create_dir(ws: &dyn Workspace, path: &str) -> Result<(), String> {
    if exists(ws, path) {
        return Err(format!("already exists: {path}"));
    }
    ws.create_dir_all(path).map_err(|e| {
        log::debug!("fs_create_dir({path}) failed: {e}");
        e.to_string()
    })
}

/// Renames (or moves) a path. Refuses to overwrite an existing target.
#[tauri::command]
pub async fn fs_rename(
    index: State<'_, FileIndexState>,
    from: String,
    to: String,
    workspace: Option<WorkspaceEnv>,
) -> Result<(), String> {
    let index = index.inner().clone();
    blocking(move || {
        let workspace = WorkspaceEnv::from_option(workspace);
        rename(&*backend_for(&workspace), &from, &to)?;
        note_changed(&index, &resolve_path(&from, &workspace));
        note_changed(&index, &resolve_path(&to, &workspace));
        Ok(())
    })
    .await
}

pub(crate) fn rename(ws: &dyn Workspace, from: &str, to: &str) -> Result<(), String> {
    if !exists(ws, from) {
        return Err(format!("not found: {from}"));
    }
    if exists(ws, to) {
        return Err(format!("already exists: {to}"));
    }
    // The backend's rename never replaces, so a target created after the
    // check above is refused rather than overwritten.
    ws.rename(from, to).map_err(|e| {
        log::debug!("fs_rename({from} -> {to}) failed: {e}");
        e.to_string()
    })
}

/// Moves a path without clobbering unless replacement was explicitly approved.
#[tauri::command]
pub async fn fs_move(
    app: AppHandle,
    from: String,
    to: String,
    root: String,
    expected_conflict: Option<String>,
    workspace: Option<WorkspaceEnv>,
) -> Result<FsMoveResult, String> {
    blocking(move || {
        let workspace = WorkspaceEnv::from_option(workspace);
        let ws = backend_for(&workspace);
        let root = resolve_authorized_root(&*ws, &root, &app.state::<WorkspaceRegistry>())?;
        let moved = fs_move_impl(&*ws, &from, &to, &root, expected_conflict.as_deref())?;
        if moved == FsMoveResult::Moved {
            let index = app.state::<FileIndexState>();
            note_changed(&index, &resolve_path(&from, &workspace));
            note_changed(&index, &resolve_path(&to, &workspace));
        }
        Ok(moved)
    })
    .await
}

pub(crate) fn fs_move_impl(
    ws: &dyn Workspace,
    from: &str,
    to: &str,
    root: &str,
    expected_conflict: Option<&str>,
) -> Result<FsMoveResult, String> {
    authorize_mutation_entry(ws, from, root)?;
    authorize_mutation_entry(ws, to, root)?;
    if same_spelling(ws.env(), from, to) {
        return Ok(FsMoveResult::Moved);
    }
    let from_meta = metadata_if_exists(ws, from)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("not found: {from}"))?;
    let to_meta = metadata_if_exists(ws, to).map_err(|error| error.to_string())?;
    let replaceable =
        |to_meta: &Meta| from_meta.kind != EntryKind::Dir && to_meta.kind != EntryKind::Dir;

    if let Some(to_meta) = to_meta {
        let replaceable = replaceable(&to_meta);
        if !replaceable || expected_conflict != Some(to_meta.token.as_str()) {
            return Ok(move_conflict(&to_meta, replaceable));
        }
        ws.replace_file(from, to).map_err(|error| {
            log::warn!("fs_move replace({from} -> {to}) failed: {error}");
            error.to_string()
        })?;
        return Ok(FsMoveResult::Moved);
    }

    match ws.rename(from, to) {
        Ok(()) => Ok(FsMoveResult::Moved),
        Err(error) if is_conflict(&error) => {
            let to_meta = metadata_if_exists(ws, to)
                .map_err(|metadata_error| metadata_error.to_string())?
                .ok_or_else(|| "destination changed while moving".to_string())?;
            Ok(move_conflict(&to_meta, replaceable(&to_meta)))
        }
        Err(error) => {
            log::warn!("fs_move({from} -> {to}) failed: {error}");
            Err(error.to_string())
        }
    }
}

/// Deletes a file or directory (recursively for dirs). Callers are
/// responsible for confirming destructive operations with the user.
#[tauri::command]
pub async fn fs_delete(
    index: State<'_, FileIndexState>,
    path: String,
    workspace: Option<WorkspaceEnv>,
) -> Result<(), String> {
    let index = index.inner().clone();
    blocking(move || {
        let workspace = WorkspaceEnv::from_option(workspace);
        delete_path(&*backend_for(&workspace), &path)?;
        note_changed(&index, &resolve_path(&path, &workspace));
        Ok(())
    })
    .await
}

fn delete_path(ws: &dyn Workspace, path: &str) -> Result<(), String> {
    remove_path(ws, path).map_err(|e| {
        log::warn!("fs_delete({path}) failed: {e}");
        e.to_string()
    })
}

#[tauri::command]
pub async fn fs_delete_batch(
    app: AppHandle,
    paths: Vec<String>,
    root: String,
    workspace: Option<WorkspaceEnv>,
) -> FsDeleteBatchResult {
    let count = paths.len();
    tauri::async_runtime::spawn_blocking(move || {
        let workspace = WorkspaceEnv::from_option(workspace);
        let ws = backend_for(&workspace);
        let Ok(root) = resolve_authorized_root(&*ws, &root, &app.state::<WorkspaceRegistry>())
        else {
            return FsDeleteBatchResult {
                deleted: Vec::new(),
                failed: paths.len(),
            };
        };
        let result = fs_delete_batch_impl(&*ws, paths, &root);
        let index = app.state::<FileIndexState>();
        for deleted in &result.deleted {
            note_changed(&index, &resolve_path(deleted, &workspace));
        }
        result
    })
    .await
    .unwrap_or(FsDeleteBatchResult {
        deleted: Vec::new(),
        failed: count,
    })
}

fn fs_delete_batch_impl(ws: &dyn Workspace, paths: Vec<String>, root: &str) -> FsDeleteBatchResult {
    let mut deleted = Vec::with_capacity(paths.len());
    let mut failed = 0;
    for path in paths {
        let result = authorize_mutation_entry(ws, &path, root)
            .and_then(|()| remove_path(ws, &path).map_err(|error| error.to_string()));
        match result {
            Ok(()) => deleted.push(path),
            Err(error) => {
                failed += 1;
                log::warn!("fs_delete_batch({path}) failed: {error}");
            }
        }
    }
    FsDeleteBatchResult { deleted, failed }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn local() -> Arc<dyn Workspace> {
        backend_for(&WorkspaceEnv::Local)
    }

    fn at(p: &Path) -> String {
        p.to_string_lossy().into_owned()
    }

    fn s(p: std::path::PathBuf) -> String {
        p.to_string_lossy().into_owned()
    }

    fn move_path(
        root: &Path,
        from: &Path,
        to: &Path,
        expected_conflict: Option<&str>,
    ) -> Result<FsMoveResult, String> {
        let root = local().canonicalize(&at(root)).unwrap();
        fs_move_impl(
            &*local(),
            &s(from.to_path_buf()),
            &s(to.to_path_buf()),
            &root,
            expected_conflict,
        )
    }

    fn conflict(result: FsMoveResult) -> (bool, String) {
        match result {
            FsMoveResult::Conflict { replaceable, token } => (replaceable, token),
            FsMoveResult::Moved => panic!("expected conflict"),
        }
    }

    #[test]
    fn create_file_makes_empty_and_refuses_to_clobber() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("new.txt");
        create_file(&*local(), &at(&f)).expect("create");
        assert!(f.exists());
        assert_eq!(std::fs::read(&f).unwrap(), b"");

        // A second create must error, not truncate existing content.
        std::fs::write(&f, b"data").unwrap();
        let err = create_file(&*local(), &at(&f)).unwrap_err();
        assert!(err.contains("already exists"), "got: {err}");
        assert_eq!(std::fs::read(&f).unwrap(), b"data");
    }

    #[test]
    fn create_dir_builds_nested_chain_and_refuses_existing() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("a/b/c");
        create_dir(&*local(), &at(&nested)).expect("create dir");
        assert!(nested.is_dir());
        let err = create_dir(&*local(), &at(&nested)).unwrap_err();
        assert!(err.contains("already exists"), "got: {err}");
    }

    #[test]
    fn rename_moves_and_never_overwrites() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("a.txt");
        let to = dir.path().join("b.txt");
        std::fs::write(&from, b"payload").unwrap();

        rename(&*local(), &at(&from), &at(&to)).expect("rename");
        assert!(!from.exists());
        assert_eq!(std::fs::read(&to).unwrap(), b"payload");

        // Missing source is reported, not silently ignored.
        let err = rename(&*local(), &at(&from), &at(&dir.path().join("c.txt"))).unwrap_err();
        assert!(err.contains("not found"), "got: {err}");

        // Refusing to overwrite an existing target is the data-loss guard.
        let occupied = dir.path().join("keep.txt");
        std::fs::write(&occupied, b"keep").unwrap();
        let err = rename(&*local(), &at(&to), &at(&occupied)).unwrap_err();
        assert!(err.contains("already exists"), "got: {err}");
        assert_eq!(std::fs::read(&occupied).unwrap(), b"keep");
        assert!(to.exists());
    }

    #[test]
    fn move_reports_conflict_without_changing_either_file() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("from.txt");
        let to = dir.path().join("to.txt");
        std::fs::write(&from, b"from").unwrap();
        std::fs::write(&to, b"to").unwrap();

        let result = move_path(dir.path(), &from, &to, None).unwrap();

        assert!(conflict(result).0);
        assert_eq!(std::fs::read(from).unwrap(), b"from");
        assert_eq!(std::fs::read(to).unwrap(), b"to");
    }

    #[cfg(unix)]
    #[test]
    fn move_treats_a_broken_destination_symlink_as_a_conflict() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("from.txt");
        let to = dir.path().join("to.txt");
        std::fs::write(&from, b"from").unwrap();
        std::os::unix::fs::symlink(dir.path().join("missing"), &to).unwrap();

        let result = move_path(dir.path(), &from, &to, None).unwrap();

        assert!(conflict(result).0);
        assert_eq!(std::fs::read(from).unwrap(), b"from");
        assert!(std::fs::symlink_metadata(to)
            .unwrap()
            .file_type()
            .is_symlink());
    }

    #[test]
    fn move_result_serializes_the_frontend_contract() {
        assert_eq!(
            serde_json::to_value(FsMoveResult::Conflict {
                replaceable: false,
                token: "opaque".into()
            })
            .unwrap(),
            serde_json::json!({
                "status": "conflict",
                "replaceable": false,
                "token": "opaque"
            })
        );
        assert_eq!(
            serde_json::to_value(FsMoveResult::Moved).unwrap(),
            serde_json::json!({ "status": "moved" })
        );
    }

    #[test]
    fn move_replaces_a_file_after_explicit_approval() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("from.txt");
        let to = dir.path().join("to.txt");
        std::fs::write(&from, b"new").unwrap();
        std::fs::write(&to, b"old").unwrap();

        let (_, token) = conflict(move_path(dir.path(), &from, &to, None).unwrap());
        let result = move_path(dir.path(), &from, &to, Some(&token)).unwrap();

        assert_eq!(result, FsMoveResult::Moved);
        assert!(!from.exists());
        assert_eq!(std::fs::read(to).unwrap(), b"new");
    }

    #[test]
    fn move_does_not_replace_a_conflict_that_changed_after_approval() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("from.txt");
        let to = dir.path().join("to.txt");
        std::fs::write(&from, b"new").unwrap();
        std::fs::write(&to, b"old").unwrap();
        let (_, token) = conflict(move_path(dir.path(), &from, &to, None).unwrap());
        std::fs::write(&to, b"changed after prompt").unwrap();

        let result = move_path(dir.path(), &from, &to, Some(&token)).unwrap();

        assert!(conflict(result).0);
        assert_eq!(std::fs::read(from).unwrap(), b"new");
        assert_eq!(std::fs::read(to).unwrap(), b"changed after prompt");
    }

    #[test]
    fn move_rejects_paths_outside_the_authorized_workspace() {
        let allowed = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let from = outside.path().join("from.txt");
        let to = allowed.path().join("to.txt");
        std::fs::write(&from, b"keep").unwrap();
        let root = local().canonicalize(&at(allowed.path())).unwrap();

        let error =
            fs_move_impl(&*local(), &s(from.clone()), &s(to), &root, None).unwrap_err();

        assert!(error.contains("outside the authorized workspace"));
        assert_eq!(std::fs::read(from).unwrap(), b"keep");
    }

    #[test]
    fn move_refuses_to_replace_a_directory() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("from");
        let to = dir.path().join("to");
        std::fs::create_dir(&from).unwrap();
        std::fs::create_dir(&to).unwrap();
        std::fs::write(from.join("new.txt"), b"new").unwrap();
        std::fs::write(to.join("old.txt"), b"old").unwrap();

        let result = move_path(dir.path(), &from, &to, None).unwrap();

        assert!(!conflict(result).0);
        assert_eq!(std::fs::read(from.join("new.txt")).unwrap(), b"new");
        assert_eq!(std::fs::read(to.join("old.txt")).unwrap(), b"old");
    }

    #[test]
    fn delete_removes_file_then_dir_recursively() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("x.txt");
        std::fs::write(&f, b"x").unwrap();
        delete_path(&*local(), &at(&f)).expect("delete file");
        assert!(!f.exists());

        let sub = dir.path().join("sub");
        std::fs::create_dir_all(sub.join("inner")).unwrap();
        std::fs::write(sub.join("inner/y.txt"), b"y").unwrap();
        delete_path(&*local(), &at(&sub)).expect("delete dir");
        assert!(!sub.exists());

        let err = delete_path(&*local(), &at(&dir.path().join("missing"))).unwrap_err();
        assert!(!err.is_empty());
    }

    #[test]
    fn batch_delete_reports_each_success_and_failure() {
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("first.txt");
        let second = dir.path().join("second.txt");
        let missing = dir.path().join("missing.txt");
        std::fs::write(&first, b"first").unwrap();
        std::fs::write(&second, b"second").unwrap();
        let root = local().canonicalize(&at(dir.path())).unwrap();
        let paths = vec![s(first.clone()), s(missing), s(second.clone())];

        let result = fs_delete_batch_impl(&*local(), paths.clone(), &root);

        assert_eq!(result.deleted, vec![paths[0].clone(), paths[2].clone()]);
        assert_eq!(result.failed, 1);
        assert!(!first.exists());
        assert!(!second.exists());
    }

    // Deleting a symlink that points at a directory must remove only the link,
    // never recurse through it and wipe the target's contents.
    #[cfg(unix)]
    #[test]
    fn delete_does_not_follow_symlink_into_target() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real");
        std::fs::create_dir(&real).unwrap();
        std::fs::write(real.join("keep.txt"), b"keep").unwrap();

        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();

        delete_path(&*local(), &at(&link)).expect("delete symlink");
        assert!(!link.exists(), "symlink itself should be gone");
        assert!(real.is_dir(), "target dir must survive");
        assert_eq!(std::fs::read(real.join("keep.txt")).unwrap(), b"keep");
    }
}
