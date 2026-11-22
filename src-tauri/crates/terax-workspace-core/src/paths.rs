//! Spelling of canonical paths, shared by every backend.

use std::path::Path;

/// The single canonical-to-display conversion: forward slashes, Windows
/// verbatim `\\?\` prefix stripped. Route every such conversion through here.
pub fn to_canon(p: impl AsRef<Path>) -> String {
    let s = p.as_ref().to_string_lossy();
    #[cfg(windows)]
    {
        strip_verbatim(&s)
    }
    #[cfg(not(windows))]
    {
        // Backslashes are legal in Unix filenames; never rewrite them.
        s.into_owned()
    }
}

// Pure so it stays unit-testable on any host. `\\?\C:\x` -> `C:/x`.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn strip_verbatim(s: &str) -> String {
    let stripped = if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = s.strip_prefix(r"\\?\") {
        rest.to_string()
    } else {
        s.to_string()
    };
    stripped.replace('\\', "/")
}

/// Maps a canonical host path back into a WSL distro's namespace.
#[cfg(any(windows, test))]
pub fn canonical_wsl_path(path: &str, distro: &str) -> Result<String, String> {
    for host in ["wsl.localhost", "wsl$"] {
        let prefix = format!("//{host}/{distro}");
        if path
            .get(..prefix.len())
            .is_some_and(|p| p.eq_ignore_ascii_case(&prefix))
        {
            let tail = &path[prefix.len()..];
            if tail.is_empty() {
                return Ok("/".into());
            }
            if tail.starts_with('/') {
                return Ok(tail.into());
            }
        }
    }
    let bytes = path.as_bytes();
    if bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && &bytes[1..3] == b":/" {
        return Ok(format!(
            "/mnt/{}/{}",
            (bytes[0] as char).to_ascii_lowercase(),
            &path[3..]
        ));
    }
    Err("canonical path is outside the workspace's WSL namespace".into())
}

#[cfg(test)]
mod tests {
    use super::{canonical_wsl_path, strip_verbatim};
    use proptest::prelude::*;

    #[test]
    fn strips_drive_verbatim_prefix() {
        assert_eq!(strip_verbatim(r"\\?\C:\Users\foo"), "C:/Users/foo");
    }

    #[test]
    fn rewrites_verbatim_unc_to_share_path() {
        assert_eq!(
            strip_verbatim(r"\\?\UNC\server\share\dir"),
            "//server/share/dir"
        );
    }

    #[test]
    fn passes_through_plain_windows_path() {
        assert_eq!(strip_verbatim(r"C:\Users\foo"), "C:/Users/foo");
    }

    #[test]
    fn leaves_forward_slash_path_unchanged() {
        assert_eq!(strip_verbatim("C:/Users/foo"), "C:/Users/foo");
    }

    #[test]
    fn handles_drive_root() {
        assert_eq!(strip_verbatim(r"\\?\C:\"), "C:/");
    }

    proptest! {
        #[test]
        fn strip_verbatim_never_leaves_backslashes_or_prefix(s in r"[A-Za-z0-9\\/: .]{0,40}") {
            let out = strip_verbatim(&s);
            prop_assert!(!out.contains('\\'));
            prop_assert!(!out.starts_with(r"\\?\"));
        }

        #[test]
        fn strip_verbatim_is_idempotent(s in r"[A-Za-z0-9\\/: .]{0,40}") {
            let once = strip_verbatim(&s);
            prop_assert_eq!(strip_verbatim(&once), once);
        }

        #[test]
        fn strip_verbatim_on_plain_input_equals_slash_swap(s in r"[A-Za-z0-9\\/: .]{0,40}") {
            prop_assume!(!s.starts_with(r"\\?\"));
            prop_assert_eq!(strip_verbatim(&s), s.replace('\\', "/"));
        }

        #[test]
        fn strip_verbatim_drive_root_is_preserved(
            drive in r"[A-Z]",
            tail in r"[A-Za-z0-9\\/ .]{0,40}",
        ) {
            let input = format!(r"\\?\{drive}:\{tail}");
            let out = strip_verbatim(&input);
            let expected = format!("{drive}:/");
            prop_assert!(out.starts_with(&expected));
        }

        #[test]
        fn strip_verbatim_unc_becomes_double_slash(tail in r"[A-Za-z0-9\\/ .]{0,40}") {
            let input = format!(r"\\?\UNC\{tail}");
            let out = strip_verbatim(&input);
            prop_assert!(out.starts_with("//"));
            prop_assert!(!out.starts_with(r"\\?\"));
        }
    }

    #[test]
    fn canonical_wsl_paths_keep_the_distro_namespace() {
        assert_eq!(
            canonical_wsl_path("//wsl.localhost/Ubuntu/home/me/target", "Ubuntu").unwrap(),
            "/home/me/target"
        );
        assert_eq!(
            canonical_wsl_path("//wsl$/Ubuntu/home/Target", "Ubuntu").unwrap(),
            "/home/Target"
        );
        assert_eq!(
            canonical_wsl_path("C:/work/target", "Ubuntu").unwrap(),
            "/mnt/c/work/target"
        );
        assert_eq!(
            canonical_wsl_path("//wsl.localhost/Ubuntu", "Ubuntu").unwrap(),
            "/"
        );
        assert!(canonical_wsl_path("//wsl.localhost/Ubuntu-other/file", "Ubuntu").is_err());
        assert!(canonical_wsl_path("//remote/share/file", "Ubuntu").is_err());
    }
}
