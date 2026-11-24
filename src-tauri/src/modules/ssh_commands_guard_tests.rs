//! Every Tauri command that takes a `WorkspaceEnv` says what it does on a
//! server: it works there (and names the test that proves it against the
//! agent), or it refuses with a message. A new command, or one that went
//! away, fails this test until it is listed, so none silently builds a path
//! on this machine from a server's.

use std::path::{Path, PathBuf};

enum Ssh {
    /// Works on a server; the test (a function in `remote/*_tests.rs`, or in
    /// `pty/remote.rs` for a server's terminal, which needs the pty
    /// module's own test stand-ins).
    Works(&'static str),
    /// Refused on a server with a message; why.
    Refused(&'static str),
}

use Ssh::*;

/// Worktrees and checkpoints made on this machine's file system (hard
/// links, a copied index, host paths).
const HOST_ONLY_GIT: Ssh =
    Refused("needs this machine's file system: \"only supported in local workspaces\"");
/// Task worktrees, their state and the project's task config live on this
/// machine (`~/.terax`, an OS file lock).
const TASKS: Ssh = Refused("task worktrees are local: \"only supported in local workspaces\"");

/// (file under `src/modules`, command, what it does on a server)
const COMMANDS: &[(&str, &str, Ssh)] = &[
    (
        "fs/ai.rs",
        "ai_glob",
        Works("the_ai_tools_search_and_read_on_the_server_without_its_secrets"),
    ),
    (
        "fs/ai.rs",
        "ai_grep",
        Works("the_ai_tools_search_and_read_on_the_server_without_its_secrets"),
    ),
    (
        "fs/ai.rs",
        "ai_read_dir",
        Works("the_ai_tools_search_and_read_on_the_server_without_its_secrets"),
    ),
    (
        "fs/ai.rs",
        "ai_read_file",
        Works("the_ai_tools_search_and_read_on_the_server_without_its_secrets"),
    ),
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
    (
        "git/commands.rs",
        "git_checkout_branch",
        Works("branches_and_discards_run_on_the_server"),
    ),
    (
        "git/commands.rs",
        "git_commit",
        Works("changes_are_staged_committed_and_shown_in_the_history"),
    ),
    (
        "git/commands.rs",
        "git_commit_file_diff",
        Works("reviews_and_remotes_run_on_the_server"),
    ),
    (
        "git/commands.rs",
        "git_commit_files",
        Works("changes_are_staged_committed_and_shown_in_the_history"),
    ),
    (
        "git/commands.rs",
        "git_diff",
        Works("changes_are_staged_committed_and_shown_in_the_history"),
    ),
    (
        "git/commands.rs",
        "git_diff_content",
        Works("reviews_and_remotes_run_on_the_server"),
    ),
    (
        "git/commands.rs",
        "git_discard",
        Works("branches_and_discards_run_on_the_server"),
    ),
    (
        "git/commands.rs",
        "git_fetch",
        Works("reviews_and_remotes_run_on_the_server"),
    ),
    (
        "git/commands.rs",
        "git_list_branches",
        Works("branches_and_discards_run_on_the_server"),
    ),
    (
        "git/commands.rs",
        "git_log",
        Works("changes_are_staged_committed_and_shown_in_the_history"),
    ),
    (
        "git/commands.rs",
        "git_panel_snapshot",
        Works("the_panel_finds_the_repository_on_the_server"),
    ),
    (
        "git/commands.rs",
        "git_pull_ff_only",
        Works("reviews_and_remotes_run_on_the_server"),
    ),
    (
        "git/commands.rs",
        "git_push",
        Works("reviews_and_remotes_run_on_the_server"),
    ),
    (
        "git/commands.rs",
        "git_remote_url",
        Works("reviews_and_remotes_run_on_the_server"),
    ),
    (
        "git/commands.rs",
        "git_resolve_repo",
        Works("the_panel_finds_the_repository_on_the_server"),
    ),
    (
        "git/commands.rs",
        "git_show_commit",
        Works("changes_are_staged_committed_and_shown_in_the_history"),
    ),
    (
        "git/commands.rs",
        "git_stage",
        Works("changes_are_staged_committed_and_shown_in_the_history"),
    ),
    (
        "git/commands.rs",
        "git_status",
        Works("changes_are_staged_committed_and_shown_in_the_history"),
    ),
    (
        "git/commands.rs",
        "git_unstage",
        Works("changes_are_staged_committed_and_shown_in_the_history"),
    ),
    (
        "git/commands_review.rs",
        "git_apply_hunks",
        Works("reviews_and_remotes_run_on_the_server"),
    ),
    (
        "git/commands_review.rs",
        "git_branch_create",
        Works("branches_and_discards_run_on_the_server"),
    ),
    (
        "git/commands_review.rs",
        "git_branch_rename",
        Works("branches_and_discards_run_on_the_server"),
    ),
    (
        "git/commands_review.rs",
        "git_checkpoint_create",
        HOST_ONLY_GIT,
    ),
    (
        "git/commands_review.rs",
        "git_checkpoint_list",
        Works("worktrees_are_listed_and_host_only_features_refuse_by_name"),
    ),
    (
        "git/commands_review.rs",
        "git_checkpoint_prune",
        Works("worktrees_are_listed_and_host_only_features_refuse_by_name"),
    ),
    (
        "git/commands_review.rs",
        "git_checkpoint_restore",
        HOST_ONLY_GIT,
    ),
    (
        "git/commands_review.rs",
        "git_diff_file",
        Works("reviews_and_remotes_run_on_the_server"),
    ),
    (
        "git/commands_review.rs",
        "git_diff_summary",
        Works("reviews_and_remotes_run_on_the_server"),
    ),
    (
        "git/commands_review.rs",
        "git_merge_base",
        Works("reviews_and_remotes_run_on_the_server"),
    ),
    (
        "git/commands_review.rs",
        "git_upstream_info",
        Works("branches_and_discards_run_on_the_server"),
    ),
    (
        "git/commands_review.rs",
        "git_watch",
        Works("a_commit_made_on_the_server_reaches_source_control"),
    ),
    ("git/commands_review.rs", "git_worktree_add", HOST_ONLY_GIT),
    (
        "git/commands_review.rs",
        "git_worktree_list",
        Works("worktrees_are_listed_and_host_only_features_refuse_by_name"),
    ),
    (
        "git/commands_review.rs",
        "git_worktree_prune",
        Works("worktrees_are_listed_and_host_only_features_refuse_by_name"),
    ),
    (
        "git/commands_review.rs",
        "git_worktree_remove",
        HOST_ONLY_GIT,
    ),
    ("git/commands_tasks.rs", "task_project_config", TASKS),
    ("git/commands_tasks.rs", "task_worktree_archive", TASKS),
    ("git/commands_tasks.rs", "task_worktree_create", TASKS),
    ("git/commands_tasks.rs", "task_worktree_disk_usage", TASKS),
    ("git/commands_tasks.rs", "task_worktree_list", TASKS),
    (
        "git/commands_tasks.rs",
        "task_worktree_prepare_archive",
        TASKS,
    ),
    ("git/commands_tasks.rs", "task_worktree_restore", TASKS),
    (
        "lsp/mod.rs",
        "lsp_spawn",
        Refused("remote LSP is a non-goal of v1: \"lsp: SSH workspaces are not supported\""),
    ),
    (
        "pty/paste_image.rs",
        "pty_paste_image",
        Works("a_pasted_image_is_uploaded_to_the_server"),
    ),
    (
        "pty/mod.rs",
        "pty_open",
        Works("a_server_terminal_runs_through_the_apps_session"),
    ),
    (
        "shell/mod.rs",
        "shell_bg_spawn",
        Works("a_background_process_on_the_server_streams_its_output"),
    ),
    (
        "shell/mod.rs",
        "shell_run_command",
        Works("the_ai_runs_a_command_on_the_server_in_its_folder"),
    ),
    (
        "shell/mod.rs",
        "shell_session_open",
        Works("a_shell_session_on_the_server_keeps_its_folder"),
    ),
    (
        "shell/mod.rs",
        "shell_session_run",
        Works("a_shell_session_on_the_server_keeps_its_folder"),
    ),
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
        .chain([modules_dir().join("pty").join("remote.rs")])
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
            Works(test) if !tests.contains(&format!("fn {test}(")) => problems.push(format!(
                "{command}: no test `{test}` among the server tests"
            )),
            Refused(why) if why.len() < 20 => {
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
