use std::cmp::Ordering;
use std::collections::HashSet;
use std::path::Path;

use ignore::WalkBuilder;
use serde::Serialize;

use super::blocking;
use crate::modules::workspace::{backend_for, resolve_path, WorkspaceEnv};
use terax_workspace_core::paths::join;
use terax_workspace_core::workspace::EntryKind as WsKind;

#[derive(Serialize)]
#[serde(rename_all = "lowercase")]
pub enum EntryKind {
    File,
    Dir,
    Symlink,
}

#[derive(Serialize)]
pub struct DirEntry {
    pub name: String,
    pub kind: EntryKind,
    pub size: u64,
    /// Milliseconds since UNIX epoch; 0 if unavailable.
    pub mtime: u64,
    pub gitignored: bool,
}

// Whether `dir` is inside a git repo. Walks up only; never descends into
// siblings, so it does not touch protected macOS folders (Desktop, ...).
fn in_git_repo(dir: &Path) -> bool {
    let mut cur = dir;
    loop {
        if cur.join(".git").exists() {
            return true;
        }
        match cur.parent() {
            Some(p) => cur = p,
            None => return false,
        }
    }
}

// Immediate children of `dir` that git does not ignore. Outside a repo every
// name is returned, so nothing is dimmed.
fn git_non_ignored_names(dir: &Path, show_hidden: bool) -> HashSet<String> {
    WalkBuilder::new(dir)
        .hidden(!show_hidden)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .ignore(false)
        .parents(true)
        .max_depth(Some(1))
        .follow_links(false)
        .build()
        .flatten()
        .filter_map(|d| d.file_name().to_str().map(str::to_string))
        .collect()
}

/// The names in `dir` (on a server) that git ignores, from one
/// `git check-ignore` run there. Outside a repository, or without git, none.
fn remote_ignored_names<'a>(
    ws: &dyn terax_workspace_core::workspace::Workspace,
    dir: &str,
    names: impl Iterator<Item = &'a str>,
) -> HashSet<String> {
    use terax_workspace_core::exec::capture::Limits;
    use terax_workspace_core::exec::ExecSpec;

    let mut stdin = Vec::new();
    for name in names.filter(|name| *name != ".git") {
        stdin.extend_from_slice(name.as_bytes());
        stdin.push(0);
    }
    if stdin.is_empty() {
        return HashSet::new();
    }
    let spec = ExecSpec {
        program: "git".into(),
        args: ["check-ignore", "-z", "--stdin"].map(Into::into).to_vec(),
        cwd: Some(dir.to_string()),
        env: vec![
            ("GIT_TERMINAL_PROMPT".into(), "0".into()),
            ("GIT_OPTIONAL_LOCKS".into(), "0".into()),
        ],
        env_remove: ["GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE"]
            .map(Into::into)
            .to_vec(),
        stdin: Some(stdin),
        timeout: std::time::Duration::from_secs(10),
        limits: Limits {
            stdout: 1024 * 1024,
            stderr: 64 * 1024,
            stop_on_stdout_cap: true,
        },
    };
    match ws.exec(spec) {
        // 0: some are ignored; 1: none; 128: not a repository.
        Ok(out) if out.exit_code == Some(0) => out
            .stdout
            .split(|byte| *byte == 0)
            .filter(|name| !name.is_empty())
            .filter_map(|name| std::str::from_utf8(name).ok())
            .map(|name| name.trim_end_matches('/').to_string())
            .collect(),
        _ => HashSet::new(),
    }
}

fn natural_cmp_inner(a: &str, b: &str) -> Ordering {
    let a = a.as_bytes();
    let b = b.as_bytes();
    let mut ai = 0;
    let mut bi = 0;

    loop {
        match (a.get(ai).copied(), b.get(bi).copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(ac), Some(bc)) => {
                if ac.is_ascii_digit() && bc.is_ascii_digit() {
                    let a_end = ai
                        + a[ai..]
                            .iter()
                            .take_while(|byte| byte.is_ascii_digit())
                            .count();
                    let b_end = bi
                        + b[bi..]
                            .iter()
                            .take_while(|byte| byte.is_ascii_digit())
                            .count();
                    let a_run = &a[ai..a_end];
                    let b_run = &b[bi..b_end];
                    let a_zeros = a_run.iter().take_while(|byte| **byte == b'0').count();
                    let b_zeros = b_run.iter().take_while(|byte| **byte == b'0').count();
                    let a_number = &a_run[a_zeros..];
                    let b_number = &b_run[b_zeros..];

                    let ord = a_number.len().cmp(&b_number.len());
                    if ord != Ordering::Equal {
                        return ord;
                    }
                    let ord = a_number.cmp(b_number);
                    if ord != Ordering::Equal {
                        return ord;
                    }
                    let ord = a_run.len().cmp(&b_run.len());
                    if ord != Ordering::Equal {
                        return ord;
                    }
                    ai = a_end;
                    bi = b_end;
                } else {
                    let ord = ac.to_ascii_lowercase().cmp(&bc.to_ascii_lowercase());
                    if ord != Ordering::Equal {
                        return ord;
                    }
                    ai += 1;
                    bi += 1;
                }
            }
        }
    }
}

fn natural_cmp(a: &str, b: &str) -> Ordering {
    let a_folded = (!a.is_ascii()).then(|| a.to_lowercase());
    let b_folded = (!b.is_ascii()).then(|| b.to_lowercase());
    natural_cmp_inner(
        a_folded.as_deref().unwrap_or(a),
        b_folded.as_deref().unwrap_or(b),
    )
}

/// Lists immediate children of `path`. Dirs first, then files, each sorted
/// with `natural_cmp` (numeric-aware, case-insensitive). Dot-prefixed entries
/// (files and dirs) are hidden unless `show_hidden` is set. `git_decorations`
/// opts into the per-entry `gitignored` flag; off by default so non-explorer
/// callers pay nothing.
#[tauri::command]
pub async fn fs_read_dir(
    path: String,
    show_hidden: bool,
    git_decorations: Option<bool>,
    workspace: Option<WorkspaceEnv>,
) -> Result<Vec<DirEntry>, String> {
    blocking(move || read_dir_entries(path, show_hidden, git_decorations, workspace)).await
}

pub fn read_dir_entries(
    path: String,
    show_hidden: bool,
    git_decorations: Option<bool>,
    workspace: Option<WorkspaceEnv>,
) -> Result<Vec<DirEntry>, String> {
    let workspace = WorkspaceEnv::from_option(workspace);
    let ws = backend_for(&workspace);
    let read = ws.read_dir(&path).map_err(|e| {
        log::debug!("fs_read_dir({path}) failed: {e}");
        e.to_string()
    })?;

    // On this machine gitignore decorations walk host paths with the
    // `ignore` crate; a server's folder asks git there (`check-ignore`).
    let remote_ignored = if git_decorations.unwrap_or(false) && workspace.is_ssh() {
        let names = read.iter().map(|entry| entry.name.as_str());
        Some(remote_ignored_names(&*ws, &path, names))
    } else {
        None
    };
    let root = resolve_path(&path, &workspace);
    // Gate on a real repo: outside one the walk is pointless and would probe
    // each child for a nested `.git`, which trips macOS folder-access prompts.
    let git_decorations =
        git_decorations.unwrap_or(false) && !workspace.is_ssh() && in_git_repo(&root);
    let git_visible = if git_decorations {
        git_non_ignored_names(&root, show_hidden)
    } else {
        HashSet::new()
    };

    let mut entries: Vec<DirEntry> = read
        .into_iter()
        .filter_map(|entry| {
            let name = entry.name;
            if name.starts_with('.') && !show_hidden {
                return None;
            }
            let child = join(&path, &name);

            // Follow symlinks for the target's kind, size and mtime; a broken
            // symlink falls back to the link itself so it is still listed.
            let (meta, was_symlink) = match ws.metadata(&child) {
                Ok(m) => (Some(m), false),
                Err(_) => (ws.symlink_metadata(&child).ok(), true),
            };
            let meta = meta?;

            let kind = if was_symlink {
                EntryKind::Symlink
            } else if meta.kind == WsKind::Dir {
                EntryKind::Dir
            } else {
                EntryKind::File
            };

            let gitignored = match &remote_ignored {
                Some(ignored) => ignored.contains(&name),
                None => git_decorations && !git_visible.contains(&name),
            };
            Some(DirEntry {
                name,
                kind,
                size: meta.len,
                mtime: meta.mtime_ms,
                gitignored,
            })
        })
        .collect();

    entries.sort_by(|a, b| {
        let rank = |k: &EntryKind| match k {
            EntryKind::Dir => 0,
            EntryKind::Symlink => 1,
            EntryKind::File => 2,
        };
        rank(&a.kind)
            .cmp(&rank(&b.kind))
            .then_with(|| natural_cmp(&a.name, &b.name))
    });

    Ok(entries)
}

/// Lists immediate subdirectories of `path`. Kept for the CwdBreadcrumb.
///
/// Symlinks to directories are included (matches shell `cd` semantics).
/// Hidden entries are filtered by dot-prefix only.
#[tauri::command]
pub async fn list_subdirs(
    path: String,
    show_hidden: bool,
    workspace: Option<WorkspaceEnv>,
) -> Result<Vec<String>, String> {
    blocking(move || subdirs(path, show_hidden, workspace)).await
}

pub fn subdirs(
    path: String,
    show_hidden: bool,
    workspace: Option<WorkspaceEnv>,
) -> Result<Vec<String>, String> {
    let ws = backend_for(&WorkspaceEnv::from_option(workspace));
    let read = ws.read_dir(&path).map_err(|e| {
        log::debug!("list_subdirs({path}) read_dir failed: {e}");
        e.to_string()
    })?;

    let mut dirs: Vec<String> = read
        .into_iter()
        .filter(|entry| show_hidden || !entry.name.starts_with('.'))
        .filter(|entry| match entry.kind {
            WsKind::Dir => true,
            WsKind::Symlink => ws
                .metadata(&join(&path, &entry.name))
                .is_ok_and(|m| m.kind == WsKind::Dir),
            _ => false,
        })
        .map(|entry| entry.name)
        .collect();

    dirs.sort_by(|a, b| natural_cmp(a, b));
    Ok(dirs)
}

#[cfg(test)]
mod tests {
    use super::natural_cmp;
    use std::cmp::Ordering;

    fn sorted(mut v: Vec<&str>) -> Vec<&str> {
        v.sort_by(|a, b| natural_cmp(a, b));
        v
    }

    #[test]
    fn numbers_sort_as_values_not_lexically() {
        assert_eq!(
            sorted(vec!["17", "170", "18", "3", "20", "171", "9"]),
            vec!["3", "9", "17", "18", "20", "170", "171"],
        );
    }

    #[test]
    fn numbers_embedded_in_names() {
        assert_eq!(
            sorted(vec!["10-x-plan.md", "2-x-plan.md", "1-x-plan.md"]),
            vec!["1-x-plan.md", "2-x-plan.md", "10-x-plan.md"],
        );
    }

    #[test]
    fn case_insensitive() {
        assert_eq!(natural_cmp("Apple", "apple"), Ordering::Equal);
        assert_eq!(natural_cmp("École", "école"), Ordering::Equal);
        assert_eq!(natural_cmp("Б2", "б2"), Ordering::Equal);
        assert_eq!(sorted(vec!["b", "A", "c"]), vec!["A", "b", "c"]);
        assert_eq!(sorted(vec!["École", "éclair"]), vec!["éclair", "École"]);
    }

    #[test]
    fn non_cased_unicode_keeps_code_point_order() {
        assert_eq!(sorted(vec!["나", "가", "다"]), vec!["가", "나", "다"]);
    }

    #[test]
    fn leading_zeros_are_a_total_order() {
        // Same value, differing zero padding: never Equal, and antisymmetric.
        assert_eq!(natural_cmp("007", "7"), Ordering::Greater);
        assert_eq!(natural_cmp("7", "007"), Ordering::Less);
        assert_eq!(sorted(vec!["v007", "v7", "v08"]), vec!["v7", "v007", "v08"]);
    }

    #[test]
    fn long_numbers_compare_without_overflow() {
        assert_eq!(
            natural_cmp("file99999999999999999999", "file100000000000000000000"),
            Ordering::Less,
        );
    }

    #[test]
    fn comparator_is_antisymmetric_and_transitive() {
        let names = [
            "", "A", "a", "É", "é", "Б", "б", "0", "00", "1", "file2", "file02", "file10", "가",
        ];

        for a in names {
            for b in names {
                assert_eq!(natural_cmp(a, b), natural_cmp(b, a).reverse());
                for c in names {
                    if natural_cmp(a, b) != Ordering::Greater
                        && natural_cmp(b, c) != Ordering::Greater
                    {
                        assert_ne!(natural_cmp(a, c), Ordering::Greater);
                    }
                }
            }
        }
    }
}
