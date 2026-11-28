pub(crate) mod agent_detect;
mod attachment;
mod da_filter;
mod foreground;
mod gate;
mod handoff;
pub(crate) mod hub;
mod output;
pub mod paste_image;
mod presentation;
pub(crate) mod protocol;
mod queue;
pub(crate) mod remote;
mod replay;
mod session;
pub(crate) mod shell_init;
mod terminal_colors;
#[cfg(unix)]
mod unix_reader;
mod wake;
mod wire;

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock, RwLock};

use base64::Engine;
use serde::Serialize;
use std::thread;

use portable_pty::PtySize;
use tauri::Manager;

use crate::modules::control::ControlState;
use crate::modules::workspace::{user_spawn_cwd_or_home, WorkspaceEnv, WorkspaceRegistry};
use attachment::{AttachToken, LeaseEpoch};
use hub::{Command, Hub};
pub use presentation::PresentationClass;
pub use protocol::stream_protocol;
pub(crate) use replay::{ReplayError, ReplayEvent};
pub(crate) use session::{ReplaySnapshot, Session};
pub(crate) use terminal_colors::TerminalColors;

/// Largest `pty_set_presentation` batch accepted; a window never has more
/// terminals than this, so anything larger is a malformed request.
const MAX_PRESENTATION_BATCH: usize = 4096;

/// A live terminal resolved from a frontend pane id.
#[derive(Clone)]
pub struct PaneTarget {
    pub pty_id: u32,
    pub host: Arc<str>,
    pub link: Arc<agent_detect::AgentLink>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PtyHandoffCheckpoint {
    pty_id: u32,
    checkpoint: String,
    cols: u16,
    rows: u16,
    attach_token: String,
}

#[derive(Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PtyLeaseAck {
    pub pty_id: u32,
    pub attach_token: String,
    pub lease_epoch: String,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PtyReplayEvent {
    Output { data: String },
    Resize { cols: u16, rows: u16 },
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PtyReplayPayload {
    pty_id: u32,
    cols: u16,
    rows: u16,
    attach_token: String,
    lease_epoch: String,
    /// Offset of the first live data frame after `events`; the receiver
    /// starts counting there instead of at zero.
    stream_offset: u64,
    events: Vec<PtyReplayEvent>,
}

/// Decimals cross IPC as strings so a token or epoch above 2^53 keeps every
/// digit; a JS number would silently round it and break the lease check.
fn parse_handoff(value: &str) -> Result<u64, String> {
    value
        .parse()
        .map_err(|_| format!("invalid handoff id: {value}"))
}

fn parse_leases(leases: &[PtyLeaseAck]) -> Result<Vec<(u32, u64, u64)>, String> {
    leases
        .iter()
        .map(|lease| {
            let token = lease
                .attach_token
                .parse()
                .map_err(|_| format!("invalid attach token for terminal {}", lease.pty_id))?;
            let epoch = lease
                .lease_epoch
                .parse()
                .map_err(|_| format!("invalid lease epoch for terminal {}", lease.pty_id))?;
            Ok((lease.pty_id, token, epoch))
        })
        .collect()
}

fn to_replay_payload(replay: handoff::Replay) -> PtyReplayPayload {
    PtyReplayPayload {
        pty_id: replay.pty_id,
        cols: replay.cols,
        rows: replay.rows,
        attach_token: replay.token.to_string(),
        lease_epoch: replay.epoch.to_string(),
        stream_offset: replay.stream_offset,
        events: replay
            .events
            .into_iter()
            .map(|event| match event {
                replay::ReplayEvent::Output { bytes } => PtyReplayEvent::Output {
                    data: base64::engine::general_purpose::STANDARD.encode(bytes),
                },
                replay::ReplayEvent::Resize { cols, rows } => PtyReplayEvent::Resize { cols, rows },
            })
            .collect(),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CleanupReason {
    Navigation,
    WindowDestroyed,
    AppShutdown,
}

pub struct PtyState {
    sessions: RwLock<HashMap<u32, Arc<Session>>>,
    // Starts at 1 so freshly-handed-out ids are never 0, which the frontend
    // sometimes treats as "unset". Increments monotonically; never reused.
    next_id: AtomicU32,
    next_token: AtomicU64,
    hub: Hub,
    /// Resolved from the app's config on the first `terax-pty` request.
    origins: OnceLock<gate::AllowedOrigins>,
}

impl Default for PtyState {
    fn default() -> Self {
        Self {
            sessions: RwLock::new(HashMap::new()),
            next_id: AtomicU32::new(1),
            next_token: AtomicU64::new(1),
            hub: Hub::new(),
            origins: OnceLock::new(),
        }
    }
}

impl PtyState {
    /// Removes a session from the registry and from the hub's delivery set.
    pub(super) fn take(&self, id: u32) -> Option<Arc<Session>> {
        let session = self.sessions.write().unwrap().remove(&id);
        if session.is_some() {
            self.hub.submit(Command::Unregister { id });
        }
        session
    }

    /// The live terminal currently bound to a frontend pane, newest first when
    /// a pane was respawned. Finished sessions are not live.
    pub fn pane_target(&self, pane_id: u32) -> Option<PaneTarget> {
        let sessions = self.sessions.read().unwrap();
        sessions
            .iter()
            .filter(|(_, session)| {
                session.pane_id == Some(pane_id) && !session.finished.load(Ordering::Acquire)
            })
            .max_by_key(|(id, _)| **id)
            .map(|(id, session)| PaneTarget {
                pty_id: *id,
                host: session.host(),
                link: session.link.clone(),
            })
    }

    /// A live session by id, for the remote server's viewer streams.
    pub(crate) fn session(&self, id: u32) -> Option<Arc<Session>> {
        self.sessions.read().unwrap().get(&id).cloned()
    }

    /// The window's page navigated away (reload, HMR): its stream token dies
    /// and its terminals stop producing output until the new page's
    /// `pty_close_all` reaps them, exactly as when their channel went away.
    pub fn reset_webview(&self, label: &str) {
        cleanup_webview(self, label, CleanupReason::Navigation);
    }

    /// The webview is gone: close every terminal it owned.
    pub fn destroy_webview(&self, label: &str) {
        cleanup_webview(self, label, CleanupReason::WindowDestroyed);
    }

    /// Native window occlusion or system sleep: every terminal of the window
    /// drops to the occluded cadence without a frontend round trip.
    pub fn set_window_hidden(&self, label: &str, hidden: bool) {
        self.hub.submit(Command::WindowHidden {
            label: Arc::from(label),
            hidden,
        });
    }
}

/// Unifies page-navigation, window-destruction and shutdown cleanup. A
/// destroyed source window must not kill terminals already frozen for a
/// handoff: the target window is about to attach them.
pub fn cleanup_webview(state: &PtyState, label: &str, reason: CleanupReason) -> usize {
    match reason {
        CleanupReason::Navigation => {
            state.hub.submit(Command::ResetWebview {
                label: Arc::from(label),
            });
            for session in state.sessions.read().unwrap().values() {
                if &*session.host() == label {
                    session.close_output();
                }
            }
            return 0;
        }
        CleanupReason::WindowDestroyed | CleanupReason::AppShutdown => {
            state.hub.submit(Command::RemoveWebview {
                label: Arc::from(label),
            });
        }
    }
    let owned: Vec<(u32, Arc<Session>)> = {
        let mut sessions = state.sessions.write().unwrap();
        let ids: Vec<u32> = sessions
            .iter()
            .filter(|(_, session)| {
                &*session.host() == label
                    && !(reason == CleanupReason::WindowDestroyed
                        && session.attachment_phase() == attachment::AttachmentPhase::Preparing)
            })
            .map(|(id, _)| *id)
            .collect();
        ids.into_iter()
            .filter_map(|id| sessions.remove(&id).map(|session| (id, session)))
            .collect()
    };
    let count = owned.len();
    for (id, session) in owned {
        reap(id, session, "webview cleanup");
    }
    count
}

fn check_presentation_batch(ids: &[u32]) -> Result<(), String> {
    if ids.len() > MAX_PRESENTATION_BATCH {
        return Err(format!(
            "pty_set_presentation: at most {MAX_PRESENTATION_BATCH} ids per call"
        ));
    }
    Ok(())
}

/// Closes output, kills the child and drops the session off the caller's
/// thread (`ClosePseudoConsole` can block until conhost drains).
fn reap(id: u32, session: Arc<Session>, reason: &str) {
    session.close_output();
    if let Err(e) = session.killer.lock().unwrap().kill() {
        // Non-fatal: the child may already have exited on its own (e.g. the
        // user ran `exit`). Log so this isn't invisible during debugging.
        log::debug!("pty {reason}: kill id={id} returned {e}");
    }
    thread::Builder::new()
        .name(format!("terax-pty-drop-{id}"))
        .spawn(move || {
            let t0 = std::time::Instant::now();
            session::drop_session(session);
            log::info!(
                "pty session id={id} dropped in {}ms",
                t0.elapsed().as_millis()
            );
        })
        .expect("spawn pty drop thread");
}

#[derive(serde::Serialize)]
pub struct PtyOpenResult {
    pub id: u32,
    pub token: String,
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn pty_open(
    app: tauri::AppHandle,
    webview: tauri::Webview,
    state: tauri::State<'_, PtyState>,
    control: tauri::State<'_, ControlState>,
    cols: u16,
    rows: u16,
    cwd: Option<String>,
    workspace: Option<WorkspaceEnv>,
    blocks: Option<bool>,
    shell: Option<String>,
    pane_id: Option<u32>,
    presentation: Option<PresentationClass>,
    colors: Option<TerminalColors>,
) -> Result<PtyOpenResult, String> {
    let workspace = WorkspaceEnv::from_option(workspace);
    let blocks = blocks.unwrap_or(false);
    // A Windows helper cannot execute inside WSL without explicit path and
    // network translation. Do not inject credentials for a broken command.
    let control_env = if workspace.is_wsl() || workspace.is_ssh() {
        None
    } else {
        pane_id.and_then(|pane_id| control.shell_env(pane_id))
    };
    let id = state.next_id.fetch_add(1, Ordering::Relaxed);
    let token = state.next_token.fetch_add(1, Ordering::Relaxed);
    let credential = control_env.clone();
    let pending_credential = credential.as_ref().map(|env| control.pending_shell(env));
    let control_app = app.clone();
    let owner: Arc<str> = Arc::from(webview.label());
    let hub = state.hub.clone();
    let class = presentation.unwrap_or_default();
    let session = tauri::async_runtime::spawn_blocking(move || {
        // Authorizing the directory canonicalizes it, which touches the disk
        // (a network volume, or wsl.exe for a WSL workspace): never on a worker.
        let registry = app.state::<WorkspaceRegistry>();
        let cwd = if workspace.is_ssh() {
            // A server path, as the server spells it: it must be inside a
            // folder of the space; otherwise the terminal opens in the
            // space's folder.
            cwd.map(|cwd| cwd.trim().to_string())
                .filter(|cwd| !cwd.is_empty() && registry.covers(&workspace, cwd))
        } else {
            user_spawn_cwd_or_home(&registry, cwd.as_deref(), &workspace)
        };
        session::spawn(
            id,
            token,
            app,
            owner,
            hub,
            class,
            cols,
            rows,
            cwd,
            workspace,
            blocks,
            shell,
            control_env,
            colors,
            pane_id,
        )
    })
    .await
    .map_err(|e| {
        log::error!("pty_open join failed: {e}");
        e.to_string()
    })?
    .map_err(|e| {
        log::error!("pty_open failed: {e}");
        e
    })?;
    let attachment_token = session.attachment_token().to_string();
    state.sessions.write().unwrap().insert(id, session);
    if let Some(env) = credential {
        control.activate_shell(
            &env,
            id,
            &crate::modules::agent_events::TauriAgentHost(&control_app),
        );
    }
    drop(pending_credential);
    // The shell can exit before this insert (instant failure, `exit` in an rc
    // file); the waiter's reap then ran with the id absent. Re-check and reap
    // so the pseudoconsole isn't stranded.
    let finished = state
        .sessions
        .read()
        .unwrap()
        .get(&id)
        .map(|s| s.finished.load(Ordering::Acquire))
        .unwrap_or(false);
    if finished {
        if let Some(s) = state.take(id) {
            thread::Builder::new()
                .name(format!("terax-pty-drop-{id}"))
                .spawn(move || session::drop_session(s))
                .expect("spawn pty drop thread");
        }
    }
    log::info!("pty opened id={id} cols={cols} rows={rows}");
    Ok(PtyOpenResult {
        id,
        token: attachment_token,
    })
}

// Input is the latency-critical path: raw body + id header skips JSON
// serialization of every keystroke on both sides of the IPC boundary.
#[tauri::command]
pub async fn pty_write(
    webview: tauri::Webview,
    state: tauri::State<'_, PtyState>,
    request: tauri::ipc::Request<'_>,
) -> Result<(), String> {
    let id: u32 = request
        .headers()
        .get("x-pty-id")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| "pty_write: missing x-pty-id header".to_string())?;
    let token: AttachToken = request
        .headers()
        .get("x-pty-token")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| "pty_write: missing x-pty-token header".to_string())?;
    let epoch: LeaseEpoch = request
        .headers()
        .get("x-pty-epoch")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| "pty_write: missing x-pty-epoch header".to_string())?;
    let tauri::ipc::InvokeBody::Raw(bytes) = request.body() else {
        return Err("pty_write: expected raw body".to_string());
    };
    let session = state
        .sessions
        .read()
        .unwrap()
        .get(&id)
        .cloned()
        .ok_or_else(|| {
            log::warn!("pty_write: unknown id={id}");
            "no session".to_string()
        })?;
    if !session.authorizes(webview.label(), token, epoch) {
        log::warn!("pty_write: unauthorized write id={id}");
        return Err("pty_write: not the active attachment".to_string());
    }
    // One ordered queue per session feeds a dedicated writer thread: no task
    // spawn or lock per keystroke, and the result is the real pipe write's.
    session.write(bytes.clone()).await.map_err(|e| {
        log::debug!("pty_write id={id} failed: {e}");
        e
    })
}

#[tauri::command]
pub fn pty_resize(
    webview: tauri::Webview,
    state: tauri::State<PtyState>,
    id: u32,
    cols: u16,
    rows: u16,
    token: u64,
    epoch: u64,
) -> Result<(), String> {
    let session = state
        .sessions
        .read()
        .unwrap()
        .get(&id)
        .cloned()
        .ok_or_else(|| {
            log::warn!("pty_resize: unknown id={id}");
            "no session".to_string()
        })?;
    if !session.authorizes(webview.label(), token, epoch) {
        log::warn!("pty_resize: unauthorized resize id={id}");
        return Err("pty_resize: not the active attachment".to_string());
    }
    let result = session
        .resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| {
            log::warn!("pty_resize id={id} failed: {e}");
            e
        });
    result
}

/// Pushes the terminal's theme so colour queries (`OSC 4/10/11/12`,
/// `CSI ? 996 n`) are answered by the reader without a webview round trip.
#[tauri::command]
pub fn pty_set_colors(
    state: tauri::State<PtyState>,
    id: u32,
    colors: TerminalColors,
) -> Result<(), String> {
    let session = state
        .sessions
        .read()
        .unwrap()
        .get(&id)
        .cloned()
        .ok_or_else(|| "no session".to_string())?;
    session.set_colors(colors);
    Ok(())
}

#[tauri::command]
pub fn pty_close(
    state: tauri::State<PtyState>,
    id: u32,
    handoff_id: Option<String>,
) -> Result<(), String> {
    let handoff = handoff_id.as_deref().map(parse_handoff).transpose()?;
    close_session(&state, id, handoff)
}

/// A terminal moving between windows belongs to its handoff transaction, not
/// to whichever view last showed it. Closing it then is only allowed for that
/// transaction (its abort path); a stray close from the source window's view
/// teardown would otherwise kill the process mid-move.
fn close_allowed(session: &Session, handoff: Option<attachment::HandoffId>) -> bool {
    let attachment = session.attachment.read().unwrap();
    match attachment.phase() {
        attachment::AttachmentPhase::Attached => true,
        _ => handoff.is_some() && attachment.handoff_id() == handoff,
    }
}

fn close_session(
    state: &PtyState,
    id: u32,
    handoff: Option<attachment::HandoffId>,
) -> Result<(), String> {
    let allowed = state
        .sessions
        .read()
        .unwrap()
        .get(&id)
        .map(|session| close_allowed(session, handoff));
    if allowed == Some(false) {
        log::warn!("pty_close refused id={id}: terminal is being handed off");
        return Err(format!("terminal {id} is being moved to another window"));
    }
    // Detached drop: on Windows `ClosePseudoConsole` can block until conhost
    // drains, which would freeze this Tauri worker thread and stall IPC.
    match state.take(id) {
        Some(session) => {
            log::info!("pty closed id={id}");
            reap(id, session, "close");
        }
        None => log::debug!("pty_close: unknown id={id}"),
    }
    Ok(())
}

#[tauri::command]
pub fn pty_has_foreground_process(state: tauri::State<PtyState>, id: u32) -> Result<bool, String> {
    let sessions = state.sessions.read().unwrap();
    let session = sessions.get(&id).ok_or_else(|| {
        log::warn!("pty_has_foreground_process: unknown session id={id}");
        "no session".to_string()
    })?;
    let shell_pid = session.shell_pid;
    if shell_pid == 0 {
        return Ok(false);
    }
    Ok(foreground::shell_has_children(shell_pid))
}

// Foreground-only check for the renderer hibernation path: true while a job
// owns the tty (tcgetpgrp != shell pgid). Stricter and cheaper than
// pty_has_foreground_process, which counts background children too.
#[tauri::command]
pub fn pty_has_foreground_job(state: tauri::State<PtyState>, id: u32) -> Result<bool, String> {
    let sessions = state.sessions.read().unwrap();
    let session = sessions.get(&id).ok_or_else(|| {
        log::warn!("pty_has_foreground_job: unknown session id={id}");
        "no session".to_string()
    })?;
    let shell_pid = session.shell_pid;
    if shell_pid == 0 {
        return Ok(false);
    }
    #[cfg(unix)]
    {
        let leader = session
            .master
            .lock()
            .unwrap()
            .as_ref()
            .and_then(|master| master.process_group_leader());
        Ok(matches!(leader, Some(pid) if pid > 0 && pid as u32 != shell_pid))
    }
    #[cfg(windows)]
    {
        Ok(foreground::shell_has_children(shell_pid))
    }
}

// A fresh webview load orphans the previous frontend's sessions in this still
// running process; reap them on boot before any new tab spawns.
#[tauri::command]
pub fn pty_close_all(state: tauri::State<PtyState>) -> Result<usize, String> {
    let drained: Vec<(u32, Arc<Session>)> = {
        let mut sessions = state.sessions.write().unwrap();
        sessions.drain().collect()
    };
    let count = drained.len();
    for (id, session) in drained {
        state.hub.submit(Command::Unregister { id });
        reap(id, session, "close_all");
    }
    if count > 0 {
        log::info!("pty_close_all: reaped {count} orphaned session(s)");
    }
    Ok(count)
}

/// Opens (or reopens) this webview's output stream and returns the secret
/// that authorizes its `terax-pty` requests. Only the page's own frame can
/// invoke commands, so other frames and origins cannot read terminal output
/// even though the custom protocol is reachable from any frame. Reopening
/// supersedes the previous stream and detaches its terminals.
#[tauri::command]
pub async fn pty_prepare_handoff(
    state: tauri::State<'_, PtyState>,
    webview: tauri::Webview,
    handoff_id: String,
    ids: Vec<u32>,
    to: String,
) -> Result<Vec<PtyHandoffCheckpoint>, String> {
    let handoff_id = parse_handoff(&handoff_id)?;
    let checkpoints = handoff::handoff_prepare(&state, handoff_id, &ids, webview.label(), &to)
        .inspect_err(|e| {
            log::warn!(
                "handoff prepare failed id={handoff_id} from={} to={to} pty_ids={ids:?}: {e}",
                webview.label()
            )
        })?;
    log::debug!(
        "handoff prepare id={} from={} to={} pty_ids={:?} checkpoints={}",
        handoff_id,
        webview.label(),
        to,
        ids,
        checkpoints.len()
    );
    Ok(checkpoints
        .into_iter()
        .map(|checkpoint| PtyHandoffCheckpoint {
            pty_id: checkpoint.pty_id,
            checkpoint: checkpoint.checkpoint.to_string(),
            cols: checkpoint.cols,
            rows: checkpoint.rows,
            attach_token: checkpoint.token.to_string(),
        })
        .collect())
}

#[tauri::command]
pub async fn pty_detach(
    state: tauri::State<'_, PtyState>,
    webview: tauri::Webview,
    handoff_id: String,
    ids: Vec<u32>,
) -> Result<(), String> {
    let handoff_id = parse_handoff(&handoff_id)?;
    let result = handoff::handoff_detach(&state, handoff_id, &ids, webview.label());
    if let Err(e) = &result {
        log::warn!(
            "handoff detach failed id={handoff_id} from={} pty_ids={ids:?}: {e}",
            webview.label()
        );
    }
    log::debug!(
        "handoff detach id={} from={} pty_ids={:?} result={:?}",
        handoff_id,
        webview.label(),
        ids,
        result.as_ref().map(|_| ()).map_err(|e| e.as_str())
    );
    result
}

#[tauri::command]
pub async fn pty_attach(
    state: tauri::State<'_, PtyState>,
    webview: tauri::Webview,
    handoff_id: String,
    ids: Vec<u32>,
) -> Result<Vec<PtyReplayPayload>, String> {
    let handoff_id = parse_handoff(&handoff_id)?;
    let replays =
        handoff::handoff_attach(&state, handoff_id, &ids, webview.label()).inspect_err(|e| {
            log::warn!(
                "handoff attach failed id={handoff_id} to={} pty_ids={ids:?}: {e}",
                webview.label()
            )
        })?;
    log::debug!(
        "handoff attach id={} to={} pty_ids={:?} replays={}",
        handoff_id,
        webview.label(),
        ids,
        replays.len()
    );
    for id in &ids {
        state.hub.submit(Command::RebindEntry {
            id: *id,
            label: Arc::from(webview.label()),
        });
    }
    Ok(replays.into_iter().map(to_replay_payload).collect())
}

#[tauri::command]
pub async fn pty_commit_handoff(
    state: tauri::State<'_, PtyState>,
    webview: tauri::Webview,
    handoff_id: String,
    leases: Vec<PtyLeaseAck>,
) -> Result<(), String> {
    let handoff_id = parse_handoff(&handoff_id)?;
    let leases = parse_leases(&leases)?;
    for (id, _, _) in &leases {
        let session = state
            .sessions
            .read()
            .unwrap()
            .get(id)
            .cloned()
            .ok_or_else(|| format!("no terminal {id}"))?;
        // The source window drives the transaction and commits it after the
        // target attached, by which point the host is already the target.
        let caller_owns = {
            let attachment = session.attachment.read().unwrap();
            &*attachment.host() == webview.label()
                || (attachment.handoff_id() == Some(handoff_id)
                    && attachment.handoff_from().as_deref() == Some(webview.label()))
        };
        if !caller_owns {
            log::warn!(
                "handoff commit refused id={handoff_id} by={} pty={id}: host is {}",
                webview.label(),
                session.host()
            );
            return Err(format!("terminal {id} is not attached to this window"));
        }
    }
    log::debug!(
        "handoff commit id={} by={} leases={:?}",
        handoff_id,
        webview.label(),
        leases.iter().map(|(id, _, _)| *id).collect::<Vec<_>>()
    );
    handoff::handoff_commit(&state, handoff_id, &leases)
        .inspect_err(|e| log::warn!("handoff commit failed id={handoff_id}: {e}"))
}

#[tauri::command]
pub async fn pty_rollback_handoff(
    state: tauri::State<'_, PtyState>,
    webview: tauri::Webview,
    handoff_id: String,
    ids: Vec<u32>,
) -> Result<Vec<PtyReplayPayload>, String> {
    let handoff_id = parse_handoff(&handoff_id)?;
    for id in &ids {
        let session = state
            .sessions
            .read()
            .unwrap()
            .get(id)
            .cloned()
            .ok_or_else(|| format!("no terminal {id}"))?;
        if session.host().as_ref() != webview.label()
            && session.attachment.read().unwrap().handoff_id() != Some(handoff_id)
        {
            log::warn!(
                "handoff rollback refused id={handoff_id} by={} pty={id}: host is {}",
                webview.label(),
                session.host()
            );
            return Err(format!("terminal {id} does not belong to this handoff"));
        }
    }
    log::warn!(
        "handoff rollback id={} by={} pty_ids={:?}",
        handoff_id,
        webview.label(),
        ids
    );
    let replays = handoff::handoff_rollback(&state, handoff_id, &ids).inspect_err(|e| {
        log::warn!("handoff rollback failed id={handoff_id} pty_ids={ids:?}: {e}")
    })?;
    for id in &ids {
        let host = state
            .sessions
            .read()
            .unwrap()
            .get(id)
            .map(|session| session.host());
        if let Some(host) = host {
            state.hub.submit(Command::RebindEntry {
                id: *id,
                label: host,
            });
        }
    }
    Ok(replays.into_iter().map(to_replay_payload).collect())
}

#[tauri::command]
pub async fn pty_stream_open(
    webview: tauri::Webview,
    state: tauri::State<'_, PtyState>,
) -> Result<String, String> {
    let token = hub::os_token().map_err(|reason| {
        log::error!("pty_stream_open refused: {reason}");
        reason.to_string()
    })?;
    let (reply, result) = tokio::sync::oneshot::channel();
    state.hub.submit(Command::OpenStream {
        label: Arc::from(webview.label()),
        token,
        epoch: 0,
        reply: Box::new(move |result_value| {
            let _ = reply.send(result_value);
        }),
    });
    result
        .await
        .map_err(|_| "PTY stream is unavailable".to_string())?
        .map_err(|reason| {
            log::error!("pty_stream_open refused: {reason}");
            reason.to_string()
        })?;
    Ok(format!("{token:016x}"))
}

/// Reports which presentation class terminals are in. Called only when a
/// class changes; unknown ids are ignored (the terminal may have just closed).
///
/// This stays `async`, off the main thread, but async commands run on a
/// multi-threaded runtime and two reports can be applied in either order.
/// `seq` is the page's counter of reports (it only grows): a terminal ignores
/// any report not newer than the last one that named it, so the final class
/// is always the latest one sent. Making the command synchronous would order
/// arrivals only if the webview delivered separate requests in order, and
/// would put every report on the main thread for nothing.
#[tauri::command]
pub async fn pty_set_presentation(
    webview: tauri::Webview,
    state: tauri::State<'_, PtyState>,
    ids: Vec<u32>,
    class: PresentationClass,
    seq: u64,
) -> Result<(), String> {
    check_presentation_batch(&ids)?;
    state.hub.submit(Command::SetPresentation {
        label: Arc::from(webview.label()),
        ids,
        class,
        seq,
    });
    Ok(())
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PtyDiagnostics {
    host_pid: u32,
    host_rss_bytes: Option<u64>,
    hub: hub::HubDiagnostics,
    sessions: Vec<PtySessionDiagnostics>,
}

#[derive(serde::Serialize)]
struct PtySessionDiagnostics {
    id: u32,
    #[serde(flatten)]
    output: session::OutputDiagnostics,
}

#[tauri::command]
pub fn pty_diagnostics(state: tauri::State<PtyState>) -> PtyDiagnostics {
    let host_pid = std::process::id();
    let sessions = state
        .sessions
        .read()
        .unwrap()
        .iter()
        .map(|(id, session)| PtySessionDiagnostics {
            id: *id,
            output: session.diagnostics(),
        })
        .collect();
    PtyDiagnostics {
        host_pid,
        host_rss_bytes: crate::modules::proc::rss::rss_bytes(host_pid),
        hub: state.hub.diagnostics(),
        sessions,
    }
}

#[tauri::command]
pub fn pty_session_shell(
    state: tauri::State<PtyState>,
    webview: tauri::Webview,
    id: u32,
) -> Result<String, String> {
    let sessions = state.sessions.read().unwrap();
    let session = sessions.get(&id).ok_or("no session")?;
    if &*session.host() != webview.label() {
        return Err("session belongs to another webview".into());
    }
    Ok(session.shell_program.clone())
}

#[tauri::command]
pub fn pty_shell_name() -> String {
    shell_init::detect_shell_name()
}

#[tauri::command]
pub fn pty_list_shells() -> Vec<shell_init::ShellInfo> {
    shell_init::list_shells()
}

#[cfg(all(test, unix))]
mod tests {
    use super::session::{spawn_with, Host};
    use super::*;
    use portable_pty::CommandBuilder;
    use std::time::{Duration, Instant};

    fn open(state: &Arc<PtyState>, id: u32, owner: &str) -> Arc<Session> {
        open_in_pane(state, id, owner, None)
    }

    fn open_in_pane(
        state: &Arc<PtyState>,
        id: u32,
        owner: &str,
        pane_id: Option<u32>,
    ) -> Arc<Session> {
        let reaper = state.clone();
        let session = spawn_with(
            Host {
                id,
                token: id as u64,
                owner: Arc::from(owner),
                hub: state.hub.clone(),
                class: PresentationClass::Hidden,
                signal: Arc::new(|_| {}),
                notify: Arc::new(|_| {}),
                pane_id,
                link: agent_detect::AgentLink::new(),
                reap: Box::new(move || {
                    if let Some(session) = reaper.take(id) {
                        session::drop_session(session);
                    }
                }),
                colors: None,
            },
            PtySize::default(),
            || {
                let mut command = CommandBuilder::new("/bin/sh");
                command.arg("-c");
                command.arg("sleep 30");
                Ok(command)
            },
        )
        .unwrap();
        state.sessions.write().unwrap().insert(id, session.clone());
        session
    }

    fn wait_for(what: &str, condition: impl Fn() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !condition() {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn pty_write_uses_attachment_host_not_the_callers_webview_label() {
        let state = Arc::new(PtyState::default());
        let session = open(&state, 1, "main");
        let token = session.attachment_token_for_test();
        assert!(session.authorizes("main", token, 0));
        assert!(!session.authorizes("space-a", token, 0));
        assert!(!session.authorizes("main", token + 1, 0));
        assert!(!session.authorizes("main", token, 1));
    }

    #[test]
    fn window_destroyed_during_handoff_does_not_kill_the_session() {
        let state = Arc::new(PtyState::default());
        let session = open(&state, 1, "main");
        handoff::handoff_prepare(&state, 1, &[1], "main", "space-a").expect("prepare");
        assert_eq!(
            cleanup_webview(&state, "main", CleanupReason::WindowDestroyed),
            0
        );
        assert!(state.sessions.read().unwrap().contains_key(&1));
        assert!(!session.queue_closed());
        let _ = cleanup_webview(&state, "main", CleanupReason::AppShutdown);
        wait_for("shutdown to reap the frozen session", || {
            !state.sessions.read().unwrap().contains_key(&1)
        });
    }

    #[test]
    fn closing_a_terminal_mid_handoff_needs_its_handoff_id() {
        let state = Arc::new(PtyState::default());
        let _session = open(&state, 1, "main");
        handoff::handoff_prepare(&state, 7, &[1], "main", "space-a").expect("prepare");
        handoff::handoff_detach(&state, 7, &[1], "main").expect("detach");

        // The source view tearing down must not kill the moving process.
        assert!(close_session(&state, 1, None).is_err());
        assert!(close_session(&state, 1, Some(8)).is_err());
        assert!(state.sessions.read().unwrap().contains_key(&1));

        // The owning transaction may still abort and close it.
        close_session(&state, 1, Some(7)).expect("close by owning handoff");
        assert!(!state.sessions.read().unwrap().contains_key(&1));
    }

    #[test]
    fn closing_an_attached_terminal_is_unconditional() {
        let state = Arc::new(PtyState::default());
        let _session = open(&state, 1, "main");
        close_session(&state, 1, None).expect("close");
        assert!(!state.sessions.read().unwrap().contains_key(&1));
        // Unknown ids stay a no-op.
        close_session(&state, 99, None).expect("unknown id");
    }

    #[test]
    fn presentation_batches_are_bounded_at_the_boundary() {
        assert!(check_presentation_batch(&[]).is_ok());
        assert!(check_presentation_batch(&vec![1; MAX_PRESENTATION_BATCH]).is_ok());
        assert!(check_presentation_batch(&vec![1; MAX_PRESENTATION_BATCH + 1]).is_err());
    }

    #[test]
    fn reloading_a_page_stops_its_terminals_streaming_but_leaves_them_for_pty_close_all() {
        let state = Arc::new(PtyState::default());
        let main = open(&state, 1, "main");
        let other = open(&state, 2, "settings");
        state.reset_webview("main");
        wait_for("main's output to close", || main.queue_closed());
        assert!(!other.queue_closed());
        // Both sessions are still registered: only pty_close_all reaps them.
        assert_eq!(state.sessions.read().unwrap().len(), 2);
        for id in [1, 2] {
            if let Some(session) = state.take(id) {
                reap(id, session, "test cleanup");
            }
        }
    }

    #[test]
    fn a_pane_resolves_to_its_newest_live_terminal_only() {
        let state = Arc::new(PtyState::default());
        let first = open_in_pane(&state, 1, "main", Some(7));
        let other = open_in_pane(&state, 2, "main", Some(8));
        let unbound = open(&state, 3, "main");
        assert_eq!(state.pane_target(7).map(|t| t.pty_id), Some(1));
        assert_eq!(state.pane_target(8).map(|t| t.pty_id), Some(2));
        assert!(state.pane_target(9).is_none());

        // A respawn in the same pane wins over the older terminal.
        let respawn = open_in_pane(&state, 4, "main", Some(7));
        let target = state.pane_target(7).unwrap();
        assert_eq!(target.pty_id, 4);
        assert_eq!(&*target.host, "main");
        assert!(std::sync::Arc::ptr_eq(&target.link, &respawn.link));

        // A closed or finished terminal is no longer reachable.
        drop(state.take(4));
        assert_eq!(state.pane_target(7).map(|t| t.pty_id), Some(1));
        first.finished.store(true, Ordering::Release);
        assert!(state.pane_target(7).is_none());
        let _ = (other, unbound);
        for id in [1, 2, 3] {
            if let Some(session) = state.take(id) {
                reap(id, session, "test cleanup");
            }
        }
    }

    #[test]
    fn destroying_a_webview_closes_exactly_its_terminals() {
        let state = Arc::new(PtyState::default());
        let doomed = open(&state, 1, "main");
        let survivor = open(&state, 2, "settings");
        let pid = doomed.shell_pid as i32;
        drop(doomed);
        state.destroy_webview("main");
        assert!(state.sessions.read().unwrap().get(&1).is_none());
        assert!(state.sessions.read().unwrap().get(&2).is_some());
        wait_for("the shell to be killed", || unsafe {
            let mut status = 0;
            libc::waitpid(pid, &mut status, libc::WNOHANG) == pid || libc::kill(pid, 0) != 0
        });
        assert!(!survivor.queue_closed());
        if let Some(session) = state.take(2) {
            reap(2, session, "test cleanup");
        }
    }
}
