//! The agent's own confinement: every path a request names must lie inside
//! the session's roots, judged on this machine after resolving links. This is
//! defence in depth; the app's `WorkspaceRegistry` has already decided.
//!
//! Two questions, because operations differ in what they touch:
//! - `follow`: where the path really leads (metadata, reads, listings, the
//!   root of a search). A link inside a root that points out is refused.
//! - `entry`: where the directory entry itself sits (its canonical parent
//!   plus its name), for operations on the entry and not its target (removing
//!   or renaming a link, writing, creating, `symlink_metadata`).
//!
//! A path that does not exist yet is judged by its nearest existing ancestor.
//! `.` and `..` are left to the operating system where they fall inside that
//! ancestor (it resolves them as the operation will), and refused in the
//! names after it: appended unresolved, `root/..` would read as inside
//! `root`.

use std::io;

use terax_workspace_core::confined::within;
use terax_workspace_core::workspace::Workspace;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// Not absolute, with empty components, or with `.` or `..` where they
    /// cannot be resolved (in names that do not exist yet, or as the entry).
    Malformed(String),
    Outside(String),
    NoRoots,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Malformed(path) => write!(f, "refused malformed path `{path}`"),
            Self::Outside(path) => write!(f, "`{path}` is outside this session's roots"),
            Self::NoRoots => f.write_str("the session's roots are not set"),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Roots(Vec<String>);

impl Roots {
    /// Canonicalizes each root; each must exist.
    pub fn new(ws: &dyn Workspace, roots: &[String]) -> io::Result<Self> {
        let mut canonical = Vec::with_capacity(roots.len());
        for root in roots {
            parse(root).map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e.to_string()))?;
            canonical.push(ws.canonicalize(root)?);
        }
        Ok(Self(canonical))
    }

    pub fn first(&self) -> Option<&str> {
        self.0.first().map(String::as_str)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    fn admit(&self, canonical: &str, asked: &str) -> Result<(), Refusal> {
        if self.0.is_empty() {
            return Err(Refusal::NoRoots);
        }
        if self.0.iter().any(|root| within(canonical, root)) {
            Ok(())
        } else {
            Err(Refusal::Outside(asked.to_string()))
        }
    }

    /// Where `path` really leads must be inside a root.
    pub fn follow(&self, ws: &dyn Workspace, path: &str) -> Result<(), Refusal> {
        parse(path)?;
        let resolved = resolve(ws, path, false)?;
        self.admit(&resolved, path)
    }

    /// Where the entry `path` itself sits must be inside a root.
    pub fn entry(&self, ws: &dyn Workspace, path: &str) -> Result<(), Refusal> {
        parse(path)?;
        let resolved = resolve(ws, path, true)?;
        self.admit(&resolved, path)
    }
}

/// The components of an absolute, normalized path, or why it is refused.
/// Accepts `/a/b` and, on Windows (where tests run the agent), `C:/a/b`.
fn parse(path: &str) -> Result<(String, Vec<&str>), Refusal> {
    let malformed = || Refusal::Malformed(path.to_string());
    if cfg!(windows) && path.contains('\\') {
        return Err(malformed());
    }
    let bytes = path.as_bytes();
    let (head, rest) = if let Some(rest) = path.strip_prefix('/') {
        ("/".to_string(), rest)
    } else if cfg!(windows)
        && bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && &bytes[1..3] == b":/"
    {
        (path[..3].to_string(), &path[3..])
    } else {
        return Err(malformed());
    };
    let rest = rest.strip_suffix('/').unwrap_or(rest);
    if rest.is_empty() {
        return Ok((head, Vec::new()));
    }
    let parts: Vec<&str> = rest.split('/').collect();
    if parts.iter().any(|part| part.is_empty()) {
        return Err(malformed());
    }
    Ok((head, parts))
}

/// The canonical form of `path` (or of its entry, `entry`), resolving the
/// nearest existing ancestor and appending the names that do not exist yet.
fn resolve(ws: &dyn Workspace, path: &str, entry: bool) -> Result<String, Refusal> {
    let (head, parts) = parse(path)?;
    let (mut keep, mut tail): (usize, Vec<&str>) = (parts.len(), Vec::new());
    if entry {
        if let Some(name) = parts.last() {
            keep -= 1;
            tail.push(name);
        }
    }
    loop {
        let candidate = join(&head, &parts[..keep]);
        match ws.canonicalize(&candidate) {
            Ok(canonical) => {
                if tail.iter().any(|name| *name == "." || *name == "..") {
                    return Err(Refusal::Malformed(path.to_string()));
                }
                let mut out = canonical;
                for name in tail.iter().rev() {
                    if !out.ends_with('/') {
                        out.push('/');
                    }
                    out.push_str(name);
                }
                return Ok(out);
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound && keep > 0 => {
                keep -= 1;
                tail.push(parts[keep]);
            }
            // Unreadable, not Unicode, or not even the head exists.
            Err(_) => return Err(Refusal::Outside(path.to_string())),
        }
    }
}

fn join(head: &str, parts: &[&str]) -> String {
    let mut out = head.to_string();
    out.push_str(&parts.join("/"));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use terax_workspace_core::env::WorkspaceEnv;
    use terax_workspace_core::host::HostWorkspace;

    struct Fixture {
        _dir: tempfile::TempDir,
        ws: HostWorkspace,
        base: String,
        roots: Roots,
    }

    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let ws = HostWorkspace::new(WorkspaceEnv::Local);
        let base = ws.canonicalize(&dir.path().to_string_lossy()).unwrap();
        std::fs::create_dir_all(format!("{base}/root/sub")).unwrap();
        std::fs::create_dir_all(format!("{base}/rootish")).unwrap();
        std::fs::create_dir_all(format!("{base}/outside")).unwrap();
        std::fs::write(format!("{base}/root/a.txt"), "a").unwrap();
        std::fs::write(format!("{base}/outside/secret.txt"), "s").unwrap();
        let roots = Roots::new(&ws, &[format!("{base}/root")]).unwrap();
        Fixture {
            _dir: dir,
            ws,
            base,
            roots,
        }
    }

    #[test]
    fn paths_inside_the_roots_are_admitted_existing_or_not() {
        let f = fixture();
        let ok = |path: String| {
            f.roots.follow(&f.ws, &path).unwrap();
            f.roots.entry(&f.ws, &path).unwrap();
        };
        ok(format!("{}/root", f.base));
        ok(format!("{}/root/", f.base));
        ok(format!("{}/root/a.txt", f.base));
        ok(format!("{}/root/new/deeper/file.txt", f.base));
    }

    #[test]
    fn paths_outside_are_refused_including_a_sibling_with_a_shared_prefix() {
        let f = fixture();
        for path in [
            format!("{}/outside/secret.txt", f.base),
            format!("{}/rootish", f.base),
            format!("{}/outside/new.txt", f.base),
            f.base.clone(),
        ] {
            assert!(
                matches!(f.roots.follow(&f.ws, &path), Err(Refusal::Outside(_))),
                "{path}"
            );
            assert!(
                matches!(f.roots.entry(&f.ws, &path), Err(Refusal::Outside(_))),
                "{path}"
            );
        }
    }

    #[test]
    fn dots_in_existing_directories_resolve_where_they_lead() {
        let f = fixture();
        for path in [
            format!("{}/root/./a.txt", f.base),
            format!("{}/root/sub/../a.txt", f.base),
            format!("{}/root/sub/..", f.base),
        ] {
            f.roots.follow(&f.ws, &path).unwrap();
        }
        f.roots
            .entry(&f.ws, &format!("{}/root/sub/../a.txt", f.base))
            .unwrap();
        for path in [
            format!("{}/root/../outside/secret.txt", f.base),
            format!("{}/root/..", f.base),
            format!("{}/root/sub/../..", f.base),
        ] {
            assert!(
                matches!(f.roots.follow(&f.ws, &path), Err(Refusal::Outside(_))),
                "{path}"
            );
        }
    }

    /// Appended unresolved, these would read as inside the root while the
    /// operation acts on the root's parent or on a directory outside.
    #[test]
    fn dots_that_cannot_be_resolved_are_refused() {
        let f = fixture();
        for path in [
            format!("{}/root/..", f.base),
            format!("{}/root/sub/..", f.base),
            format!("{}/root/.", f.base),
        ] {
            assert!(
                matches!(f.roots.entry(&f.ws, &path), Err(Refusal::Malformed(_))),
                "entry {path}"
            );
        }
        // Windows resolves `..` by the text, even past a name that does not
        // exist (and so will the operation); elsewhere it cannot be resolved.
        let escape = format!("{}/root/missing/../../outside", f.base);
        assert!(f.roots.follow(&f.ws, &escape).is_err(), "{escape}");
        #[cfg(unix)]
        assert!(matches!(
            f.roots
                .follow(&f.ws, &format!("{}/root/missing/..", f.base)),
            Err(Refusal::Malformed(_))
        ));
    }

    #[test]
    fn relative_and_empty_components_are_refused() {
        let f = fixture();
        for path in [
            "relative/a.txt".to_string(),
            "../a.txt".to_string(),
            format!("{}/root//a.txt", f.base),
            String::new(),
        ] {
            assert!(
                matches!(f.roots.follow(&f.ws, &path), Err(Refusal::Malformed(_))),
                "{path:?}"
            );
        }
    }

    #[test]
    fn no_roots_refuses_everything() {
        let f = fixture();
        let none = Roots::default();
        assert_eq!(
            none.follow(&f.ws, &format!("{}/root/a.txt", f.base)),
            Err(Refusal::NoRoots)
        );
    }

    /// A link inside the root: following it leads out (refused), but the
    /// entry itself is inside, so it can still be removed or renamed.
    #[test]
    fn a_link_out_of_a_root_is_refused_to_follow_but_admitted_as_an_entry() {
        let f = fixture();
        let link = format!("{}/root/escape", f.base);
        let target = format!("{}/outside", f.base);
        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, &link).unwrap();
        #[cfg(windows)]
        if let Err(error) = std::os::windows::fs::symlink_dir(&target, &link) {
            eprintln!("skipped: no symlink privilege ({error})");
            return;
        }
        assert!(matches!(
            f.roots.follow(&f.ws, &link),
            Err(Refusal::Outside(_))
        ));
        assert!(matches!(
            f.roots.follow(&f.ws, &format!("{link}/secret.txt")),
            Err(Refusal::Outside(_))
        ));
        assert!(
            matches!(
                f.roots.entry(&f.ws, &format!("{link}/secret.txt")),
                Err(Refusal::Outside(_))
            ),
            "an entry under the link sits outside"
        );
        f.roots.entry(&f.ws, &link).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn windows_drive_paths_parse_and_backslashes_do_not() {
        assert!(parse("C:/x/y").is_ok());
        assert!(parse("C:/").is_ok());
        assert!(matches!(parse("C:\\x"), Err(Refusal::Malformed(_))));
        assert!(matches!(parse("C:x"), Err(Refusal::Malformed(_))));
    }
}
