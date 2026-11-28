//! Deny-by-default command policy for `/api/app/invoke`.
//!
//! Every Tauri command in `lib.rs`'s `generate_handler!` must be classified
//! here. Unclassified commands are refused at runtime and fail the guard test.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandClass {
    Read,
    Write,
    Control,
    Secret,
    Native,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transport {
    PlainRemote,
    Loopback,
    Https,
}

/// Classifies an app or Tauri plugin command. Returns `None` for anything
/// unknown (deny by default).
pub fn class(cmd: &str) -> Option<CommandClass> {
    if let Some(plugin_cmd) = cmd.strip_prefix("plugin:") {
        return plugin_class(plugin_cmd);
    }

    match cmd {
        // Web remote
        "remote_access_status" | "remote_devices_list" => Some(CommandClass::Read),
        "remote_report_ports" => Some(CommandClass::Write),
        "remote_access_start"
        | "remote_access_stop"
        | "remote_pairing_start"
        | "remote_device_update"
        | "remote_device_revoke" => Some(CommandClass::Control),
        "remote_publish_tabs" | "remote_relay_push" => Some(CommandClass::Native),

        // PTY
        "pty_diagnostics"
        | "pty_has_foreground_process"
        | "pty_has_foreground_job"
        | "pty_shell_name"
        | "pty_session_shell"
        | "pty_list_shells" => Some(CommandClass::Read),
        "pty_stream_open" | "pty_set_presentation" | "pty_set_colors" => {
            Some(CommandClass::Write)
        }
        "pty_open"
        | "pty_write"
        | "pty_prepare_handoff"
        | "pty_detach"
        | "pty_attach"
        | "pty_commit_handoff"
        | "pty_rollback_handoff"
        | "pty_resize"
        | "pty_close"
        | "pty_close_all"
        | "pty_paste_image" => Some(CommandClass::Control),

        // Filesystem & search
        "list_subdirs"
        | "fs_read_dir"
        | "fs_read_file"
        | "fs_read_binary"
        | "fs_read_bytes"
        | "fs_read_head"
        | "ai_read_file"
        | "ai_read_dir"
        | "ai_grep"
        | "ai_glob"
        | "fs_read_project_memory"
        | "fs_stat"
        | "fs_canonicalize"
        | "fs_search"
        | "fs_warm_index"
        | "fs_list_files"
        | "fs_grep"
        | "fs_grep_interactive"
        | "fs_glob" => Some(CommandClass::Read),
        "asset_grant" | "asset_revoke" | "fs_publish_html" | "fs_drop_html" | "fs_watch_add"
        | "fs_watch_remove" => Some(CommandClass::Write),
        "fs_write_file"
        | "fs_create_file"
        | "fs_create_dir"
        | "fs_rename"
        | "fs_move"
        | "fs_delete"
        | "fs_delete_batch"
        | "fs_upload_file"
        | "fs_copy" => Some(CommandClass::Control),
        "fs_clipboard_paths" => Some(CommandClass::Native),

        // Live server
        "live_server_list" => Some(CommandClass::Read),
        "live_server_start" | "live_server_stop" => Some(CommandClass::Control),

        // LSP
        "lsp_detect" | "lsp_host_pid" | "lsp_resolve_root" => Some(CommandClass::Read),
        "lsp_spawn" | "lsp_send" | "lsp_kill" => Some(CommandClass::Control),

        // Git & tasks
        "git_resolve_repo"
        | "git_panel_snapshot"
        | "git_status"
        | "git_diff"
        | "git_diff_content"
        | "git_log"
        | "git_show_commit"
        | "git_commit_files"
        | "git_commit_file_diff"
        | "git_remote_url"
        | "git_list_branches"
        | "git_merge_base"
        | "git_diff_summary"
        | "git_diff_file"
        | "git_worktree_list"
        | "task_project_config"
        | "task_worktree_list"
        | "task_worktree_disk_usage"
        | "git_checkpoint_list"
        | "git_upstream_info" => Some(CommandClass::Read),
        "git_watch" | "git_unwatch" => Some(CommandClass::Write),
        "git_stage"
        | "git_unstage"
        | "git_discard"
        | "git_commit"
        | "git_fetch"
        | "git_pull_ff_only"
        | "git_push"
        | "git_checkout_branch"
        | "git_apply_hunks"
        | "git_worktree_add"
        | "task_worktree_create"
        | "task_worktree_prepare_archive"
        | "task_worktree_archive"
        | "task_worktree_restore"
        | "git_worktree_remove"
        | "git_worktree_prune"
        | "git_checkpoint_create"
        | "git_checkpoint_restore"
        | "git_checkpoint_prune"
        | "git_branch_create"
        | "git_branch_rename" => Some(CommandClass::Control),

        // Shell
        "shell_bg_logs" | "shell_bg_list" => Some(CommandClass::Read),
        "shell_run_command"
        | "shell_session_open"
        | "shell_session_run"
        | "shell_session_close"
        | "shell_bg_spawn"
        | "shell_bg_kill" => Some(CommandClass::Control),

        // Remote SSH
        "remote_state"
        | "remote_log"
        | "remote_ssh_hosts"
        | "remote_home"
        | "remote_browse_dirs" => Some(CommandClass::Read),
        "remote_connect" | "remote_disconnect" => Some(CommandClass::Control),

        // Workspace
        "wsl_list_distros"
        | "wsl_default_distro"
        | "wsl_home"
        | "workspace_current_dir" => Some(CommandClass::Read),
        "workspace_authorize" => Some(CommandClass::Control),
        "workspace_pick_folder" => Some(CommandClass::Native),

        // Control & agent events
        "control_frontend_ready" => Some(CommandClass::Write),
        "control_respond" | "agent_decisions_ready" | "agent_decision_respond" => {
            Some(CommandClass::Control)
        }

        // Launch & windows
        "get_launch_dir" | "get_launch_files" => Some(CommandClass::Read),
        "open_settings_window" | "open_space_window" | "focus_space_window" => {
            Some(CommandClass::Native)
        }

        // Agent hooks
        "agent_command_available" | "agent_hooks_plan" | "agent_hooks_status" => {
            Some(CommandClass::Read)
        }
        "agent_enable_hooks" | "agent_disable_hooks" | "agent_hooks_apply" => {
            Some(CommandClass::Control)
        }

        // Secrets
        "secrets_get" | "secrets_set" | "secrets_delete" | "secrets_get_all" | "secrets_has" => {
            Some(CommandClass::Secret)
        }

        // Net & AI sessions
        "lm_ping" | "ai_http_stream" | "ai_http_cancel" | "ai_http_ack" => {
            Some(CommandClass::Control)
        }
        "ai_session_load" => Some(CommandClass::Read),
        "ai_session_save"
        | "ai_session_delete"
        | "ai_session_finish_migration"
        | "ai_session_prune_backup" => Some(CommandClass::Write),

        // History
        "history_suggest" | "history_commands" | "history_list" => Some(CommandClass::Read),
        "history_record" => Some(CommandClass::Write),

        // Native-only UI & bench
        "window_backdrop_kind"
        | "window_set_backdrop"
        | "window_presentation_state"
        | "bench_config"
        | "bench_window"
        | "bench_exit" => Some(CommandClass::Native),

        _ => None,
    }
}

fn plugin_class(plugin_cmd: &str) -> Option<CommandClass> {
    let (plugin, action) = plugin_cmd.split_once('|')?;
    match plugin {
        "event" => match action {
            "listen" | "unlisten" | "emit" | "emit_to" => Some(CommandClass::Write),
            _ => None,
        },
        "store" => match action {
            "load" | "get_store" | "get" | "has" | "keys" | "values" | "entries" | "length" => {
                Some(CommandClass::Read)
            }
            "set" | "delete" | "clear" | "reset" | "save" | "reload" => Some(CommandClass::Write),
            _ => None,
        },
        "path" => match action {
            "resolve_directory"
            | "resolve"
            | "normalize"
            | "join"
            | "dirname"
            | "extname"
            | "basename"
            | "is_absolute" => Some(CommandClass::Read),
            _ => None,
        },
        "os" => match action {
            "platform" | "version" | "os_type" | "family" | "arch" | "exe_extension"
            | "locale" | "hostname" => Some(CommandClass::Read),
            _ => None,
        },
        "app" => match action {
            "version" | "name" | "tauri_version" | "identifier" | "bundle_type" => {
                Some(CommandClass::Read)
            }
            "app_show"
            | "app_hide"
            | "exit"
            | "fetch_data_store_identifiers"
            | "remove_data_store"
            | "default_window_icon"
            | "set_app_theme"
            | "set_dock_visibility" => Some(CommandClass::Native),
            _ => None,
        },
        "core"
        | "window"
        | "webview"
        | "updater"
        | "autostart"
        | "process"
        | "opener"
        | "clipboard-manager"
        | "notification"
        | "window-state"
        | "log"
        | "resources" => Some(CommandClass::Native),
        _ => None,
    }
}

/// Whether `cmd` may be forwarded from the browser for a device with
/// `app_grant` over `transport`.
pub fn allowed(cmd: &str, app_grant: bool, transport: Transport) -> bool {
    let Some(c) = class(cmd) else {
        return false;
    };
    allowed_class(c, cmd, app_grant, transport)
}

pub fn allowed_class(c: CommandClass, cmd: &str, app_grant: bool, transport: Transport) -> bool {
    if !app_grant {
        return false;
    }
    match c {
        CommandClass::Native => false,
        CommandClass::Secret => {
            if matches!(cmd, "secrets_get" | "secrets_get_all") {
                return false;
            }
            matches!(transport, Transport::Loopback | Transport::Https)
        }
        CommandClass::Read | CommandClass::Write | CommandClass::Control => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn extract_generate_handler_commands(lib_rs: &str) -> Vec<String> {
        let marker = "tauri::generate_handler![";
        let start = lib_rs
            .find(marker)
            .expect("tauri::generate_handler![ not found in lib.rs")
            + marker.len();
        let rest = &lib_rs[start..];
        let end = rest.find("])").expect("closing ]) not found for generate_handler!");
        let block = &rest[..end];
        let mut cmds = Vec::new();
        for raw_line in block.lines() {
            let line = raw_line.split("//").next().unwrap_or("").trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let entry = line.trim_end_matches(',').trim();
            if entry.is_empty() {
                continue;
            }
            let name = entry.rsplit("::").next().unwrap_or(entry).trim();
            if !name.is_empty() {
                cmds.push(name.to_string());
            }
        }
        cmds
    }

    #[test]
    fn every_command_in_generate_handler_is_classified() {
        let lib_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("src")
            .join("lib.rs");
        let lib_rs = std::fs::read_to_string(&lib_path).unwrap();
        let commands = extract_generate_handler_commands(&lib_rs);
        assert!(
            commands.len() > 100,
            "expected >100 commands in generate_handler!, got {}",
            commands.len()
        );
        let unclassified: Vec<String> = commands
            .into_iter()
            .filter(|cmd| class(cmd).is_none())
            .collect();
        assert!(
            unclassified.is_empty(),
            "unclassified commands in generate_handler!: {}",
            unclassified.join(", ")
        );
    }

    #[test]
    fn deny_by_default_and_grant_rules() {
        assert_eq!(class("nonexistent_cmd"), None);
        assert!(!allowed("nonexistent_cmd", true, Transport::Loopback));
        assert!(!allowed("nonexistent_cmd", true, Transport::Https));

        // Without app grant, nothing is allowed.
        assert!(!allowed("pty_list_shells", false, Transport::Loopback));
        assert!(!allowed("plugin:store|get", false, Transport::Https));

        // Native commands are never allowed.
        assert!(!allowed("window_set_backdrop", true, Transport::Loopback));
        assert!(!allowed("open_settings_window", true, Transport::Https));
        assert!(!allowed("plugin:window|close", true, Transport::Loopback));
        assert!(!allowed("plugin:core|__FETCH_CHANNEL_DATA__", true, Transport::Loopback));

        // Secrets require HTTPS or loopback, and secrets_get / secrets_get_all are never allowed.
        assert!(!allowed("secrets_set", true, Transport::PlainRemote));
        assert!(allowed("secrets_set", true, Transport::Loopback));
        assert!(allowed("secrets_has", true, Transport::Https));
        assert!(!allowed("secrets_has", true, Transport::PlainRemote));
        assert!(!allowed("secrets_get", true, Transport::Loopback));
        assert!(!allowed("secrets_get", true, Transport::Https));
        assert!(!allowed("secrets_get_all", true, Transport::Loopback));
        assert!(!allowed("secrets_get_all", true, Transport::Https));

        // Read/Write/Control are allowed with app grant on any transport.
        assert!(allowed("pty_list_shells", true, Transport::PlainRemote));
        assert!(allowed("plugin:store|set", true, Transport::PlainRemote));
        assert!(allowed("git_commit", true, Transport::PlainRemote));
    }
}
