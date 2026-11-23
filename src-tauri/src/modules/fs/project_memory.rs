use std::path::Path;
use tauri::Manager;

use crate::modules::workspace::{backend_for, WorkspaceEnv, WorkspaceRegistry};
use terax_workspace_core::confined::{Confine, ConfinedError, ConfinedRead};
use terax_workspace_core::paths::join;
use terax_workspace_core::workspace::Workspace;

const MAX_BYTES: u64 = 32 * 1024;
const ESCAPED: &str = "project memory escaped the project or changed after its safety check";
const INEXACT: &str = "project memory path cannot be represented losslessly";

#[tauri::command]
pub async fn fs_read_project_memory(
    root: String,
    checked_path: String,
    workspace: Option<WorkspaceEnv>,
    app: tauri::AppHandle,
) -> Result<Option<String>, String> {
    super::blocking(move || {
        let workspace = WorkspaceEnv::from_option(workspace);
        let ws = backend_for(&workspace);
        let root = ws.canonicalize(&root).map_err(|e| e.to_string())?;
        if !app.state::<WorkspaceRegistry>().covers(&workspace, &root) {
            return Err("project memory is outside the authorized workspace".into());
        }
        read_memory_in(&*ws, &root, &checked_path)
    })
    .await
}

/// `read_memory_in` for host paths in a local workspace (the tests).
#[cfg(test)]
fn read_memory(root: &Path, checked_path: &Path) -> Result<Option<String>, String> {
    if !lossless_paths(&[root, checked_path]) {
        return Err(INEXACT.into());
    }
    let ws = backend_for(&WorkspaceEnv::Local);
    read_memory_in(&*ws, &comparison_path(root), &checked_path.to_string_lossy())
}

/// `root/TERAX.md`, read only if the file actually opened is inside `root`
/// (a canonical workspace path), passes the secret-path policy, and is the
/// very file the frontend checked (`checked_path`).
pub(crate) fn read_memory_in(
    ws: &dyn Workspace,
    root: &str,
    checked_path: &str,
) -> Result<Option<String>, String> {
    let request = ConfinedRead {
        roots: vec![Confine {
            path: root.to_string(),
            subtree: true,
        }],
        secret_policy: true,
        offset: 0,
        limit: MAX_BYTES,
    };
    let read = ws
        .read_confined(&join(root, "TERAX.md"), &request)
        .map_err(|error| match error {
            ConfinedError::NotAFile => "project memory must be a regular file".to_string(),
            ConfinedError::Inexact => INEXACT.to_string(),
            ConfinedError::Outside => ESCAPED.to_string(),
            ConfinedError::Refused(why) => why.to_string(),
            ConfinedError::Io(error) => error.to_string(),
        })?;
    super::read_policy::deny_runtime_in(&read.real_path, ws.env())?;
    // The frontend's spelling, normalized the way real paths are; a
    // server's path is compared as the server spells it.
    let checked = if ws.env().is_ssh() {
        checked_path.to_string()
    } else {
        comparison_path(Path::new(checked_path))
    };
    if read.real_path != checked {
        return Err(ESCAPED.into());
    }
    let mut bytes = read.bytes;
    if bytes.contains(&0) {
        return Ok(None);
    }
    if bytes.len() > MAX_BYTES as usize {
        bytes.truncate(MAX_BYTES as usize);
        if let Err(error) = std::str::from_utf8(&bytes) {
            if error.error_len().is_none() {
                bytes.truncate(error.valid_up_to());
            }
        }
    }
    Ok(String::from_utf8(bytes).ok())
}

#[cfg(test)]
fn lossless_paths(paths: &[&Path]) -> bool {
    paths.iter().all(|path| path.to_str().is_some())
}

fn comparison_path(path: &Path) -> String {
    super::to_canon(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn case_distinct_roots_never_compare_equal() {
        let root = comparison_path(Path::new("//wsl.localhost/Ubuntu/home/me/repo"));
        let sibling = comparison_path(Path::new(
            "//wsl.localhost/Ubuntu/home/me/Repo/instructions.md",
        ));
        assert!(!Path::new(&sibling).starts_with(&root));
        assert_ne!(
            comparison_path(Path::new("C:/Repo/memory.md")),
            comparison_path(Path::new("C:/repo/memory.md"))
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn invalid_utf8_sibling_cannot_alias_an_authorized_root() {
        use std::os::unix::ffi::OsStrExt;
        let dir = tempfile::tempdir().unwrap();
        let parent = std::fs::canonicalize(dir.path()).unwrap();
        let root = parent.join("repo�");
        let outside = parent.join(std::ffi::OsStr::from_bytes(b"repo\xff"));
        std::fs::create_dir(&root).unwrap();
        std::fs::create_dir(&outside).unwrap();
        let target = outside.join("instructions.md");
        std::fs::write(&target, "OUTSIDE_FIXTURE").unwrap();
        std::os::unix::fs::symlink(&target, root.join("TERAX.md")).unwrap();
        let frontend_path = std::path::PathBuf::from(target.to_string_lossy().as_ref());
        assert!(read_memory(&root, &frontend_path).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn byte_distinct_paths_are_not_accepted_as_display_strings() {
        use std::os::unix::ffi::OsStrExt;
        let raw = Path::new(std::ffi::OsStr::from_bytes(b"/repo\xff/memory.md"));
        assert!(!lossless_paths(&[Path::new("/repo�"), raw]));
        assert!(lossless_paths(&[Path::new("/repo�/memory.md")]));
    }

    /// The Unix link tests, on Windows: `TERAX.md` as a file symlink (which
    /// needs Developer Mode or elevation; skipped without it).
    #[cfg(windows)]
    #[test]
    fn windows_links_cannot_escape_or_substitute_the_checked_memory() {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        let project = root.join("project");
        let outside = root.join("outside");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("instructions.md"), "OUTSIDE_FIXTURE").unwrap();
        std::fs::write(project.join(".env"), "SYNTHETIC_CREDENTIAL").unwrap();
        std::fs::write(project.join("other.md"), "OTHER").unwrap();
        let memory = project.join("TERAX.md");
        let link = |target: &Path| {
            let _ = std::fs::remove_file(&memory);
            std::os::windows::fs::symlink_file(target, &memory)
        };
        if let Err(error) = link(&outside.join("instructions.md")) {
            eprintln!("skipped: no symlink privilege ({error})");
            return;
        }
        let escaped = read_memory(&project, &outside.join("instructions.md"));
        assert!(escaped.is_err(), "a link out of the project");

        link(&project.join(".env")).unwrap();
        assert!(
            read_memory(&project, &project.join(".env")).is_err(),
            "a link to a secret, even when the checked path matches it"
        );

        link(&project.join("other.md")).unwrap();
        assert!(
            read_memory(&project, &memory).is_err(),
            "a target other than the file the frontend checked"
        );
        assert_eq!(
            read_memory(&project, &project.join("other.md")).unwrap().as_deref(),
            Some("OTHER"),
            "the checked target itself is read"
        );
    }

    #[test]
    fn ordinary_memory_is_bounded_utf8() {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        let path = root.join("TERAX.md");
        std::fs::write(&path, "project instructions").unwrap();
        assert_eq!(
            read_memory(&root, &path).unwrap().as_deref(),
            Some("project instructions")
        );
        std::fs::write(&path, "€".repeat(MAX_BYTES as usize)).unwrap();
        assert_eq!(read_memory(&root, &path).unwrap().unwrap().len(), 32766);
    }

    #[cfg(unix)]
    #[test]
    fn native_memory_reads_refuse_protected_targets_even_when_the_checked_path_matches() {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        for name in [".env", ".cache/terax/control.json"] {
            let target = root.join(name);
            std::fs::create_dir_all(target.parent().unwrap()).unwrap();
            std::fs::write(&target, "SYNTHETIC_CREDENTIAL").unwrap();
            let memory = root.join("TERAX.md");
            std::os::unix::fs::symlink(&target, &memory).unwrap();
            assert!(read_memory(&root, &target).is_err(), "{name}");
            std::fs::remove_file(&memory).unwrap();
        }
    }

    #[cfg(unix)]
    #[test]
    fn links_cannot_escape_or_substitute_a_previously_checked_target() {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        let outside = tempfile::NamedTempFile::new().unwrap();
        let memory = root.join("TERAX.md");
        std::os::unix::fs::symlink(outside.path(), &memory).unwrap();
        assert!(read_memory(&root, outside.path()).is_err());
        std::fs::remove_file(&memory).unwrap();
        let secret = root.join(".env");
        std::fs::write(&secret, "SYNTHETIC_SECRET").unwrap();
        std::os::unix::fs::symlink(&secret, &memory).unwrap();
        assert!(read_memory(&root, &memory).is_err());
        std::fs::remove_file(&memory).unwrap();
        let allowed = root.join("instructions.md");
        std::fs::write(&allowed, "allowed instructions").unwrap();
        std::os::unix::fs::symlink(&allowed, &memory).unwrap();
        assert_eq!(
            read_memory(&root, &allowed).unwrap().as_deref(),
            Some("allowed instructions")
        );
    }
}
