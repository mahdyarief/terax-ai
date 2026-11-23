//! Every Tauri command that takes a `WorkspaceEnv` says what it does on a
//! server: it works there (and names the test that proves it against the
//! agent), it refuses with a message, or it is still waiting for a phase of
//! the remote plan. A new command, or one that went away, fails this test
//! until it is listed, so none silently builds a path on this machine from a
//! server's.

use std::path::{Path, PathBuf};

enum Ssh {
    /// Works on a server; the test (a function in `remote/*_tests.rs`).
    Works(&'static str),
    /// Refused on a server with a message; why.
    Refused(&'static str),
    /// Not done yet (refused or untested on a server until then); the
    /// phase that brings it.
    Pending(&'static str),
}

use Ssh::*;

const PHASE5: Ssh = Pending("Phase 5: git and terminals on a server");
const PHASE6: Ssh = Pending("Phase 6: AI tools on a server");

/// (file under `src/modules`, command, what it does on a server)
const COMMANDS: &[(&str, &str, Ssh)] = &[
    ("fs/ai.rs", "ai_glob", PHASE6),
    ("fs/ai.rs", "ai_grep", PHASE6),
    ("fs/ai.rs", "ai_read_dir", PHASE6),
    ("fs/ai.rs", "ai_read_file", PHASE6),
    (
        "fs/assets.rs",
        "asset_grant",
        Works("an_image_and_a_pdf_on_the_server_are_served_to_their_view"),
    ),
    (
        "fs/copy.rs",
        "fs_copy",
        Works("files_from_this_machine_are_uploaded_into_the_folder"),
    ),
    (
        "fs/file.rs",
        "fs_canonicalize",
        Works("previews_canonical_paths_and_project_memory_come_from_the_server"),
    ),
    (
        "fs/file.rs",
        "fs_read_binary",
        Works("previews_canonical_paths_and_project_memory_come_from_the_server"),
    ),
    (
        "fs/file.rs",
        "fs_read_bytes",
        Works("previews_canonical_paths_and_project_memory_come_from_the_server"),
    ),
    (
        "fs/file.rs",
        "fs_read_file",
        Works("the_editor_opens_saves_and_sees_a_change_made_on_the_server"),
    ),
    (
        "fs/file.rs",
        "fs_read_head",
        Works("previews_canonical_paths_and_project_memory_come_from_the_server"),
    ),
    (
        "fs/file.rs",
        "fs_stat",
        Works("the_editor_opens_saves_and_sees_a_change_made_on_the_server"),
    ),
    (
        "fs/file.rs",
        "fs_write_file",
        Works("the_editor_opens_saves_and_sees_a_change_made_on_the_server"),
    ),
    (
        "fs/grep.rs",
        "fs_glob",
        Works("grep_and_glob_run_on_the_server"),
    ),
    (
        "fs/grep.rs",
        "fs_grep",
        Works("grep_and_glob_run_on_the_server"),
    ),
    (
        "fs/grep.rs",
        "fs_grep_interactive",
        Works("previews_canonical_paths_and_project_memory_come_from_the_server"),
    ),
    (
        "fs/mutate.rs",
        "fs_create_dir",
        Works("files_are_created_renamed_moved_and_deleted_on_the_server"),
    ),
    (
        "fs/mutate.rs",
        "fs_create_file",
        Works("files_are_created_renamed_moved_and_deleted_on_the_server"),
    ),
    (
        "fs/mutate.rs",
        "fs_delete",
        Works("files_are_created_renamed_moved_and_deleted_on_the_server"),
    ),
    (
        "fs/mutate.rs",
        "fs_delete_batch",
        Works("files_are_created_renamed_moved_and_deleted_on_the_server"),
    ),
    (
        "fs/mutate.rs",
        "fs_move",
        Works("files_are_created_renamed_moved_and_deleted_on_the_server"),
    ),
    (
        "fs/mutate.rs",
        "fs_rename",
        Works("files_are_created_renamed_moved_and_deleted_on_the_server"),
    ),
    (
        "fs/project_memory.rs",
        "fs_read_project_memory",
        Works("previews_canonical_paths_and_project_memory_come_from_the_server"),
    ),
    (
        "fs/search.rs",
        "fs_search",
        Works("the_finder_finds_files_on_the_server_and_leaves_ignored_ones_out"),
    ),
    (
        "fs/search.rs",
        "fs_list_files",
        Works("the_finder_finds_files_on_the_server_and_leaves_ignored_ones_out"),
    ),
    (
        "fs/search.rs",
        "fs_warm_index",
        Works("the_index_sees_our_own_changes_and_rebuilds_after_a_reconnect"),
    ),
    (
        "fs/tree.rs",
        "fs_read_dir",
        Works("the_explorer_lists_a_server_folder"),
    ),
    (
        "fs/tree.rs",
        "list_subdirs",
        Works("the_explorer_lists_a_server_folder"),
    ),
    (
        "fs/watch.rs",
        "fs_watch_add",
        Works("a_watched_server_folder_reports_changes_and_is_watched_again_after_a_reconnect"),
    ),
    (
        "fs/watch.rs",
        "fs_watch_remove",
        Works("a_watched_server_folder_reports_changes_and_is_watched_again_after_a_reconnect"),
    ),
    ("git/commands.rs", "git_checkout_branch", PHASE5),
    ("git/commands.rs", "git_commit", PHASE5),
    ("git/commands.rs", "git_commit_file_diff", PHASE5),
    ("git/commands.rs", "git_commit_files", PHASE5),
    ("git/commands.rs", "git_diff", PHASE5),
    ("git/commands.rs", "git_diff_content", PHASE5),
    ("git/commands.rs", "git_discard", PHASE5),
    ("git/commands.rs", "git_fetch", PHASE5),
    ("git/commands.rs", "git_list_branches", PHASE5),
    ("git/commands.rs", "git_log", PHASE5),
    ("git/commands.rs", "git_panel_snapshot", PHASE5),
    ("git/commands.rs", "git_pull_ff_only", PHASE5),
    ("git/commands.rs", "git_push", PHASE5),
    ("git/commands.rs", "git_remote_url", PHASE5),
    ("git/commands.rs", "git_resolve_repo", PHASE5),
    ("git/commands.rs", "git_show_commit", PHASE5),
    ("git/commands.rs", "git_stage", PHASE5),
    ("git/commands.rs", "git_status", PHASE5),
    ("git/commands.rs", "git_unstage", PHASE5),
    ("git/commands_review.rs", "git_apply_hunks", PHASE5),
    ("git/commands_review.rs", "git_branch_create", PHASE5),
    ("git/commands_review.rs", "git_branch_rename", PHASE5),
    ("git/commands_review.rs", "git_checkpoint_create", PHASE5),
    ("git/commands_review.rs", "git_checkpoint_list", PHASE5),
    ("git/commands_review.rs", "git_checkpoint_prune", PHASE5),
    ("git/commands_review.rs", "git_checkpoint_restore", PHASE5),
    ("git/commands_review.rs", "git_diff_file", PHASE5),
    ("git/commands_review.rs", "git_diff_summary", PHASE5),
    ("git/commands_review.rs", "git_merge_base", PHASE5),
    ("git/commands_review.rs", "git_upstream_info", PHASE5),
    ("git/commands_review.rs", "git_watch", PHASE5),
    ("git/commands_review.rs", "git_worktree_add", PHASE5),
    ("git/commands_review.rs", "git_worktree_list", PHASE5),
    ("git/commands_review.rs", "git_worktree_prune", PHASE5),
    ("git/commands_review.rs", "git_worktree_remove", PHASE5),
    ("git/commands_tasks.rs", "task_project_config", PHASE5),
    ("git/commands_tasks.rs", "task_worktree_archive", PHASE5),
    ("git/commands_tasks.rs", "task_worktree_create", PHASE5),
    ("git/commands_tasks.rs", "task_worktree_disk_usage", PHASE5),
    ("git/commands_tasks.rs", "task_worktree_list", PHASE5),
    (
        "git/commands_tasks.rs",
        "task_worktree_prepare_archive",
        PHASE5,
    ),
    ("git/commands_tasks.rs", "task_worktree_restore", PHASE5),
    (
        "lsp/mod.rs",
        "lsp_spawn",
        Refused("remote LSP is a non-goal of v1: \"lsp: SSH workspaces are not supported\""),
    ),
    ("pty/mod.rs", "pty_open", PHASE5),
    ("shell/mod.rs", "shell_bg_spawn", PHASE6),
    ("shell/mod.rs", "shell_run_command", PHASE6),
    ("shell/mod.rs", "shell_session_open", PHASE6),
    ("shell/mod.rs", "shell_session_run", PHASE6),
    (
        "workspace.rs",
        "workspace_authorize",
        Works("the_explorer_lists_a_server_folder"),
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

/// The commands in `source` whose parameters name `WorkspaceEnv`.
fn commands_taking_an_env(source: &str) -> Vec<String> {
    let lines: Vec<&str> = source.lines().collect();
    let mut found = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if !line.trim_start().starts_with("#[tauri::command") {
            continue;
        }
        let Some(at) = (i + 1..lines.len()).find(|&j| lines[j].contains("fn ")) else {
            continue;
        };
        let head = lines[at];
        let name: String = head[head.find("fn ").unwrap() + 3..]
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        // The signature runs to the line that opens the body.
        let signature: String = lines[at..]
            .iter()
            .take_while(|l| !l.trim_end().ends_with('{'))
            .chain(lines[at..].iter().find(|l| l.trim_end().ends_with('{')))
            .copied()
            .collect();
        if signature.contains("WorkspaceEnv") {
            found.push(name);
        }
    }
    found
}

fn scanned() -> Vec<(String, String)> {
    let root = modules_dir();
    let mut files = Vec::new();
    sources(&root, &mut files);
    let mut found = Vec::new();
    for path in files {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if name.ends_with("_tests.rs") {
            continue;
        }
        let rel = path
            .strip_prefix(&root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let source = std::fs::read_to_string(&path).unwrap();
        for command in commands_taking_an_env(&source) {
            found.push((rel.clone(), command));
        }
    }
    found.sort();
    found
}

fn remote_tests() -> String {
    let mut files = Vec::new();
    sources(&modules_dir().join("remote"), &mut files);
    files
        .into_iter()
        .filter(|p| p.to_string_lossy().ends_with("_tests.rs"))
        .map(|p| std::fs::read_to_string(p).unwrap())
        .collect()
}

#[test]
fn every_command_taking_a_workspace_says_what_it_does_on_a_server() {
    let found = scanned();
    let mut problems = Vec::new();
    for (file, command) in &found {
        if !COMMANDS.iter().any(|(f, c, _)| f == file && c == command) {
            problems.push(format!(
                "{file}: `{command}` takes a WorkspaceEnv; list what it does on a server in COMMANDS"
            ));
        }
    }
    let tests = remote_tests();
    for (file, command, ssh) in COMMANDS {
        if !found.iter().any(|(f, c)| f == file && c == command) {
            problems.push(format!(
                "{file}: `{command}` is not a command taking a WorkspaceEnv any more; remove it"
            ));
        }
        match ssh {
            Works(test) if !tests.contains(&format!("fn {test}(")) => {
                problems.push(format!("{command}: no test `{test}` in remote/*_tests.rs"))
            }
            Refused(why) | Pending(why) if why.len() < 20 => {
                problems.push(format!("{command}: say why"))
            }
            _ => {}
        }
    }
    assert!(problems.is_empty(), "\n{}", problems.join("\n"));
}

#[test]
fn the_scan_finds_commands_by_their_signature() {
    let source = "\
#[tauri::command]
pub async fn takes_it(
    path: String,
    workspace: Option<WorkspaceEnv>,
) -> Result<(), String> {
}
#[tauri::command]
pub fn does_not(path: String) -> String {
    let _ = WorkspaceEnv::Local;
}
#[tauri::command(rename_all = \"snake_case\")]
fn one_line(workspace: WorkspaceEnv) {}
";
    assert_eq!(commands_taking_an_env(source), ["takes_it", "one_line"]);
}
