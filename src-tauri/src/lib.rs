pub mod modules;

#[cfg(target_os = "macos")]
use modules::app_menu;
use modules::{
    agent, control, fs, git, history, lsp, net, pty, secrets, shell, vibrancy, workspace,
};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder, WindowEvent};
#[cfg(target_os = "macos")]
use tauri::PhysicalPosition;
use tauri_plugin_window_state::StateFlags;

/// Drained on first read so HMR / re-mounts can't replay the launch dir.
#[derive(Default)]
struct LaunchDir(Mutex<Option<String>>);

/// Drained on first read so HMR / re-mounts can't replay the launch files.
#[derive(Default)]
struct LaunchFiles(Mutex<Vec<String>>);

#[tauri::command]
fn get_launch_dir(state: State<'_, LaunchDir>) -> Option<String> {
    state.0.lock().expect("LaunchDir mutex poisoned").take()
}

#[tauri::command]
fn get_launch_files(state: State<'_, LaunchFiles>) -> Vec<String> {
    std::mem::take(&mut *state.0.lock().expect("LaunchFiles mutex poisoned"))
}

enum LaunchEntry {
    Dir(PathBuf),
    File(PathBuf),
}

#[derive(Default, Debug, PartialEq)]
struct LaunchTarget {
    dir: Option<String>,
    files: Vec<String>,
}

/// First dir arg (else the first file's parent) becomes the workspace; every
/// file arg is opened. Kept free of fs/env access so it stays unit-testable.
fn resolve_launch_target(entries: Vec<LaunchEntry>) -> LaunchTarget {
    let mut dir = None;
    let mut files = Vec::new();
    for entry in entries {
        match entry {
            LaunchEntry::Dir(path) => {
                if dir.is_none() {
                    dir = Some(fs::to_canon(&path));
                }
            }
            LaunchEntry::File(path) => {
                if dir.is_none() {
                    dir = path.parent().map(fs::to_canon);
                }
                files.push(fs::to_canon(&path));
            }
        }
    }
    LaunchTarget { dir, files }
}

fn parse_launch_target() -> LaunchTarget {
    let entries = std::env::args()
        .skip(1)
        .filter(|arg| !arg.starts_with('-'))
        .filter_map(|arg| std::fs::canonicalize(arg).ok())
        .filter_map(|path| {
            let meta = std::fs::metadata(&path).ok()?;
            Some(if meta.is_dir() {
                LaunchEntry::Dir(path)
            } else {
                LaunchEntry::File(path)
            })
        })
        .collect();
    resolve_launch_target(entries)
}

const fn settings_always_on_top(is_macos: bool) -> bool {
    !is_macos
}

const SPACE_WINDOW_PREFIX: &str = "space-";
const REMOTE_WINDOW_PREFIX: &str = "remote-";

fn is_remote_relay_label(label: &str) -> bool {
    label.starts_with(REMOTE_WINDOW_PREFIX)
}

fn space_window_label(space_id: &str) -> String {
    format!("{SPACE_WINDOW_PREFIX}{space_id}")
}

fn space_id_from_label(label: &str) -> Option<String> {
    label
        .strip_prefix(SPACE_WINDOW_PREFIX)
        .map(str::to_string)
}

fn can_open_space_window(label: &str) -> bool {
    label == "main"
}

#[tauri::command]
async fn focus_space_window(app: tauri::AppHandle, space_id: String) -> Result<(), String> {
    let label = space_window_label(&space_id);
    let window = app
        .get_webview_window(&label)
        .ok_or_else(|| "space window is not open".to_string())?;
    window.set_focus().map_err(|error| error.to_string())
}

/// Opens (or focuses) the window dedicated to one space. Idempotent so a
/// double click can't spawn two windows for the same space.
#[tauri::command]
async fn open_space_window(
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
    space_id: String,
) -> Result<(), String> {
    if !can_open_space_window(window.label()) {
        return Err("space windows can only be opened from the main window".into());
    }
    let label = space_window_label(&space_id);

    if let Some(window) = app.get_webview_window(&label) {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        return Ok(());
    }

    let url = format!("index.html?windowKind=space&spaceId={space_id}");
    let builder = WebviewWindowBuilder::new(&app, &label, WebviewUrl::App(url.into()))
        .title("Terax")
        .inner_size(1100.0, 760.0)
        .min_inner_size(720.0, 480.0)
        .resizable(true)
        // The frontend shows the window after the first paint, matching main.
        .visible(false);

    #[cfg(target_os = "macos")]
    let builder = builder
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .hidden_title(true);

    #[cfg(any(target_os = "linux", target_os = "windows"))]
    let builder = builder.decorations(false).transparent(true);

    #[cfg_attr(windows, allow(unused_variables))]
    let window = builder.build().map_err(|e| e.to_string())?;

    #[cfg(target_os = "linux")]
    {
        let _ = window.set_decorations(false);
    }

    Ok(())
}

#[tauri::command]
async fn open_settings_window(app: tauri::AppHandle, tab: Option<String>) -> Result<(), String> {
    let url_path = match tab.as_deref() {
        Some(t) if !t.is_empty() => format!("settings.html?tab={}", t),
        _ => "settings.html".to_string(),
    };

    if let Some(window) = app.get_webview_window("settings") {
        #[cfg(not(target_os = "macos"))]
        let _ = window.set_always_on_top(true);
        let _ = window.show();
        let _ = window.set_focus();
        if let Some(t) = tab.as_deref().filter(|s| !s.is_empty()) {
            // emit() serializes via JSON — no string-escape footgun, unlike
            // eval() with format!(). Frontend listens via Tauri event API.
            let _ = window.emit("terax:settings-tab", t);
        }
        return Ok(());
    }

    let builder = WebviewWindowBuilder::new(&app, "settings", WebviewUrl::App(url_path.into()))
        .title("Settings")
        .inner_size(900.0, 700.0)
        .min_inner_size(820.0, 620.0)
        .resizable(true)
        .visible(false)
        .always_on_top(settings_always_on_top(cfg!(target_os = "macos")));

    // A normal-level child stays above Terax but recedes with the app on macOS.
    // Never combine the macOS parent with always_on_top; that breaks WebView
    // compositing and can hide Settings behind the main window (#33, #957).
    let builder = if let Some(main) = app.get_webview_window("main") {
        builder.parent(&main).map_err(|e| e.to_string())?
    } else {
        builder
    };

    #[cfg(target_os = "macos")]
    let builder = builder
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .hidden_title(true);

    // On Linux/Windows we render our own titlebar, so drop native chrome
    // and make the window transparent.
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    let builder = builder.decorations(false).transparent(true);

    #[cfg_attr(windows, allow(unused_variables))]
    let window = builder.build().map_err(|e| e.to_string())?;

    // Some Linux compositors (GNOME/Mutter with CSD-by-default) ignore the
    // builder-time decorations flag — re-assert it after realize.
    #[cfg(target_os = "linux")]
    {
        let _ = window.set_decorations(false);
    }

    #[cfg(target_os = "macos")]
    if let Some(main) = app.get_webview_window("main") {
        if let (Ok(main_pos), Ok(main_size), Ok(settings_size)) = (
            main.outer_position(),
            main.outer_size(),
            window.outer_size(),
        ) {
            let x = main_pos.x
                + ((main_size.width as i32).saturating_sub(settings_size.width as i32)) / 2;
            let y = main_pos.y
                + ((main_size.height as i32).saturating_sub(settings_size.height as i32)) / 2;
            let _ = window.set_position(PhysicalPosition::new(x, y));
        } else {
            let _ = window.center();
        }
    }

    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[cfg(windows)]
    {
        let args: Vec<String> = std::env::args().collect();
        if args.get(1).map(String::as_str) == Some("__terax_notify") {
            if let (Some(agent), Some(event)) = (args.get(2), args.get(3)) {
                agent::emit_conout_marker(agent, event);
            }
            use std::io::Write;
            let mut out = std::io::stdout();
            let _ = out.write_all(b"{}");
            let _ = out.flush();
            std::process::exit(0);
        }
    }

    modules::runtime::install();

    let launch = parse_launch_target();
    let serve = modules::web_remote::parse_serve_args(std::env::args().skip(1));
    let cli_dir = launch.dir.clone();
    workspace::init_launch_cwd(cli_dir.as_deref());
    let control_state = control::ControlState::default();
    let control_for_setup = control_state.clone();

    let builder = tauri::Builder::default();
    let builder = builder.plugin(tauri_plugin_clipboard_manager::init());
    #[cfg(target_os = "macos")]
    let builder = builder
        .menu(app_menu::build)
        .on_menu_event(app_menu::handle_event);
    builder
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        // Skip restoring VISIBLE — frontend calls window.show() after first
        // paint so the user never sees a transparent window-shadow flash on
        // Windows/Linux.
        .plugin(
            tauri_plugin_window_state::Builder::new()
                .with_state_flags(StateFlags::all() & !StateFlags::VISIBLE)
                .build(),
        )
        .plugin(tauri_plugin_autostart::Builder::new().build())
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(tauri_plugin_log::log::LevelFilter::Info)
                .build(),
        )
        .plugin(tauri_plugin_opener::init())
        // One async scheme multiplexes every terminal's output; see pty/hub.rs.
        .register_asynchronous_uri_scheme_protocol("terax-pty", pty::stream_protocol)
        .register_asynchronous_uri_scheme_protocol("asset", fs::assets::serve)
        .register_asynchronous_uri_scheme_protocol("terax-doc", fs::preview::serve)
        .on_page_load(|webview, payload| {
            if matches!(payload.event(), tauri::webview::PageLoadEvent::Started) {
                if let Some(grants) = webview.try_state::<fs::assets::AssetGrants>() { grants.revoke_owner(webview.label()); }
                if let Some(state) = webview.try_state::<net::AiStreamState>() {
                    state.cancel_window(webview.window().label());
                }
                if let Some(state) = webview.try_state::<pty::PtyState>() {
                    state.reset_webview(webview.label());
                }
                // The reloaded page has no decision UI until it announces one.
                if let Some(state) = webview.try_state::<control::ControlState>() {
                    state
                        .hooks()
                        .broker
                        .set_window_ready(webview.label(), false);
                    state.control_frontend_ready(webview.label(), false);
                }
            }
        })
        .setup(move |_app| {
            #[cfg(feature = "bench")]
            modules::bench::guard(_app.handle())?;
            #[cfg(target_os = "macos")]
            modules::window_presentation::macos::install(_app.handle());
            if let Some(remote) = _app.try_state::<modules::web_remote::WebRemoteState>() {
                remote.attach_device_store(modules::web_remote::devices::store_path(
                    _app.handle(),
                ));
            }
            if serve.enabled {
                // Nobody is at this machine: the browser is the only window.
                if let Some(main) = _app.get_webview_window("main") {
                    let _ = main.hide();
                }
                let handle = _app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    let state = handle.state::<modules::web_remote::WebRemoteState>();
                    match modules::web_remote::start_listener(
                        &handle, &state, serve.bind, serve.port, false, false,
                    )
                    .await
                    {
                        Ok(info) => {
                            log::info!(
                                "remote serve listening on {} for {}",
                                info.port.unwrap_or(0),
                                info.urls.first().cloned().unwrap_or_default()
                            );
                            // No window can show a pairing code here, so the
                            // first one goes to stderr (the user's journal).
                            if let Ok(pairing) = state.start_pairing() {
                                eprintln!(
                                    "terax: pair a browser within {} minutes: {}",
                                    pairing.expires_in_sec / 60,
                                    pairing.url
                                );
                            }
                        }
                        Err(error) => log::error!("remote serve could not start: {error}"),
                    }
                });
            }
            if let Err(error) = control::start(_app.handle().clone(), control_for_setup.clone()) {
                log::warn!("could not start Terax control server: {error}");
            }
            modules::remote::commands::install(_app.handle(), control_for_setup.clone());
            if let Some(main) = _app.get_webview_window("main") {
                let handle = _app.handle().clone();
                main.on_window_event(move |event| {
                    // CloseRequested can be cancelled by the frontend guard.
                    if matches!(event, WindowEvent::Destroyed) {
                        if let Some(settings) = handle.get_webview_window("settings") {
                            let _ = settings.destroy();
                        }
                        // Closing main ends the session: tear down every space
                        // window too. Their own Destroyed events run the
                        // per-label PTY cleanup, so nothing is handled here.
                        let labels: Vec<String> = handle
                            .webview_windows()
                            .keys()
                            .filter(|label| label.starts_with("space-"))
                            .cloned()
                            .collect();
                        for label in &labels {
                            if let Some(window) = handle.get_webview_window(label) {
                                let _ = window.destroy();
                            }
                        }
                    }
                });
            }
            Ok(())
        })
        .manage(pty::PtyState::default())
        .manage(net::AiStreamState::default())
        .manage(modules::ai_sessions::SessionFilesState::default())
        .manage(modules::window_presentation::WindowPresentationState::default())
        .manage(control_state)
        .manage(shell::ShellState::default())
        .manage(secrets::SecretsState::default())
        .manage(fs::watch::FsWatchState::default())
        .manage(modules::remote::watch::RemoteWatches::default())
        .manage(modules::remote::git_watch::RemoteGitWatches::default())
        .manage(fs::assets::AssetGrants::default())
        .manage(fs::preview::HtmlPreviews::default())
        .manage(modules::live_server::LiveServers::default())
        .manage(modules::web_remote::WebRemoteState::default())
        .manage(git::watch::GitWatchState::default())
        .manage(history::HistoryState::default())
        .manage(lsp::LspState::default())
        .manage(fs::supersede::Supersede::default())
        .manage(fs::index::FileIndexState::default())
        .manage({
            let registry = workspace::WorkspaceRegistry::default();
            workspace::bootstrap_registry(&registry);
            if let Some(ref launch_dir) = cli_dir {
                let _ = registry.authorize(launch_dir);
            }
            registry
        })
        .manage(LaunchDir(Mutex::new(cli_dir)))
        .manage(LaunchFiles(Mutex::new(launch.files)))
        .invoke_handler(tauri::generate_handler![
            modules::web_remote::remote_publish_tabs,
            modules::web_remote::remote_access_start,
            modules::web_remote::remote_access_stop,
            modules::web_remote::remote_access_status,
            modules::web_remote::remote_pairing_start,
            modules::web_remote::remote_devices_list,
            modules::web_remote::remote_device_update,
            modules::web_remote::remote_device_revoke,
            modules::web_remote::app::sessions::remote_relay_push,
            pty::pty_open,
            pty::pty_write,
            pty::pty_stream_open,
            pty::pty_prepare_handoff,
            pty::pty_detach,
            pty::pty_attach,
            pty::pty_commit_handoff,
            pty::pty_rollback_handoff,
            pty::pty_set_presentation,
            pty::pty_set_colors,
            pty::pty_diagnostics,
            pty::pty_resize,
            pty::pty_close,
            pty::pty_close_all,
            pty::pty_has_foreground_process,
            pty::pty_has_foreground_job,
            pty::pty_shell_name,
            pty::pty_session_shell,
            pty::pty_list_shells,
            fs::tree::list_subdirs,
            fs::tree::fs_read_dir,
            fs::file::fs_read_file,
            fs::file::fs_read_binary,
            fs::file::fs_read_bytes,
            fs::file::fs_read_head,
            fs::ai::ai_read_file,
            fs::ai::ai_read_dir,
            fs::ai::ai_grep,
            fs::ai::ai_glob,
            fs::assets::asset_grant,
            fs::assets::asset_revoke,
            fs::preview::fs_publish_html,
            fs::preview::fs_drop_html,
            modules::live_server::live_server_start,
            modules::live_server::live_server_stop,
            modules::live_server::live_server_list,
            fs::project_memory::fs_read_project_memory,
            fs::file::fs_write_file,
            fs::file::fs_stat,
            fs::file::fs_canonicalize,
            fs::mutate::fs_create_file,
            fs::mutate::fs_create_dir,
            fs::mutate::fs_rename,
            fs::mutate::fs_move,
            fs::mutate::fs_delete,
            fs::mutate::fs_delete_batch,
            fs::mutate::fs_upload_file,
            fs::copy::fs_copy,
            fs::clipboard::fs_clipboard_paths,
            pty::paste_image::pty_paste_image,
            fs::watch::fs_watch_add,
            fs::watch::fs_watch_remove,
            lsp::lsp_detect,
            lsp::lsp_host_pid,
            lsp::lsp_resolve_root,
            lsp::lsp_spawn,
            lsp::lsp_send,
            lsp::lsp_kill,
            fs::search::fs_search,
            fs::search::fs_warm_index,
            fs::search::fs_list_files,
            fs::grep::fs_grep,
            fs::grep::fs_grep_interactive,
            fs::grep::fs_glob,
            git::commands::git_resolve_repo,
            git::commands::git_panel_snapshot,
            git::commands::git_status,
            git::commands::git_diff,
            git::commands::git_diff_content,
            git::commands::git_stage,
            git::commands::git_unstage,
            git::commands::git_discard,
            git::commands::git_commit,
            git::commands::git_fetch,
            git::commands::git_pull_ff_only,
            git::commands::git_push,
            git::commands::git_log,
            git::commands::git_show_commit,
            git::commands::git_commit_files,
            git::commands::git_commit_file_diff,
            git::commands::git_remote_url,
            git::commands::git_list_branches,
            git::commands::git_checkout_branch,
            git::commands_review::git_merge_base,
            git::commands_review::git_diff_summary,
            git::commands_review::git_diff_file,
            git::commands_review::git_apply_hunks,
            git::commands_review::git_worktree_list,
            git::commands_review::git_worktree_add,
            git::commands_tasks::task_project_config,
            git::commands_tasks::task_worktree_list,
            git::commands_tasks::task_worktree_create,
            git::commands_tasks::task_worktree_prepare_archive,
            git::commands_tasks::task_worktree_archive,
            git::commands_tasks::task_worktree_restore,
            git::commands_tasks::task_worktree_disk_usage,
            git::commands_review::git_worktree_remove,
            git::commands_review::git_worktree_prune,
            git::commands_review::git_checkpoint_create,
            git::commands_review::git_checkpoint_list,
            git::commands_review::git_checkpoint_restore,
            git::commands_review::git_checkpoint_prune,
            git::commands_review::git_branch_create,
            git::commands_review::git_branch_rename,
            git::commands_review::git_upstream_info,
            git::commands_review::git_watch,
            git::commands_review::git_unwatch,
            shell::shell_run_command,
            shell::shell_session_open,
            shell::shell_session_run,
            shell::shell_session_close,
            shell::shell_bg_spawn,
            shell::shell_bg_logs,
            shell::shell_bg_kill,
            shell::shell_bg_list,
            modules::remote::commands::remote_connect,
            modules::remote::commands::remote_disconnect,
            modules::remote::commands::remote_state,
            modules::remote::commands::remote_log,
            modules::remote::commands::remote_ssh_hosts,
            modules::remote::commands::remote_home,
            modules::remote::commands::remote_browse_dirs,
            workspace::wsl_list_distros,
            workspace::wsl_default_distro,
            workspace::ipc::wsl_home,
            workspace::workspace_authorize,
            workspace::workspace_current_dir,
            workspace::workspace_pick_folder,
            control::control_frontend_ready,
            control::control_respond,
            modules::agent_events::agent_decisions_ready,
            modules::agent_events::agent_decision_respond,
            get_launch_dir,
            get_launch_files,
            open_settings_window,
            open_space_window,
            focus_space_window,
            agent::agent_enable_hooks,
            modules::agent_command::agent_command_available,
            agent::agent_disable_hooks,
            agent::agent_hooks_plan,
            agent::agent_hooks_apply,
            agent::agent_hooks_status,
            secrets::secrets_get,
            secrets::secrets_set,
            secrets::secrets_delete,
            secrets::secrets_get_all,
            secrets::secrets_has,
            net::lm_ping,
            net::ai_http_stream,
            net::ai_http_cancel,
            net::ai_http_ack,
            modules::ai_sessions::ai_session_load,
            modules::ai_sessions::ai_session_save,
            modules::ai_sessions::ai_session_delete,
            modules::ai_sessions::ai_session_finish_migration,
            modules::ai_sessions::ai_session_prune_backup,
            history::history_suggest,
            history::history_commands,
            history::history_record,
            history::history_list,
            vibrancy::window_backdrop_kind,
            vibrancy::window_set_backdrop,
            modules::window_presentation::window_presentation_state,
            #[cfg(feature = "bench")]
            modules::bench::bench_config,
            #[cfg(feature = "bench")]
            modules::bench::bench_window,
            #[cfg(feature = "bench")]
            modules::bench::bench_exit,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            match event {
                tauri::RunEvent::WindowEvent { label, event, .. } => match event {
                    tauri::WindowEvent::Focused(true) => {
                        if let Some(state) = app.try_state::<control::ControlState>() {
                            state.note_window_focus(&label);
                        }
                    }
                    tauri::WindowEvent::Destroyed => {
                        if let Some(grants) = app.try_state::<fs::assets::AssetGrants>() { grants.revoke_owner(&label); }
                        if let Some(state) = app.try_state::<net::AiStreamState>() {
                            state.cancel_window(&label);
                        }
                        if let Some(state) = app.try_state::<pty::PtyState>() {
                            state.destroy_webview(&label);
                        }
                        // A closed window's tabs leave the browser's list at once;
                        // its PTYs may still be alive until the window's own cleanup.
                        modules::web_remote::forget_window(app, &label);
                        if let Some(state) = app.try_state::<control::ControlState>() {
                            state.hooks().broker.set_window_ready(&label, false);
                            state.control_frontend_ready(&label, false);
                        }
                        if let Some(space_id) = space_id_from_label(&label) {
                            let now = SystemTime::now()
                                .duration_since(UNIX_EPOCH)
                                .map(|d| d.as_millis() as i64)
                                .unwrap_or(0);
                            let _ = app.emit(
                                "spaces:host-changed",
                                serde_json::json!({
                                    "spaceId": space_id,
                                    "hostWindowLabel": "main",
                                    "updatedAt": now,
                                    "reason": "window-closed",
                                }),
                            );
                        }
                    }
                    _ => {}
                },
                // Servers exit on stdin EOF, but destructors are not guaranteed
                // on process exit; kill explicitly.
                tauri::RunEvent::Exit => {
                    #[cfg(target_os = "macos")]
                    modules::window_presentation::macos::uninstall();
                    if let Some(state) = app.try_state::<lsp::LspState>() {
                        state.kill_all();
                    }
                    if let Some(state) = app.try_state::<control::ControlState>() {
                        state.shutdown();
                    }
                }
                // macOS delivers "Open With" files here, not as argv (cold and
                // warm start, several at once). Seed the drain-once state and
                // emit; canonicalize so the /tmp -> /private/tmp symlink can't
                // defeat openFileTab's path dedupe against a CLI launch.
                #[cfg(target_os = "macos")]
                tauri::RunEvent::Opened { urls } => {
                    let entries = urls
                        .iter()
                        .filter_map(|u| u.to_file_path().ok())
                        .filter_map(|p| std::fs::canonicalize(p).ok())
                        .filter(|p| p.is_file())
                        .map(LaunchEntry::File)
                        .collect();
                    let target = resolve_launch_target(entries);
                    if target.files.is_empty() {
                        return;
                    }
                    if let Some(dir) = &target.dir {
                        if let Some(registry) = app.try_state::<workspace::WorkspaceRegistry>() {
                            let _ = registry.authorize(dir);
                        }
                        if let Some(state) = app.try_state::<LaunchDir>() {
                            *state.0.lock().expect("LaunchDir mutex poisoned") = Some(dir.clone());
                        }
                    }
                    if let Some(state) = app.try_state::<LaunchFiles>() {
                        *state.0.lock().expect("LaunchFiles mutex poisoned") = target.files.clone();
                    }
                    let _ = app.emit("terax:open-file", target.files);
                }
                _ => {}
            }
        });
}

#[cfg(test)]
mod launch_target_tests {
    use super::{resolve_launch_target, settings_always_on_top, LaunchEntry, LaunchTarget};
    use std::path::PathBuf;

    #[test]
    fn no_entries_resolves_to_empty() {
        assert_eq!(resolve_launch_target(vec![]), LaunchTarget::default());
    }

    #[test]
    fn dir_arg_sets_workspace_and_opens_nothing() {
        let out = resolve_launch_target(vec![LaunchEntry::Dir(PathBuf::from("/home/u/proj"))]);
        assert_eq!(out.dir.as_deref(), Some("/home/u/proj"));
        assert!(out.files.is_empty());
    }

    #[test]
    fn file_arg_opens_file_and_uses_parent_as_workspace() {
        let out = resolve_launch_target(vec![LaunchEntry::File(PathBuf::from(
            "/home/u/proj/main.rs",
        ))]);
        assert_eq!(out.dir.as_deref(), Some("/home/u/proj"));
        assert_eq!(out.files, vec!["/home/u/proj/main.rs".to_string()]);
    }

    #[test]
    fn multiple_files_all_open_and_first_parent_wins() {
        let out = resolve_launch_target(vec![
            LaunchEntry::File(PathBuf::from("/a/one.txt")),
            LaunchEntry::File(PathBuf::from("/b/two.txt")),
        ]);
        assert_eq!(out.dir.as_deref(), Some("/a"));
        assert_eq!(
            out.files,
            vec!["/a/one.txt".to_string(), "/b/two.txt".to_string()]
        );
    }

    #[test]
    fn explicit_dir_takes_precedence_over_file_parent() {
        let out = resolve_launch_target(vec![
            LaunchEntry::Dir(PathBuf::from("/workspace")),
            LaunchEntry::File(PathBuf::from("/other/x.rs")),
        ]);
        assert_eq!(out.dir.as_deref(), Some("/workspace"));
        assert_eq!(out.files, vec!["/other/x.rs".to_string()]);
    }

    #[test]
    fn settings_float_only_outside_macos() {
        assert!(!settings_always_on_top(true));
        assert!(settings_always_on_top(false));
    }
}

#[cfg(test)]
mod space_window_tests {
    use super::{
        can_open_space_window, is_remote_relay_label, space_id_from_label, space_window_label,
    };

    #[test]
    fn space_window_label_round_trips() {
        assert_eq!(space_window_label("abc"), "space-abc");
        assert_eq!(space_id_from_label("space-abc"), Some("abc".to_string()));
        assert_eq!(space_id_from_label("main"), None);
    }

    #[test]
    fn space_id_from_label_ignores_non_space_labels() {
        assert_eq!(space_id_from_label("settings"), None);
        assert_eq!(space_id_from_label("space-"), Some(String::new()));
    }

    #[test]
    fn only_main_can_open_space_windows() {
        assert!(can_open_space_window("main"));
        assert!(!can_open_space_window("space-abc"));
        assert!(!can_open_space_window("settings"));
        assert!(!can_open_space_window("remote-0"));
    }

    #[test]
    fn remote_relay_labels_are_recognized_and_not_spaces() {
        assert!(is_remote_relay_label("remote-0"));
        assert!(!is_remote_relay_label("main"));
        assert!(!is_remote_relay_label("space-abc"));
        assert_eq!(space_id_from_label("remote-0"), None);
    }
}
