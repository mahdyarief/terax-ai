//! Source control on a server's folder: the git commands run by git on the
//! server (the real agent served in-process), with the repository found and
//! authorized in the server's spelling.

use std::path::Path;
use std::process::Command;

use terax_workspace_core::env::WorkspaceEnv;

use super::testing::connected;
use crate::modules::git::types::DiscardEntry;
use crate::modules::git::{
    branch, checkpoint, operations, project_config, task_worktree, worktree,
};
use crate::modules::workspace::WorkspaceRegistry;

struct Repo {
    _dir: tempfile::TempDir,
    /// The repository's root, as the server spells it.
    root: String,
    env: WorkspaceEnv,
    registry: WorkspaceRegistry,
}

fn git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// A repository with one commit (`README.md`) in `repo/`, its folder
/// authorized on host `name`, and a sibling `outside/` repository.
fn repo(name: &str) -> Repo {
    let env = WorkspaceEnv::Ssh {
        host: connected(name),
    };
    let dir = tempfile::tempdir().unwrap();
    for sub in ["repo", "outside"] {
        let path = dir.path().join(sub);
        std::fs::create_dir(&path).unwrap();
        git(&path, &["init", "-q", "-b", "main"]);
        for (key, value) in [
            ("user.name", "Test"),
            ("user.email", "test@example.com"),
            ("commit.gpgsign", "false"),
            ("core.autocrlf", "false"),
        ] {
            git(&path, &["config", key, value]);
        }
        std::fs::write(path.join("README.md"), "# demo\n").unwrap();
        git(&path, &["add", "."]);
        git(&path, &["commit", "-q", "-m", "initial"]);
    }
    let base = dir.path().to_string_lossy().replace('\\', "/");
    let registry = WorkspaceRegistry::default();
    let root = registry
        .authorize_in(&env, &format!("{base}/repo"))
        .unwrap();
    Repo {
        _dir: dir,
        root,
        env,
        registry,
    }
}

impl Repo {
    fn write(&self, rel: &str, content: &str) {
        std::fs::write(format!("{}/{rel}", self.root), content).unwrap();
    }
    fn changed(&self) -> Vec<(String, bool, bool)> {
        operations::status(&self.registry, &self.root, &self.env)
            .unwrap()
            .changed_files
            .into_iter()
            .map(|f| (f.path, f.staged, f.untracked))
            .collect()
    }
}

#[test]
fn the_panel_finds_the_repository_on_the_server() {
    let r = repo("git-panel");
    let info = operations::resolve_repo(&r.registry, &r.root, &r.env)
        .unwrap()
        .expect("a repository");
    assert_eq!(info.branch, "main");
    assert_eq!(
        info.repo_root.trim_end_matches('/'),
        r.root.trim_end_matches('/')
    );
    let snapshot = operations::panel_snapshot(&r.registry, &r.root, &r.env).unwrap();
    assert!(snapshot.status.unwrap().changed_files.is_empty());

    // The sibling repository is not authorized.
    let outside = format!("{}/outside", r.root.trim_end_matches("/repo"));
    assert!(operations::status(&r.registry, &outside, &r.env).is_err());
    assert!(operations::resolve_repo(&r.registry, &outside, &r.env).is_err());
}

#[test]
fn changes_are_staged_committed_and_shown_in_the_history() {
    let r = repo("git-commit");
    r.write("README.md", "# demo\nmore\n");
    r.write("notes.md", "new\n");
    assert_eq!(
        r.changed(),
        [
            ("README.md".to_string(), false, false),
            ("notes.md".to_string(), false, true)
        ]
    );
    let diff = operations::diff(&r.registry, &r.root, Some("README.md"), false, &r.env).unwrap();
    assert!(diff.diff_text.contains("+more"), "{}", diff.diff_text);

    operations::stage(&r.registry, &r.root, &["notes.md".into()], &r.env).unwrap();
    assert!(r.changed().contains(&("notes.md".to_string(), true, false)));
    operations::unstage(&r.registry, &r.root, &["notes.md".into()], &r.env).unwrap();
    assert!(r.changed().contains(&("notes.md".to_string(), false, true)));

    operations::stage(
        &r.registry,
        &r.root,
        &["README.md".into(), "notes.md".into()],
        &r.env,
    )
    .unwrap();
    let commit = operations::commit(&r.registry, &r.root, "second", &r.env).unwrap();
    assert_eq!(commit.summary, "second");
    assert!(r.changed().is_empty());

    let log = operations::log(&r.registry, &r.root, 10, None, &r.env).unwrap();
    assert_eq!(
        log.iter().map(|e| e.subject.as_str()).collect::<Vec<_>>(),
        ["second", "initial"]
    );
    let files = operations::commit_files(&r.registry, &r.root, &commit.commit_sha, &r.env).unwrap();
    let mut paths: Vec<_> = files.into_iter().map(|f| f.path).collect();
    paths.sort();
    assert_eq!(paths, ["README.md", "notes.md"]);
    let shown =
        operations::show_commit_diff(&r.registry, &r.root, &commit.commit_sha, &r.env).unwrap();
    assert!(shown.diff_text.contains("+new"));
}

#[test]
fn branches_and_discards_run_on_the_server() {
    let r = repo("git-branch");
    branch::create(&r.registry, &r.root, "draft", None, &r.env).unwrap();
    branch::rename(&r.registry, &r.root, "draft", "feature", &r.env).unwrap();
    let branches = operations::list_branches(&r.registry, &r.root, &r.env).unwrap();
    let names: Vec<_> = branches.branches.iter().map(|b| b.name.as_str()).collect();
    assert!(
        names.contains(&"feature") && names.contains(&"main"),
        "{names:?}"
    );
    operations::checkout_branch(&r.registry, &r.root, "feature", &r.env).unwrap();
    assert_eq!(
        operations::status(&r.registry, &r.root, &r.env)
            .unwrap()
            .branch,
        "feature"
    );
    assert!(branch::upstream_info(&r.registry, &r.root, &r.env).is_ok());

    r.write("README.md", "scribble\n");
    r.write("scratch.txt", "x");
    operations::discard(
        &r.registry,
        &r.root,
        &[
            DiscardEntry {
                path: "README.md".into(),
                untracked: false,
            },
            DiscardEntry {
                path: "scratch.txt".into(),
                untracked: true,
            },
        ],
        &r.env,
    )
    .unwrap();
    assert!(r.changed().is_empty());
    assert_eq!(
        std::fs::read_to_string(format!("{}/README.md", r.root)).unwrap(),
        "# demo\n"
    );
    // A path that climbs out of the repository is refused.
    assert!(operations::stage(
        &r.registry,
        &r.root,
        &["../outside/README.md".into()],
        &r.env
    )
    .is_err());
}

#[test]
fn worktrees_are_listed_and_host_only_features_refuse_by_name() {
    let r = repo("git-host-only");
    let entries = worktree::list(&r.registry, &r.root, &r.env).unwrap();
    assert_eq!(entries.len(), 1);
    assert!(entries[0].exists, "looked up on the server");

    let refused = worktree::remove(
        &r.registry,
        &r.root,
        &format!("{}/wt", r.root),
        false,
        &r.env,
    )
    .unwrap_err()
    .to_string();
    assert!(
        refused.contains("only supported in local workspaces"),
        "{refused}"
    );
    // Pruning runs git on the server; nothing is stale here.
    assert!(worktree::prune(&r.registry, &r.root, &r.env)
        .unwrap()
        .pruned
        .is_empty());
    // Checkpoints are refs, so listing and pruning them work; making and
    // restoring one needs this machine's file system.
    assert!(checkpoint::list(&r.registry, &r.root, "task-1", &r.env)
        .unwrap()
        .is_empty());
    assert_eq!(
        checkpoint::prune(&r.registry, &r.root, "task-1", &r.env).unwrap(),
        0
    );
    let refused = checkpoint::create(&r.registry, &r.root, "task-1", 1, &r.env)
        .unwrap_err()
        .to_string();
    assert!(
        refused.contains("only supported in local workspaces"),
        "{refused}"
    );
    // Tasks keep their worktrees and state on this machine.
    let home = tempfile::tempdir().unwrap();
    let refused = task_worktree::list(&r.registry, &r.root, &r.env, home.path())
        .unwrap_err()
        .to_string();
    assert!(
        refused.contains("only supported in local workspaces"),
        "{refused}"
    );
    let refused = project_config::read(&r.registry, &r.root, &r.env)
        .unwrap_err()
        .to_string();
    assert!(
        refused.contains("only supported in local workspaces"),
        "{refused}"
    );
}

#[test]
fn reviews_and_remotes_run_on_the_server() {
    use crate::modules::git::review::{
        self, ApplyHunksRequest, ApplyMode, ApplySelection, GitDiffScope,
    };

    let r = repo("git-review");
    r.write("README.md", "# demo\nreviewed\n");
    let content =
        operations::diff_content(&r.registry, &r.root, "README.md", false, None, &r.env).unwrap();
    assert_eq!(content.original_content, "# demo\n");
    assert_eq!(content.modified_content, "# demo\nreviewed\n");
    let summary =
        review::diff_summary(&r.registry, &r.root, &GitDiffScope::Uncommitted, &r.env).unwrap();
    assert_eq!(summary.files.len(), 1);
    let file = review::diff_file(
        &r.registry,
        &r.root,
        &GitDiffScope::Unstaged,
        "README.md",
        None,
        true,
        &r.env,
    )
    .unwrap();
    assert!(!file.unchanged);
    // Staging the whole file from the review: the patch goes to git on the
    // server through stdin.
    let outcome = review::apply_hunks(
        &r.registry,
        &r.root,
        &ApplyHunksRequest {
            scope: GitDiffScope::Unstaged,
            path: "README.md".into(),
            old_path: None,
            mode: ApplyMode::Stage,
            selection: ApplySelection::File,
        },
        &r.env,
    )
    .unwrap();
    assert!(outcome.applied, "{:?}", outcome.refusal);
    assert_eq!(r.changed(), [("README.md".to_string(), true, false)]);
    let commit = operations::commit(&r.registry, &r.root, "reviewed", &r.env).unwrap();
    let shown = operations::commit_file_diff(
        &r.registry,
        &r.root,
        &commit.commit_sha,
        "README.md",
        None,
        &r.env,
    )
    .unwrap();
    assert!(shown.modified_content.contains("reviewed"));
    assert!(
        review::merge_base(&r.registry, &r.root, "HEAD", "HEAD~1", &r.env)
            .unwrap()
            .is_some()
    );

    // A remote on the server: fetch, pull and push run there.
    let remote_dir = format!("{}/remote.git", r.root.trim_end_matches("/repo"));
    git(
        Path::new(r.root.trim_end_matches("/repo")),
        &["clone", "-q", "--bare", "repo", "remote.git"],
    );
    git(
        Path::new(&r.root),
        &["remote", "add", "origin", &remote_dir],
    );
    assert_eq!(
        operations::remote_url(&r.registry, &r.root, "origin", &r.env).unwrap(),
        Some(remote_dir.clone())
    );
    operations::fetch(&r.registry, &r.root, &r.env).unwrap();
    r.write("README.md", "# pushed\n");
    operations::stage(&r.registry, &r.root, &["README.md".into()], &r.env).unwrap();
    operations::commit(&r.registry, &r.root, "pushed", &r.env).unwrap();
    operations::push(&r.registry, &r.root, true, &r.env).unwrap();
    operations::pull_ff_only(&r.registry, &r.root, &r.env).unwrap();
    assert_eq!(
        git(Path::new(&remote_dir), &["log", "-1", "--format=%s"]).trim(),
        "pushed"
    );
}

#[test]
// The agent ships for Linux servers. Its watcher (notify) is inotify there
// and ReadDirectoryChangesW in the Windows tests; on macOS notify uses
// FSEvents, which missed this change in CI, and no macOS agent exists.
#[cfg_attr(
    target_os = "macos",
    ignore = "the agent's watcher is not built for macOS"
)]
fn a_commit_made_on_the_server_reaches_source_control() {
    use std::sync::{mpsc, Arc, Mutex};
    use std::time::Duration;

    use super::git_watch::RemoteGitWatches;
    use super::testing::global_registry;
    use super::watch::{resume, Deliver, RemoteWatches, WatchSet};
    use crate::modules::git::utils::authorized_repo_root;
    use crate::modules::git::watch::{remote_rewatch_in, remote_subscribe_in};

    let r = repo("git-watch");
    let WorkspaceEnv::Ssh { host } = &r.env else {
        unreachable!()
    };
    std::fs::create_dir(format!("{}/src", r.root)).unwrap();
    std::fs::write(format!("{}/src/lib.rs", r.root), "").unwrap();
    git(Path::new(&r.root), &["add", "."]);
    git(Path::new(&r.root), &["commit", "-q", "-m", "src"]);

    let git_watches = Arc::new(RemoteGitWatches::default());
    let watches = RemoteWatches::default();
    let (tx, rx) = mpsc::channel();
    let tx = Mutex::new(tx);
    let seen = Arc::clone(&git_watches);
    // What `fs::watch::remote_deliver` does for source control.
    let deliver: Deliver = Arc::new(move |host, paths| {
        for change in seen.changed(host, &paths) {
            let _ = tx.lock().unwrap().send(change);
        }
    });
    let ws = global_registry().workspace(host).unwrap();
    resume(&ws, host, WatchSet::default(), deliver).unwrap();

    let repo = authorized_repo_root(&r.registry, &r.root, &r.env).unwrap();
    let info = remote_subscribe_in(&git_watches, &watches, host, &repo);
    assert!(info.watching);
    assert_eq!(info.repo, super::git_watch::key(host, &r.root));
    assert!(info.watched_dirs >= 3, "root, refs/heads and src: {info:?}");
    let next = || {
        let mut kinds = Vec::new();
        // Changes come in bursts; gather until it is quiet.
        while let Ok((repo, more)) = rx.recv_timeout(Duration::from_millis(1500)) {
            assert_eq!(repo, info.repo);
            kinds.extend(more);
        }
        kinds.sort_unstable();
        kinds.dedup();
        kinds
    };

    // A file added in a tracked folder, outside the app. (Folders report
    // their listing; an edit to a file's contents alone is seen on the
    // panel's focus refresh, or through the editor's own watch.)
    std::fs::write(
        format!("{}/src/main.rs", r.root),
        "fn main() {}
",
    )
    .unwrap();
    assert!(next().contains(&"worktree"));
    // Committed on the server: git writes the index and the branch through
    // lock files, so `.git` and the refs move.
    git(Path::new(&r.root), &["add", "."]);
    git(Path::new(&r.root), &["commit", "-q", "-m", "x"]);
    let kinds = next();
    assert!(
        kinds.contains(&"index") && kinds.contains(&"refs"),
        "{kinds:?}"
    );

    // The last subscriber leaves: the folders are no longer watched.
    let (released_host, paths) = git_watches.unsubscribe(&info.repo).unwrap();
    remote_rewatch_in(&watches, &released_host, &paths, false);
    assert!(watches.set(host).is_empty());
}

#[test]
fn the_explorer_dims_what_git_on_the_server_ignores() {
    use crate::modules::fs::tree::read_dir_entries;

    let r = repo("git-ignored");
    std::fs::write(format!("{}/.gitignore", r.root), "build/\n*.log\n").unwrap();
    std::fs::create_dir(format!("{}/build", r.root)).unwrap();
    std::fs::create_dir(format!("{}/src", r.root)).unwrap();
    std::fs::write(format!("{}/debug.log", r.root), "").unwrap();
    let ignored = |decorations: bool| -> Vec<String> {
        let mut names: Vec<String> =
            read_dir_entries(r.root.clone(), true, Some(decorations), Some(r.env.clone()))
                .unwrap()
                .into_iter()
                .filter(|e| e.gitignored)
                .map(|e| e.name)
                .collect();
        names.sort();
        names
    };
    assert_eq!(ignored(true), ["build", "debug.log"]);
    assert!(ignored(false).is_empty(), "only when asked for");
}
