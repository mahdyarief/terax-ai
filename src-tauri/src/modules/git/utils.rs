use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use std::sync::Arc;

use crate::modules::git::errors::{GitError, Result};
use crate::modules::workspace::{backend_for, resolve_path, within, WorkspaceEnv, WorkspaceRegistry};
use terax_workspace_core::paths::join;
use terax_workspace_core::workspace::Workspace;

#[derive(Clone, Debug)]
pub struct ResolvedGitDirectory {
    pub workspace: WorkspaceEnv,
    /// The path git is run in (its `-C` / working directory).
    pub git_path: String,
    /// The canonical workspace path, which file access and authorization use.
    pub ws_path: String,
    /// The canonical host path. Being replaced by `ws_path` module by module
    /// (phase 1, task 10).
    pub local_path: PathBuf,
}

impl ResolvedGitDirectory {
    /// The backend that reaches this repository's files.
    pub fn backend(&self) -> Arc<dyn Workspace> {
        backend_for(&self.workspace)
    }
}

/// `path` relative to `root`, `/`-separated, when it is `root` or lies beneath
/// it (both canonical workspace paths).
pub fn relative_to(root: &str, path: &str) -> Option<String> {
    let root = root.trim_end_matches('/');
    if path.trim_end_matches('/') == root {
        return Some(String::new());
    }
    path.strip_prefix(root)
        .and_then(|rest| rest.strip_prefix('/'))
        .map(str::to_string)
}

pub fn split_upstream(upstream: &str) -> (Option<String>, Option<String>) {
    match upstream.split_once('/') {
        Some((remote, branch)) => (Some(remote.to_string()), Some(branch.to_string())),
        None => (None, Some(upstream.to_string())),
    }
}

pub fn display_path(path: &Path) -> String {
    crate::modules::fs::to_canon(path)
}

/// Removes a Windows verbatim prefix and nothing else (separators are kept):
/// `\\?\C:\x` -> `C:\x`, `\\?\UNC\server\share\x` -> `\\server\share\x`. Git
/// cannot take a `\\?\` path where it expects a location (a clone URL, a
/// remote, a worktree), so every canonicalized path that is handed to git goes
/// through this or through `display_path`. Pure, so it is tested on any host.
pub fn strip_verbatim_prefix(path: &str) -> String {
    if let Some(rest) = path.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = path.strip_prefix(r"\\?\") {
        rest.to_string()
    } else {
        path.to_string()
    }
}

/// A path as git should receive it: on Windows without a verbatim prefix and
/// with forward slashes (the form `display_path` gives and git prints); elsewhere
/// the exact bytes, so a non-UTF-8 name survives.
pub fn git_arg(path: &Path) -> std::ffi::OsString {
    #[cfg(windows)]
    {
        std::ffi::OsString::from(display_path(path))
    }
    #[cfg(not(windows))]
    {
        path.as_os_str().to_os_string()
    }
}

/// `path` without its Windows verbatim prefix; unchanged elsewhere, where a
/// backslash is an ordinary filename character.
pub fn plain_path(path: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        PathBuf::from(strip_verbatim_prefix(&path.to_string_lossy()))
    }
    #[cfg(not(windows))]
    {
        path.to_path_buf()
    }
}

fn normalize_git_path(path: &str) -> String {
    path.replace('\\', "/")
}

pub fn canonical_dir(
    registry: &WorkspaceRegistry,
    path: &str,
    workspace: &WorkspaceEnv,
) -> Result<ResolvedGitDirectory> {
    let candidate = resolve_path(path, workspace);
    if !candidate.is_dir() {
        return Err(GitError::NotADirectory(path.to_string()));
    }
    let local_path = registry
        .canonicalize_cached(&candidate)
        .map_err(GitError::Io)?;
    let ws_path = registry
        .canonicalize_in(&*backend_for(workspace), path)
        .map_err(GitError::Io)?;
    let git_path = if workspace.is_wsl() {
        normalize_git_path(path)
    } else {
        display_path(&local_path)
    };
    Ok(ResolvedGitDirectory {
        workspace: workspace.clone(),
        git_path,
        ws_path,
        local_path,
    })
}

pub fn authorized_repo_root(
    registry: &WorkspaceRegistry,
    path: &str,
    workspace: &WorkspaceEnv,
) -> Result<ResolvedGitDirectory> {
    let canonical = canonical_dir(registry, path, workspace)?;
    if !registry.covers(&canonical.workspace, &canonical.ws_path) {
        return Err(GitError::PathOutsideWorkspace(PathBuf::from(&canonical.ws_path)));
    }
    Ok(canonical)
}

/// The canonical workspace path of `rel` under `repo_root` (itself canonical),
/// refused when it resolves outside the repository. A path that does not
/// exist (staging a removal) is validated through its nearest existing
/// ancestor.
pub fn resolve_within_repo(ws: &dyn Workspace, repo_root: &str, rel: &str) -> Result<String> {
    // A rooted `rel` names a place outside the repository; joining it as text
    // would quietly read it as a path inside instead.
    if !is_safe_pathspec(rel) || rel.starts_with(['/', '\\']) {
        return Err(GitError::InvalidPath(rel.into()));
    }
    let joined = join(repo_root, rel);
    match ws.canonicalize(&joined) {
        Ok(canonical) => {
            if !within(ws.env(), &canonical, repo_root) {
                return Err(GitError::PathOutsideWorkspace(PathBuf::from(canonical)));
            }
            Ok(canonical)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            resolve_deleted_within_repo(ws, repo_root, &joined, rel)
        }
        Err(e) => Err(GitError::Io(e)),
    }
}

pub fn is_safe_pathspec(rel: &str) -> bool {
    if rel.is_empty() || rel.contains(':') || rel.contains('\0') {
        return false;
    }
    if rel.chars().any(|c| (c as u32) < 0x20) {
        return false;
    }
    // Reject `.`/`..` so the deleted-path branch can't be used to escape the repo.
    !rel.split(['/', '\\']).any(|c| c == "." || c == "..")
}

fn resolve_deleted_within_repo(
    ws: &dyn Workspace,
    repo_root: &str,
    joined: &str,
    rel: &str,
) -> Result<String> {
    let mut tail: Vec<&OsStr> = Vec::new();
    let mut cursor = Path::new(joined);
    loop {
        let name = cursor
            .file_name()
            .ok_or_else(|| GitError::InvalidPath(rel.into()))?;
        let parent = cursor
            .parent()
            .ok_or_else(|| GitError::InvalidPath(rel.into()))?;
        tail.push(name);
        match ws.canonicalize(&parent.to_string_lossy()) {
            Ok(canonical_parent) => {
                if !within(ws.env(), &canonical_parent, repo_root) {
                    return Err(GitError::PathOutsideWorkspace(PathBuf::from(canonical_parent)));
                }
                let mut resolved = canonical_parent;
                for component in tail.iter().rev() {
                    resolved = join(&resolved, &component.to_string_lossy());
                }
                return Ok(resolved);
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                cursor = parent;
            }
            Err(e) => return Err(GitError::Io(e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local() -> Arc<dyn Workspace> {
        backend_for(&WorkspaceEnv::Local)
    }

    fn tmp_root() -> String {
        local()
            .canonicalize(&std::env::temp_dir().to_string_lossy())
            .unwrap()
    }

    #[test]
    fn relative_paths_are_taken_by_component() {
        assert_eq!(relative_to("/r", "/r/a/b").as_deref(), Some("a/b"));
        assert_eq!(relative_to("/r", "/r").as_deref(), Some(""));
        assert_eq!(relative_to("/r/", "/r/a").as_deref(), Some("a"));
        assert_eq!(relative_to("C:/", "C:/a").as_deref(), Some("a"));
        assert_eq!(relative_to("/r", "/rx/a"), None);
        assert_eq!(relative_to("/r/a", "/r"), None);
    }

    #[test]
    fn verbatim_drive_paths_lose_only_their_prefix() {
        assert_eq!(
            strip_verbatim_prefix(r"\\?\C:\Users\runner\Temp\.tmpAbC"),
            r"C:\Users\runner\Temp\.tmpAbC"
        );
        assert_eq!(strip_verbatim_prefix(r"\\?\D:\"), r"D:\");
    }

    #[test]
    fn verbatim_unc_paths_become_ordinary_unc_paths() {
        assert_eq!(
            strip_verbatim_prefix(r"\\?\UNC\server\share\dir\file.txt"),
            r"\\server\share\dir\file.txt"
        );
        assert_eq!(
            strip_verbatim_prefix(r"\\?\UNC\wsl.localhost\Ubuntu\home"),
            r"\\wsl.localhost\Ubuntu\home"
        );
    }

    #[test]
    fn ordinary_paths_are_left_alone() {
        for path in [
            r"C:\Users\x",
            "C:/Users/x",
            r"\\server\share\x",
            "/home/user/repo",
            "relative/dir",
            r"\\.\COM1",
            "",
        ] {
            assert_eq!(strip_verbatim_prefix(path), path);
        }
        // Not a prefix when it is not at the start.
        assert_eq!(strip_verbatim_prefix(r"x\\?\C:\y"), r"x\\?\C:\y");
    }

    #[test]
    fn git_arg_is_the_form_git_accepts_for_a_location() {
        let verbatim = git_arg(Path::new(r"\\?\C:\Users\runner\Temp\wt"));
        let unc = git_arg(Path::new(r"\\?\UNC\server\share\wt"));
        if cfg!(windows) {
            assert_eq!(
                verbatim,
                std::ffi::OsString::from("C:/Users/runner/Temp/wt")
            );
            assert_eq!(unc, std::ffi::OsString::from("//server/share/wt"));
        } else {
            // Elsewhere the exact bytes, so a backslash or non-UTF-8 name survives.
            assert_eq!(
                verbatim,
                std::ffi::OsString::from(r"\\?\C:\Users\runner\Temp\wt")
            );
        }
        assert_eq!(
            git_arg(Path::new("/home/me/my repo/wt")),
            std::ffi::OsString::from("/home/me/my repo/wt")
        );
    }

    #[test]
    fn plain_path_only_rewrites_on_windows() {
        let verbatim = Path::new(r"\\?\C:\a\b");
        if cfg!(windows) {
            assert_eq!(plain_path(verbatim), PathBuf::from(r"C:\a\b"));
        } else {
            assert_eq!(plain_path(verbatim), verbatim);
        }
        assert_eq!(plain_path(Path::new("/tmp/x")), PathBuf::from("/tmp/x"));
    }

    #[test]
    fn split_upstream_separates_remote_and_branch() {
        assert_eq!(
            split_upstream("origin/main"),
            (Some("origin".to_string()), Some("main".to_string()))
        );
    }

    #[test]
    fn split_upstream_splits_on_the_first_slash_only() {
        assert_eq!(
            split_upstream("origin/feature/x"),
            (Some("origin".to_string()), Some("feature/x".to_string()))
        );
    }

    #[test]
    fn split_upstream_without_slash_yields_branch_only() {
        assert_eq!(split_upstream("main"), (None, Some("main".to_string())));
    }

    #[test]
    fn normalize_git_path_converts_backslashes_to_slashes() {
        assert_eq!(normalize_git_path("a\\b\\c"), "a/b/c");
        assert_eq!(normalize_git_path("a/b"), "a/b");
    }

    #[test]
    fn safe_pathspec_accepts_normal_paths() {
        assert!(is_safe_pathspec("src/main.rs"));
        assert!(is_safe_pathspec("a/b/c-d_e.txt"));
        assert!(is_safe_pathspec("folder with spaces/file.md"));
        assert!(is_safe_pathspec("file.with.dots"));
    }

    #[test]
    fn safe_pathspec_rejects_colon() {
        assert!(!is_safe_pathspec("evil:path"));
        assert!(!is_safe_pathspec(":head"));
        assert!(!is_safe_pathspec("a/b:c"));
    }

    #[test]
    fn safe_pathspec_rejects_nul_and_control() {
        assert!(!is_safe_pathspec("foo\0bar"));
        assert!(!is_safe_pathspec("foo\nbar"));
        assert!(!is_safe_pathspec("foo\rbar"));
        assert!(!is_safe_pathspec("foo\tbar"));
    }

    #[test]
    fn safe_pathspec_rejects_empty() {
        assert!(!is_safe_pathspec(""));
    }

    #[test]
    fn resolve_within_repo_rejects_colon_path() {
        let err = resolve_within_repo(&*local(), &tmp_root(), "evil:path");
        assert!(matches!(err, Err(GitError::InvalidPath(_))));
    }

    #[test]
    fn resolve_within_repo_rejects_nul_path() {
        let err = resolve_within_repo(&*local(), &tmp_root(), "evil\0path");
        assert!(matches!(err, Err(GitError::InvalidPath(_))));
    }

    #[test]
    fn safe_pathspec_rejects_dot_components() {
        assert!(!is_safe_pathspec("../escape"));
        assert!(!is_safe_pathspec("a/../b"));
        assert!(!is_safe_pathspec("./a"));
        assert!(!is_safe_pathspec("a/."));
        assert!(!is_safe_pathspec(".."));
    }

    #[test]
    fn resolve_within_repo_handles_deleted_directory() {
        let base = std::env::temp_dir().join("terax_git_deleted_dir_test");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("envs/__pycache__")).unwrap();
        let repo_root = local().canonicalize(&base.to_string_lossy()).unwrap();
        std::fs::remove_dir_all(base.join("envs")).unwrap();
        let resolved = resolve_within_repo(&*local(), &repo_root, "envs/__pycache__/g1.pyc")
            .expect("deleted path");
        assert_eq!(resolved, format!("{repo_root}/envs/__pycache__/g1.pyc"));
        let _ = std::fs::remove_dir_all(&base);
    }

    /// A link inside the repository that points out of it resolves outside, and
    /// is refused even though the text of the path stays inside.
    #[test]
    fn resolve_within_repo_rejects_a_link_out_of_the_repository() {
        let repo = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("secret.txt"), b"x").unwrap();
        let link = repo.path().join("escape");
        #[cfg(unix)]
        std::os::unix::fs::symlink(outside.path(), &link).unwrap();
        #[cfg(windows)]
        {
            // A junction needs no symlink privilege.
            let status = std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(&link)
                .arg(outside.path())
                .stdout(std::process::Stdio::null())
                .status()
                .unwrap();
            assert!(status.success(), "mklink /J failed");
        }
        let root = local().canonicalize(&repo.path().to_string_lossy()).unwrap();
        for rel in ["escape/secret.txt", "escape/missing/new.txt"] {
            let err = resolve_within_repo(&*local(), &root, rel);
            assert!(matches!(err, Err(GitError::PathOutsideWorkspace(_))), "{rel}: {err:?}");
        }
    }

    #[test]
    fn resolve_within_repo_rejects_rooted_paths() {
        for rooted in ["/etc/passwd", "\\windows\\system32"] {
            let err = resolve_within_repo(&*local(), &tmp_root(), rooted);
            assert!(matches!(err, Err(GitError::InvalidPath(_))), "{rooted}");
        }
    }

    #[test]
    fn resolve_within_repo_rejects_deleted_escape() {
        let err = resolve_within_repo(&*local(), &tmp_root(), "../outside.txt");
        assert!(matches!(err, Err(GitError::InvalidPath(_))));
    }
}
