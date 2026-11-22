//! Reads that may only see files inside given roots, checked against the file
//! that was actually opened. A link swapped in after a caller's own checks
//! cannot redirect the read: the backend opens first, asks the OS where the
//! opened file really is, and only then decides. A remote agent runs this same
//! check on the server, so the policy lives here rather than in the app.

use std::io;
use std::sync::OnceLock;

use regex::{RegexSet, RegexSetBuilder};
use serde::Deserialize;

/// A place a confined read may see: exactly `path`, or (`subtree`) anything
/// beneath it as well. A canonical workspace path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Confine {
    pub path: String,
    pub subtree: bool,
}

#[derive(Clone, Debug)]
pub struct ConfinedRead {
    /// The opened file must be inside one of these.
    pub roots: Vec<Confine>,
    /// Also refuse secret-looking paths ([`refusal`]) by the opened file's
    /// real path.
    pub secret_policy: bool,
    pub offset: u64,
    /// Up to `limit + 1` bytes come back, as with `Workspace::read`.
    pub limit: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfinedOutcome {
    /// The opened file's canonical workspace path.
    pub real_path: String,
    pub bytes: Vec<u8>,
    /// The file's length when it was opened.
    pub size: u64,
    pub mtime_ms: u64,
}

#[derive(Debug)]
pub enum ConfinedError {
    Io(io::Error),
    /// The opened object is not a regular file (also one swapped in late).
    NotAFile,
    /// The real path cannot be spelled exactly, so it cannot be compared.
    Inexact,
    /// The opened file is outside every root.
    Outside,
    /// The secret-path policy refused the real path.
    Refused(&'static str),
}

impl From<io::Error> for ConfinedError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl std::fmt::Display for ConfinedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "{error}"),
            Self::NotAFile => f.write_str("not a regular file"),
            Self::Inexact => f.write_str("path cannot be represented exactly"),
            Self::Outside => f.write_str("outside the allowed roots"),
            Self::Refused(why) => f.write_str(why),
        }
    }
}

/// True when the canonical `/`-separated `path` is `root` or lies beneath it,
/// by component (`/a/b` is within `/a`, `/a/bc` is not).
pub fn within(path: &str, root: &str) -> bool {
    let root = root.trim_end_matches('/');
    let path = path.trim_end_matches('/');
    path == root || path.strip_prefix(root).is_some_and(|rest| rest.starts_with('/'))
}

/// Whether the opened file's real path lies inside one of `roots`.
pub fn allowed(real_path: &str, roots: &[Confine]) -> bool {
    roots.iter().any(|root| {
        real_path.trim_end_matches('/') == root.path.trim_end_matches('/')
            || (root.subtree && within(real_path, &root.path))
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PathPolicy {
    secret_basenames: Vec<String>,
    protected_dirs: Vec<String>,
}

/// Why the secret-path policy refuses `path`, or `None`. The patterns are the
/// frontend's own (`pathPolicy.json`), so both sides refuse the same names.
pub fn refusal(path: &str) -> Option<&'static str> {
    static POLICY: OnceLock<(RegexSet, Vec<String>)> = OnceLock::new();
    let (patterns, directories) = POLICY.get_or_init(|| {
        let policy: PathPolicy = serde_json::from_str(include_str!(
            "../../../../src/modules/ai/lib/pathPolicy.json"
        ))
        .expect("valid path policy");
        (
            RegexSetBuilder::new(policy.secret_basenames)
                .case_insensitive(true)
                .build()
                .expect("valid secret patterns"),
            policy.protected_dirs,
        )
    });
    if path.is_empty() || path.chars().any(|c| c < ' ') {
        return Some("refused invalid AI path");
    }
    let normalized = path.replace('\\', "/");
    let basename = normalized.rsplit('/').next().unwrap_or("");
    if patterns.is_match(&basename.replace('\u{feff}', " ")) {
        return Some("refused sensitive AI file");
    }
    let comparison = normalized
        .split('/')
        .map(|part| {
            part.split(':')
                .next()
                .unwrap_or("")
                .trim_end_matches(|c: char| c == '.' || c == '\u{feff}' || c.is_whitespace())
                .to_lowercase()
        })
        .collect::<Vec<_>>()
        .join("/");
    let comparison = format!("{comparison}/");
    if directories
        .iter()
        .any(|dir| comparison.contains(&format!("{dir}/")))
    {
        return Some("refused protected AI directory");
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roots_admit_exact_files_or_whole_subtrees() {
        let file = Confine {
            path: "/r/a.png".into(),
            subtree: false,
        };
        let dir = Confine {
            path: "/r".into(),
            subtree: true,
        };
        assert!(allowed("/r/a.png", std::slice::from_ref(&file)));
        assert!(!allowed("/r/a.png.bak", std::slice::from_ref(&file)));
        assert!(!allowed("/r/a.png/x", std::slice::from_ref(&file)));
        assert!(allowed("/r/sub/a.png", std::slice::from_ref(&dir)));
        assert!(!allowed("/rx/a.png", std::slice::from_ref(&dir)));
        assert!(!allowed("/r/a.png", &[]));
        assert!(within("C:/r/x", "C:/"));
        assert!(within("/x", "/"));
    }

    #[test]
    fn the_shared_policy_refuses_secrets_by_name_and_directory() {
        for path in [
            "/repo/.env",
            "/repo/.env.local",
            "C:/repo/id_ed25519",
            "/repo/.ssh/config",
            "/repo/.env\u{feff}x",
            "/repo/.ssh\u{feff}/key",
            "/repo/secrets.json.tmp-abc",
            "/repo/a\nb",
            "",
        ] {
            assert!(refusal(path).is_some(), "{path:?}");
        }
        for path in ["/repo/environment.ts", "/repo/src/key.rs", "C:/repo/README.md"] {
            assert_eq!(refusal(path), None, "{path:?}");
        }
    }
}
