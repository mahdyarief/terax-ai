use std::path::Path;

use terax_workspace_core::env::{resolve_path, WorkspaceEnv};

/// The secret-path policy for a workspace path. On this machine (local,
/// WSL) by its host spelling, which also covers the app's own credential
/// directory. On a server by the server's spelling: its host spelling is a
/// placeholder no pattern matches, and this app's directory is not there.
pub(super) fn readable_in(path: &str, workspace: &WorkspaceEnv) -> Result<(), String> {
    if workspace.is_ssh() {
        return match terax_workspace_core::confined::refusal(path) {
            Some(why) => Err(why.into()),
            None => Ok(()),
        };
    }
    readable(&resolve_path(path, workspace))
}

/// `deny_runtime` for a workspace path; nothing to deny on a server.
pub(super) fn deny_runtime_in(path: &str, workspace: &WorkspaceEnv) -> Result<(), String> {
    if workspace.is_ssh() {
        return Ok(());
    }
    deny_runtime(&resolve_path(path, workspace))
}

/// The secret-path policy for a host path: the shared patterns (in the
/// workspace core, so a remote agent refuses the same names) plus this app's
/// own credential directory.
pub(super) fn readable(path: &Path) -> Result<(), String> {
    deny_runtime(path)?;
    let path = path.to_str().ok_or("refused non-UTF-8 AI path")?;
    match terax_workspace_core::confined::refusal(path) {
        Some(why) => Err(why.into()),
        None => Ok(()),
    }
}

/// Refuses this app's runtime directories (the control credentials of a
/// release and of a dev build), wherever the cache directory has been
/// relocated to. A host concern: a remote agent has its own.
pub(super) fn deny_runtime(path: &Path) -> Result<(), String> {
    if let Some(cache) = dirs::cache_dir() {
        for name in terax_control_protocol::RUNTIME_DIRS {
            deny_runtime_directory(path, &cache.join(name))?;
        }
    }
    Ok(())
}

fn inside_directory(path: &Path, directory: &Path) -> bool {
    #[cfg(windows)]
    {
        windows_inside_directory(path, directory)
    }
    #[cfg(not(windows))]
    {
        path.starts_with(directory)
    }
}

#[cfg(any(windows, test))]
fn windows_inside_directory(path: &Path, directory: &Path) -> bool {
    let (Some(path), Some(directory)) = (path.to_str(), directory.to_str()) else {
        return false;
    };
    Path::new(&super::strip_verbatim(path)).starts_with(super::strip_verbatim(directory))
}

fn deny_runtime_directory(path: &Path, directory: &Path) -> Result<(), String> {
    if inside_directory(path, directory)
        || std::fs::canonicalize(directory)
            .is_ok_and(|canonical| inside_directory(path, &canonical))
    {
        return Err("refused application credential directory".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relocated_runtime_credentials_are_protected() {
        let tmp = tempfile::tempdir().unwrap();
        let relocated = tmp.path().join("custom-cache").join("terax");
        std::fs::create_dir_all(&relocated).unwrap();
        let actual = std::fs::canonicalize(&relocated).unwrap();
        assert!(deny_runtime_directory(&actual.join("control.json"), &relocated).is_err());
        assert!(deny_runtime_directory(&actual.join("control.json.tmp-abc"), &relocated).is_err());
        assert!(deny_runtime_directory(&tmp.path().join("source.ts"), &relocated).is_ok());
        #[cfg(unix)]
        {
            let alias = tmp.path().join("cache-alias");
            std::os::unix::fs::symlink(&relocated, &alias).unwrap();
            assert!(deny_runtime_directory(&actual.join("control.json"), &alias).is_err());
        }
    }

    #[test]
    fn a_dev_builds_credentials_are_protected_as_well() {
        let cache = dirs::cache_dir().unwrap();
        for name in ["terax", "terax-dev"] {
            assert!(deny_runtime(&cache.join(name).join("control.json")).is_err());
        }
        assert!(deny_runtime(&cache.join("terax-devtools").join("a")).is_ok());
    }

    #[test]
    fn windows_runtime_paths_match_across_verbatim_prefixes() {
        for (path, directory) in [
            (r"C:\project\state\control.json", r"\\?\C:\project\state"),
            (r"\\?\C:\project\state\control.json", r"C:\project\state"),
            (
                r"\\server\share\state\control.json",
                r"\\?\UNC\server\share\state",
            ),
        ] {
            assert!(windows_inside_directory(
                Path::new(path),
                Path::new(directory)
            ));
        }
        assert!(!windows_inside_directory(
            Path::new("C:/project/state-other/file"),
            Path::new("C:/project/state")
        ));
        assert!(!windows_inside_directory(
            Path::new("//wsl.localhost/Ubuntu/home/me/State/file"),
            Path::new("//wsl.localhost/Ubuntu/home/me/state")
        ));
    }

    #[cfg(windows)]
    #[test]
    fn a_junction_relocated_cache_is_denied_after_opened_path_normalization() {
        use std::os::windows::process::CommandExt;
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp
            .path()
            .join("authorized-project")
            .join("application-state");
        let alias = tmp.path().join("relocated-cache").join("terax");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::create_dir_all(alias.parent().unwrap()).unwrap();
        std::fs::write(target.join("control.json"), "SYNTHETIC_CREDENTIAL").unwrap();
        // Quote fixture paths for cmd; percent expansion cannot be quoted away.
        for path in [&alias, &target] {
            assert!(!path.to_str().unwrap().contains(['%', '"', '\r', '\n']));
        }
        let result = std::process::Command::new("cmd.exe")
            .args(["/D", "/V:OFF", "/C"])
            .raw_arg(format!(
                "mklink /J \"{}\" \"{}\"",
                alias.display(),
                target.display()
            ))
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let actual = std::fs::canonicalize(target.join("control.json")).unwrap();
        let opened = std::path::PathBuf::from(super::super::to_canon(actual));
        assert!(deny_runtime_directory(&opened, &alias).is_err());
        std::fs::remove_dir(alias).unwrap();
    }

    #[test]
    fn javascript_bom_whitespace_is_also_sensitive_natively() {
        for path in [
            "/repo/.env\u{feff}x",
            "/repo/.ssh\u{feff}/key",
            "/repo/secrets.json.tmp-abc",
        ] {
            assert!(readable(Path::new(path)).is_err(), "{path}");
        }
        assert!(readable(Path::new("/repo/environment.ts")).is_ok());
    }

    #[test]
    fn a_server_path_is_checked_as_the_server_spells_it() {
        let ssh = WorkspaceEnv::Ssh {
            host: terax_workspace_core::env::SshHost::try_from("box".to_string()).unwrap(),
        };
        assert!(readable_in("/home/me/.ssh/id_ed25519", &ssh).is_err());
        assert!(readable_in("/srv/app/.env", &ssh).is_err());
        assert!(readable_in("/srv/app/src/main.rs", &ssh).is_ok());
        assert!(deny_runtime_in("/home/me/.cache/terax/control.json", &ssh).is_ok());
    }
}
