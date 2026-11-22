use super::{index::FileIndexState, mutate::note_dir_changed, to_canon};
use crate::modules::workspace::{backend_for, resolve_path, WorkspaceEnv, WorkspaceRegistry};
use serde::Serialize;
use std::path::{Path, PathBuf};
use tauri::Manager;

const MAX_COPY_SOURCES: usize = 1024;

#[derive(Debug, Default, Serialize)]
pub struct CopyResult {
    pub created: Vec<String>,
    pub failed: Vec<CopyFailure>,
}

#[derive(Debug, Serialize)]
pub struct CopyFailure {
    pub source: String,
    pub message: String,
}

#[tauri::command]
pub async fn fs_copy(
    app: tauri::AppHandle,
    sources: Vec<String>,
    dest_dir: String,
    root: String,
    workspace: Option<WorkspaceEnv>,
    rename_on_conflict: Option<bool>,
) -> Result<CopyResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        copy_batch(
            sources,
            &dest_dir,
            &root,
            &WorkspaceEnv::from_option(workspace),
            &app.state::<WorkspaceRegistry>(),
            &app.state::<FileIndexState>(),
            rename_on_conflict.unwrap_or(false),
        )
    })
    .await
    .map_err(|error| error.to_string())?
}

fn copy_destination(
    dest_dir: &str,
    root: &str,
    workspace: &WorkspaceEnv,
    registry: &WorkspaceRegistry,
) -> Result<PathBuf, String> {
    let root = resolve_path(root, workspace);
    let dest = resolve_path(dest_dir, workspace);
    if !root.is_absolute() || !dest.is_absolute() {
        return Err("copy destination and workspace root must be absolute".into());
    }
    let root = std::fs::canonicalize(root).map_err(|error| error.to_string())?;
    let dest = std::fs::canonicalize(dest).map_err(|error| error.to_string())?;
    if !registry.is_authorized(&root) || !dest.starts_with(&root) {
        return Err("copy destination is outside the authorized workspace".into());
    }
    if !dest.is_dir() {
        return Err("copy destination is not a directory".into());
    }
    Ok(dest)
}

fn copy_batch(
    sources: Vec<String>,
    dest_dir: &str,
    root: &str,
    workspace: &WorkspaceEnv,
    registry: &WorkspaceRegistry,
    index: &FileIndexState,
    rename_on_conflict: bool,
) -> Result<CopyResult, String> {
    if sources.len() > MAX_COPY_SOURCES {
        return Err(format!("copy at most {MAX_COPY_SOURCES} items at a time"));
    }
    // Authorization still canonicalizes host paths against the registry; it
    // moves behind the backend with the registry itself (phase 1, task 11).
    let dest = copy_destination(dest_dir, root, workspace, registry)?;
    let ws = backend_for(workspace);
    let mut result = CopyResult::default();
    for source in sources {
        // Sources are always paths on this machine (OS clipboard, desktop drop).
        match ws.import(Path::new(&source), dest_dir, rename_on_conflict) {
            Ok(name) => result
                .created
                .push(to_canon(Path::new(dest_dir).join(name))),
            Err(error) => result.failed.push(CopyFailure {
                source,
                message: error.to_string(),
            }),
        }
    }
    if !result.created.is_empty() {
        note_dir_changed(index, &dest);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(path: impl AsRef<Path>) -> String {
        to_canon(path)
    }

    fn copy(sources: &[PathBuf], dest: &Path, rename: bool) -> CopyResult {
        let registry = WorkspaceRegistry::default();
        registry.authorize(dest).unwrap();
        copy_batch(
            sources.iter().map(s).collect(),
            &s(dest),
            &s(dest),
            &WorkspaceEnv::Local,
            &registry,
            &FileIndexState::default(),
            rename,
        )
        .unwrap()
    }

    #[test]
    fn copying_invalidates_search_without_a_directory_watcher() {
        let src = tempfile::tempdir().unwrap();
        let dest = tempfile::tempdir().unwrap();
        std::fs::write(src.path().join("new.txt"), b"new").unwrap();
        let registry = WorkspaceRegistry::default();
        registry.authorize(dest.path()).unwrap();
        let index = FileIndexState::default();
        let key = super::super::index::IndexKey {
            root: std::fs::canonicalize(dest.path()).unwrap(),
            show_hidden: false,
            max_depth: None,
        };
        assert!(index.get_or_build(&key).entries.is_empty());
        let result = copy_batch(
            vec![
                s(src.path().join("new.txt")),
                s(src.path().join("missing.txt")),
            ],
            &s(dest.path()),
            &s(dest.path()),
            &WorkspaceEnv::Local,
            &registry,
            &index,
            false,
        )
        .unwrap();
        assert_eq!(result.created.len(), 1);
        assert_eq!(result.failed.len(), 1);
        assert!(index
            .get_or_build(&key)
            .entries
            .iter()
            .any(|(name, _)| name == "new.txt"));
    }

    #[test]
    fn copies_files_and_directories_without_removing_sources() {
        let src = tempfile::tempdir().unwrap();
        let dest = tempfile::tempdir().unwrap();
        std::fs::write(src.path().join("a.txt"), b"payload").unwrap();
        std::fs::create_dir_all(src.path().join("d/inner")).unwrap();
        std::fs::write(src.path().join("d/inner/y.txt"), b"y").unwrap();
        let result = copy(
            &[src.path().join("a.txt"), src.path().join("d")],
            dest.path(),
            false,
        );
        assert!(result.failed.is_empty());
        assert_eq!(
            result.created,
            [s(dest.path().join("a.txt")), s(dest.path().join("d"))]
        );
        assert_eq!(
            std::fs::read(dest.path().join("a.txt")).unwrap(),
            b"payload"
        );
        assert_eq!(
            std::fs::read(dest.path().join("d/inner/y.txt")).unwrap(),
            b"y"
        );
        assert!(src.path().join("a.txt").exists());
    }

    #[test]
    fn conflicts_never_replace_files_or_directories() {
        let src = tempfile::tempdir().unwrap();
        let dest = tempfile::tempdir().unwrap();
        std::fs::write(src.path().join("a.txt"), b"new").unwrap();
        std::fs::write(dest.path().join("a.txt"), b"old").unwrap();
        std::fs::create_dir(dest.path().join("a copy.txt")).unwrap();
        let result = copy(&[src.path().join("a.txt")], dest.path(), false);
        assert!(result.created.is_empty());
        assert_eq!(result.failed.len(), 1);
        let result = copy(&[src.path().join("a.txt")], dest.path(), true);
        assert!(result.failed.is_empty());
        assert_eq!(result.created, [s(dest.path().join("a copy 2.txt"))]);
        assert_eq!(std::fs::read(dest.path().join("a.txt")).unwrap(), b"old");
        assert_eq!(
            std::fs::read(dest.path().join("a copy 2.txt")).unwrap(),
            b"new"
        );
        assert!(dest.path().join("a copy.txt").is_dir());
        assert_eq!(std::fs::read_dir(dest.path()).unwrap().count(), 3);
    }

    #[test]
    fn concurrent_publishes_choose_distinct_names_without_clobbering() {
        let src = tempfile::tempdir().unwrap();
        let dest = tempfile::tempdir().unwrap();
        std::fs::write(src.path().join("a.txt"), b"source").unwrap();
        let barrier = std::sync::Barrier::new(4);
        let results = std::thread::scope(|scope| {
            let threads: Vec<_> = (0..4)
                .map(|_| {
                    scope.spawn(|| {
                        barrier.wait();
                        copy(&[src.path().join("a.txt")], dest.path(), true)
                    })
                })
                .collect();
            threads
                .into_iter()
                .map(|thread| thread.join().unwrap())
                .collect::<Vec<_>>()
        });
        let mut paths = std::collections::HashSet::new();
        for result in results {
            assert!(result.failed.is_empty());
            assert_eq!(result.created.len(), 1);
            assert!(paths.insert(result.created[0].clone()));
            assert_eq!(std::fs::read(&result.created[0]).unwrap(), b"source");
        }
        assert_eq!(std::fs::read_dir(dest.path()).unwrap().count(), 4);
    }

    #[test]
    fn reports_partial_batch_success_and_continues_after_missing_sources() {
        let src = tempfile::tempdir().unwrap();
        let dest = tempfile::tempdir().unwrap();
        std::fs::write(src.path().join("a"), b"a").unwrap();
        std::fs::write(src.path().join("b"), b"b").unwrap();
        let result = copy(
            &[
                src.path().join("a"),
                src.path().join("missing"),
                src.path().join("b"),
            ],
            dest.path(),
            true,
        );
        assert_eq!(result.created.len(), 2);
        assert_eq!(result.failed.len(), 1);
        assert_eq!(result.failed[0].source, s(src.path().join("missing")));
        assert_eq!(std::fs::read_dir(dest.path()).unwrap().count(), 2);
    }

    #[test]
    fn rejects_self_copy_but_allows_sibling_duplicates() {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("d");
        std::fs::create_dir_all(folder.join("inner")).unwrap();
        for dest in [&folder, &folder.join("inner")] {
            let result = copy(std::slice::from_ref(&folder), dest, true);
            assert!(result.created.is_empty());
            assert!(result.failed[0].message.contains("into itself"));
        }
        let result = copy(&[folder], root.path(), true);
        assert!(result.failed.is_empty());
        assert!(root.path().join("d copy/inner").is_dir());
    }

    #[test]
    fn requires_authorized_absolute_destination_inside_captured_root() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let registry = WorkspaceRegistry::default();
        let ws = WorkspaceEnv::Local;
        assert!(copy_destination(&s(root.path()), &s(root.path()), &ws, &registry).is_err());
        registry.authorize(root.path()).unwrap();
        assert!(copy_destination(&s(outside.path()), &s(root.path()), &ws, &registry).is_err());
        assert!(copy_destination("relative", &s(root.path()), &ws, &registry).is_err());
        assert!(copy_destination(&s(root.path()), &s(root.path()), &ws, &registry).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn rejects_destination_symlink_escape_and_preserves_source_links() {
        use std::os::unix::fs::symlink;
        let src = tempfile::tempdir().unwrap();
        let dest = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        symlink(outside.path(), dest.path().join("escape")).unwrap();
        let registry = WorkspaceRegistry::default();
        registry.authorize(dest.path()).unwrap();
        assert!(copy_destination(
            &s(dest.path().join("escape")),
            &s(dest.path()),
            &WorkspaceEnv::Local,
            &registry
        )
        .is_err());

        std::fs::create_dir(src.path().join("d")).unwrap();
        symlink(".", src.path().join("d/cycle")).unwrap();
        symlink("missing", src.path().join("d/dangling")).unwrap();
        let result = copy(&[src.path().join("d")], dest.path(), true);
        assert!(result.failed.is_empty());
        assert_eq!(
            std::fs::read_link(dest.path().join("d/cycle")).unwrap(),
            Path::new(".")
        );
        assert_eq!(
            std::fs::read_link(dest.path().join("d/dangling")).unwrap(),
            Path::new("missing")
        );
    }

    #[cfg(unix)]
    #[test]
    fn does_not_follow_a_conflicting_destination_symlink() {
        let src = tempfile::tempdir().unwrap();
        let dest = tempfile::tempdir().unwrap();
        std::fs::write(src.path().join("a"), b"new").unwrap();
        std::fs::write(src.path().join("victim"), b"old").unwrap();
        std::os::unix::fs::symlink(src.path().join("victim"), dest.path().join("a")).unwrap();
        let result = copy(&[src.path().join("a")], dest.path(), true);
        assert!(result.failed.is_empty());
        assert_eq!(std::fs::read(src.path().join("victim")).unwrap(), b"old");
        assert_eq!(std::fs::read(dest.path().join("a copy")).unwrap(), b"new");
    }

    #[cfg(unix)]
    #[test]
    fn unsupported_nested_entries_leave_no_partial_directory() {
        let src = tempfile::tempdir().unwrap();
        let dest = tempfile::tempdir().unwrap();
        std::fs::create_dir(src.path().join("d")).unwrap();
        std::fs::write(src.path().join("d/a"), b"a").unwrap();
        let _socket = std::os::unix::net::UnixListener::bind(src.path().join("d/socket")).unwrap();
        let result = copy(&[src.path().join("d")], dest.path(), true);
        assert!(result.created.is_empty());
        assert_eq!(result.failed.len(), 1);
        assert_eq!(std::fs::read_dir(dest.path()).unwrap().count(), 0);
    }

    #[cfg(windows)]
    #[test]
    fn wsl_drvfs_results_keep_workspace_paths() {
        let src = tempfile::tempdir().unwrap();
        let dest = tempfile::tempdir().unwrap();
        std::fs::write(src.path().join("a"), b"a").unwrap();
        let host = s(std::fs::canonicalize(dest.path()).unwrap());
        let wsl = format!("/mnt/{}{}", host[..1].to_ascii_lowercase(), &host[2..]);
        let registry = WorkspaceRegistry::default();
        registry.authorize(dest.path()).unwrap();
        let result = copy_batch(
            vec![s(src.path().join("a"))],
            &wsl,
            &wsl,
            &WorkspaceEnv::Wsl {
                distro: "Ubuntu".into(),
            },
            &registry,
            &FileIndexState::default(),
            true,
        )
        .unwrap();
        assert!(result.failed.is_empty());
        assert_eq!(result.created, [format!("{wsl}/a")]);
    }
}
