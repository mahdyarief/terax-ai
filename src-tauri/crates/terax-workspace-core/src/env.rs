//! Where a workspace lives (this machine, a WSL distribution, or a server
//! reached over SSH) and how its paths map to paths this process can open.

use std::path::PathBuf;

use serde::Deserialize;

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum WorkspaceEnv {
    #[default]
    Local,
    Wsl {
        distro: String,
    },
    /// A server reached over SSH. Its paths are the server's: they never map
    /// to anything on this machine, and only `backend_for` reaches them.
    Ssh {
        host: SshHost,
    },
}

/// A host as `ssh` takes it: an alias from `~/.ssh/config` or
/// `user@host[:port]`. Checked when it is made (and when it is deserialized),
/// so it can never be read as an option or split into several arguments.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Deserialize)]
#[serde(try_from = "String")]
pub struct SshHost(String);

impl SshHost {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for SshHost {
    type Error = String;

    fn try_from(host: String) -> Result<Self, String> {
        if host.is_empty() || host.len() > 255 {
            return Err("an SSH host must be 1 to 255 bytes".into());
        }
        if host.starts_with('-') {
            return Err(format!("an SSH host cannot start with `-`: {host}"));
        }
        if host
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || matches!(c, '\'' | '"' | '`' | '$' | ';' | '|' | '&'))
        {
            return Err(format!("an SSH host cannot contain spaces, quotes or shell characters: {host:?}"));
        }
        Ok(Self(host))
    }
}

impl TryFrom<&str> for SshHost {
    type Error = String;

    fn try_from(host: &str) -> Result<Self, String> {
        Self::try_from(host.to_string())
    }
}

impl std::fmt::Display for SshHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl WorkspaceEnv {
    pub fn from_option(workspace: Option<Self>) -> Self {
        workspace.unwrap_or_default()
    }

    pub fn is_wsl(&self) -> bool {
        matches!(self, Self::Wsl { .. })
    }

    pub fn is_ssh(&self) -> bool {
        matches!(self, Self::Ssh { .. })
    }

    /// A workspace on this machine's own file system. Features that work on
    /// host paths only (checkpoints, task worktrees, creating and removing
    /// worktrees, the git state watcher) require it, so every other kind of
    /// workspace, today WSL and later a remote host, is refused by default.
    pub fn is_local(&self) -> bool {
        matches!(self, Self::Local)
    }
}

/// The host path for a workspace path. A path on an SSH server has none: it
/// maps to `UNREACHABLE`, which every file-system call and `current_dir`
/// refuses (`InvalidInput`), so a caller that forgets to check the env can
/// never open the same path on this machine.
#[cfg(windows)]
pub fn resolve_path(path: &str, workspace: &WorkspaceEnv) -> PathBuf {
    match workspace {
        WorkspaceEnv::Local => PathBuf::from(path),
        WorkspaceEnv::Wsl { distro } => wsl_path_to_host(distro, path),
        WorkspaceEnv::Ssh { .. } => PathBuf::from(UNREACHABLE),
    }
}

#[cfg(not(windows))]
pub fn resolve_path(path: &str, workspace: &WorkspaceEnv) -> PathBuf {
    match workspace {
        WorkspaceEnv::Ssh { .. } => PathBuf::from(UNREACHABLE),
        _ => PathBuf::from(path),
    }
}

/// The error for asking this machine to reach a remote workspace's path or
/// run a process in it.
pub fn not_on_this_machine() -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "this operation is not available for a workspace on an SSH host",
    )
}

/// Not a path any operating system accepts: it holds a NUL byte.
pub const UNREACHABLE: &str = "\0terax: a remote path has no path on this machine";

/// True for WSL distro names safe to splice into a UNC path. Real WSL distros
/// are alphanumeric with `.`, `_`, `-` separators (e.g. `Ubuntu-22.04`). Reject
/// anything that could traverse out of the `\\wsl.localhost\<distro>\` prefix
/// (`..`, `\`, `/`, `:`, `?`, `*`, control bytes) or empty names.
#[cfg(windows)]
fn is_safe_distro_name(name: &str) -> bool {
    if name.is_empty() || name.len() > 255 {
        return false;
    }
    if name == "." || name == ".." || name.starts_with('.') {
        return false;
    }
    name.chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | ' '))
        && !name.contains("..")
}

#[cfg(windows)]
pub fn validate_wsl_distro_name(distro: &str) -> Result<(), String> {
    if is_safe_distro_name(distro) {
        Ok(())
    } else {
        Err(format!("unsafe WSL distro name: {distro}"))
    }
}

#[cfg(windows)]
pub fn wsl_drvfs_to_windows(path: &str) -> Option<PathBuf> {
    let normalized = path.replace('\\', "/");
    let rest = normalized.strip_prefix("/mnt/")?;
    let mut parts = rest.splitn(2, '/');
    let drive = parts.next()?;
    if drive.len() != 1 {
        return None;
    }
    let drive = drive.chars().next()?;
    if !drive.is_ascii_alphabetic() {
        return None;
    }
    let suffix = parts.next().unwrap_or("").replace('/', "\\");
    let mut host = format!("{}:\\", drive.to_ascii_uppercase());
    if !suffix.is_empty() {
        host.push_str(&suffix);
    }
    Some(PathBuf::from(host))
}

#[cfg(windows)]
pub fn wsl_path_to_unc(distro: &str, path: &str) -> PathBuf {
    // Defense-in-depth: refuse to construct a UNC path with a distro name that
    // could escape the WSL share root via `..`, `\`, or other path metachars.
    // Returns a clearly-invalid path that downstream `is_dir()`/`metadata()`
    // checks will reject. The webview's distro list comes from `wsl.exe --list`
    // and is normally trustworthy, but a locally-registered malicious distro
    // can name itself with traversal characters; this filter blocks that.
    if !is_safe_distro_name(distro) {
        return PathBuf::from(r"\\wsl.localhost\__terax_invalid_distro__");
    }
    let normalized = path.replace('\\', "/");
    let trimmed = normalized.trim_start_matches('/');
    let primary = PathBuf::from(format!(
        r"\\wsl.localhost\{}\{}",
        distro,
        trimmed.replace('/', r"\")
    ));
    if primary.exists() {
        return primary;
    }
    PathBuf::from(format!(r"\\wsl$\{}\{}", distro, trimmed.replace('/', r"\")))
}

#[cfg(windows)]
pub fn wsl_path_to_host(distro: &str, path: &str) -> PathBuf {
    // `/mnt/<drive>` is drvfs-backed Windows storage. Accessing it through the
    // WSL UNC share can return "Access is denied" on Windows even though the
    // same path is readable inside WSL. Use the native drive path instead.
    wsl_drvfs_to_windows(path).unwrap_or_else(|| wsl_path_to_unc(distro, path))
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn distro_validator_accepts_real_names() {
        assert!(is_safe_distro_name("Ubuntu"));
        assert!(is_safe_distro_name("Ubuntu-22.04"));
        assert!(is_safe_distro_name("Debian"));
        assert!(is_safe_distro_name("Alpine_3.18"));
        assert!(is_safe_distro_name("openSUSE-Tumbleweed"));
    }

    #[test]
    fn distro_validator_rejects_path_traversal() {
        assert!(!is_safe_distro_name(".."));
        assert!(!is_safe_distro_name("..\\..\\Windows"));
        assert!(!is_safe_distro_name("../foo"));
        assert!(!is_safe_distro_name("foo/bar"));
        assert!(!is_safe_distro_name("foo\\bar"));
        assert!(!is_safe_distro_name("foo..bar"));
    }

    #[test]
    fn distro_validator_rejects_special_chars() {
        assert!(!is_safe_distro_name("foo:bar"));
        assert!(!is_safe_distro_name("foo?bar"));
        assert!(!is_safe_distro_name("foo*bar"));
        assert!(!is_safe_distro_name("foo\0bar"));
        assert!(!is_safe_distro_name(""));
        assert!(!is_safe_distro_name(".hidden"));
    }

    #[test]
    fn wsl_path_to_unc_blocks_traversal_distro() {
        // Malicious distro name must produce a path that is_dir() will reject,
        // never escape the WSL share root.
        let p = wsl_path_to_unc("..\\..\\..\\Windows", "/etc/passwd");
        let s = p.to_string_lossy();
        assert!(s.contains("__terax_invalid_distro__"), "got: {s}");
        assert!(!s.contains("\\..\\"), "got: {s}");
    }

    #[test]
    fn wsl_path_to_unc_accepts_valid_distro() {
        let p = wsl_path_to_unc("Ubuntu", "/etc/hosts");
        let s = p.to_string_lossy();
        assert!(!s.contains("__terax_invalid_distro__"), "got: {s}");
    }

    #[test]
    fn resolve_path_keeps_local_paths_unchanged() {
        let path = r"C:\Users\vinicios\repo";
        assert_eq!(
            resolve_path(path, &WorkspaceEnv::Local),
            PathBuf::from(path)
        );
    }

    #[test]
    fn resolve_path_maps_wsl_paths_to_host() {
        let workspace = WorkspaceEnv::Wsl {
            distro: "Ubuntu".into(),
        };
        assert_eq!(
            resolve_path("/home/vinicios/repo", &workspace),
            wsl_path_to_host("Ubuntu", "/home/vinicios/repo")
        );
    }

    #[test]
    fn wsl_drvfs_root_maps_to_windows_drive() {
        assert_eq!(wsl_drvfs_to_windows("/mnt/c"), Some(PathBuf::from(r"C:\")));
    }

    #[test]
    fn wsl_drvfs_child_maps_to_windows_drive() {
        assert_eq!(
            wsl_drvfs_to_windows("/mnt/d/Users/vinicios/repo"),
            Some(PathBuf::from(r"D:\Users\vinicios\repo"))
        );
    }

    #[test]
    fn wsl_drvfs_rejects_non_drive_mounts() {
        assert_eq!(wsl_drvfs_to_windows("/mnt/wsl"), None);
        assert_eq!(wsl_drvfs_to_windows("/home/vinicios"), None);
    }
}

#[cfg(test)]
mod wire_tests {
    use super::WorkspaceEnv;

    #[test]
    fn only_a_local_workspace_is_local() {
        assert!(WorkspaceEnv::Local.is_local());
        let wsl = WorkspaceEnv::Wsl {
            distro: "Ubuntu".into(),
        };
        assert!(!wsl.is_local());
        assert!(wsl.is_wsl() && !WorkspaceEnv::Local.is_wsl());
    }

    // The frontend sends these exact shapes (src/modules/workspace/env.ts).
    // Changing the serde attributes would break every command silently.
    #[test]
    fn deserializes_the_shapes_the_frontend_sends() {
        let local: WorkspaceEnv = serde_json::from_str(r#"{"kind":"local"}"#).unwrap();
        assert!(matches!(local, WorkspaceEnv::Local));

        let wsl: WorkspaceEnv =
            serde_json::from_str(r#"{"kind":"wsl","distro":"Ubuntu-22.04"}"#).unwrap();
        assert!(matches!(wsl, WorkspaceEnv::Wsl { ref distro } if distro == "Ubuntu-22.04"));
    }

    #[test]
    fn rejects_unknown_kinds_and_a_wsl_env_without_a_distro() {
        assert!(serde_json::from_str::<WorkspaceEnv>(r#"{"kind":"ssh"}"#).is_err());
        assert!(serde_json::from_str::<WorkspaceEnv>(r#"{"kind":"wsl"}"#).is_err());
    }

    #[test]
    fn a_missing_env_means_local() {
        assert!(matches!(WorkspaceEnv::from_option(None), WorkspaceEnv::Local));
        assert!(!WorkspaceEnv::Local.is_wsl());
    }
}
