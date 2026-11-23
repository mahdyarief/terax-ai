//! Workspace commands reach files and processes through the `Workspace`
//! backend, so they also work where this process cannot open the files (a
//! remote host). This test reads the fs, git and remote modules' sources and fails
//! when a direct host call appears outside the files listed below.
//! `remote` is scanned too: an SSH connection must never fall back to a
//! path or a process on this machine.
//!
//! Each listed file has an exact count, so a new call in an allowed file is
//! caught too. When a call moves behind the backend, lower its count; when one
//! is really host-only, raise it here and say why.

use std::path::{Path, PathBuf};

const TOKENS: [&str; 5] = [
    "std::fs",
    "File::open",
    "File::create",
    "Command::new",
    "OpenOptions",
];

/// (path under `src/modules`, exact count, why it stays on the host)
const HOST_ONLY: &[(&str, usize, &str)] = &[
    (
        "fs/ai.rs",
        1,
        "ai_glob/ai_grep walk host paths; they move with the search engines",
    ),
    (
        "fs/index.rs",
        1,
        "the file-list index runs on the host engine, keyed by host canonical paths",
    ),
    (
        "fs/local_file.rs",
        1,
        "opens files the user picked on this machine (dialogs, clipboard)",
    ),
    (
        "fs/mutate.rs",
        1,
        "reports changes to the host file-list index (see fs/index.rs)",
    ),
    (
        "fs/read_policy.rs",
        1,
        "this app's own credential directory, which is on this machine",
    ),
    (
        "fs/watch.rs",
        3,
        "the watcher runs on the host engine; a remote one belongs to the agent",
    ),
    (
        "git/checkpoint.rs",
        29,
        "checkpoints are local-only (hard links, mtimes, the racy-git index copy)",
    ),
    (
        "git/plumbing.rs",
        1,
        "git_dir/common_dir: host paths for the local-only modules",
    ),
    (
        "git/project_config.rs",
        6,
        "worktree placement, which moves with worktree.rs",
    ),
    (
        "git/task_state.rs",
        7,
        "the task store is app state under ~/.terax, with an OS file lock",
    ),
    ("git/task_worktree.rs", 20, "task worktrees are local-only"),
    (
        "remote/ssh_config.rs",
        1,
        "the user's ~/.ssh/config and its includes are on this machine, as is ssh",
    ),
    (
        "git/watch.rs",
        7,
        "the git state watcher is local-only and runs on the host engine",
    ),
    (
        "git/worktree.rs",
        33,
        "creating and removing worktrees is local-only",
    ),
];

fn modules_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src/modules")
}

fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            sources(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

/// Test-only files: whole modules of tests and their shared fixtures.
fn test_only(name: &str) -> bool {
    name.ends_with("_tests.rs") || name == "testutil.rs"
}

/// The production part of a source file: everything before its inline test
/// module (`#[cfg(...test...)]` followed by `mod name {`), without line
/// comments. A `#[cfg(test)]` on a single item does not end it.
fn production(source: &str) -> Vec<&str> {
    let lines: Vec<&str> = source.lines().collect();
    let mut kept = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        let next = lines.get(i + 1).map(|l| l.trim()).unwrap_or("");
        let opens_module = ["mod ", "pub mod ", "pub(crate) mod ", "pub(super) mod "]
            .iter()
            .any(|prefix| next.starts_with(prefix))
            && next.ends_with('{');
        if trimmed.starts_with("#[cfg(") && trimmed.contains("test") && opens_module {
            break;
        }
        if !trimmed.starts_with("//") {
            kept.push(*line);
        }
    }
    kept
}

fn count(lines: &[&str]) -> usize {
    lines
        .iter()
        .map(|line| {
            TOKENS
                .iter()
                .map(|token| {
                    line.match_indices(token)
                        .filter(|(at, token)| {
                            // Whole words: `File::open` is not
                            // `ProjectFile::open`, `OpenOptions` not `OpenOptionsExt`.
                            let ident = |c: char| c.is_alphanumeric() || c == '_';
                            !line[..*at].chars().next_back().is_some_and(ident)
                                && !line[at + token.len()..].chars().next().is_some_and(ident)
                        })
                        .count()
                })
                .sum::<usize>()
        })
        .sum()
}

fn host_calls() -> Vec<(String, usize)> {
    let root = modules_dir();
    let mut files = Vec::new();
    for dir in ["fs", "git", "remote"] {
        sources(&root.join(dir), &mut files);
    }
    let mut found: Vec<(String, usize)> = files
        .into_iter()
        .filter(|path| !test_only(&path.file_name().unwrap().to_string_lossy()))
        .filter_map(|path| {
            let source = std::fs::read_to_string(&path).unwrap();
            let n = count(&production(&source));
            let rel = path
                .strip_prefix(&root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            (n > 0).then_some((rel, n))
        })
        .collect();
    found.sort();
    found
}

#[test]
fn workspace_commands_reach_files_through_the_backend() {
    let found = host_calls();
    let mut problems = Vec::new();
    for (file, n) in &found {
        match HOST_ONLY.iter().find(|(allowed, _, _)| allowed == file) {
            None => problems.push(format!(
                "{file}: {n} direct host call(s); go through `backend_for(workspace)` \
                 (or, if this is really host-only, add it to HOST_ONLY with a reason)"
            )),
            Some((_, expected, _)) if expected != n => problems.push(format!(
                "{file}: {n} direct host call(s), HOST_ONLY expects {expected}; \
                 update the count (and the reason, if a new call was added)"
            )),
            Some(_) => {}
        }
    }
    for (file, _, _) in HOST_ONLY {
        if !found.iter().any(|(f, _)| f == file) {
            problems.push(format!(
                "{file}: no direct host calls left; remove it from HOST_ONLY"
            ));
        }
    }
    assert!(problems.is_empty(), "\n{}", problems.join("\n"));
}

#[test]
fn every_host_only_file_says_why() {
    for (file, _, why) in HOST_ONLY {
        assert!(why.len() > 20, "{file} needs a real reason");
        assert!(modules_dir().join(file).is_file(), "{file} does not exist");
    }
}

#[test]
fn the_scan_sees_what_it_guards() {
    let source = "\
use std::fs;
fn a() { let _ = ProjectFile::open(); let _ = File::open(\"x\"); }
use std::os::unix::fs::OpenOptionsExt;
// std::fs in a comment
#[cfg(test)]
fn only_in_tests() {}
fn b() { std::process::Command::new(\"git\"); }
#[cfg(test)]
mod tests {
    fn c() { std::fs::read(\"x\"); }
}
";
    assert_eq!(count(&production(source)), 3);
    assert!(test_only("edge_tests.rs") && test_only("testutil.rs"));
    assert!(!test_only("utils.rs"));
}
