//! Hidden relay webview sessions (`remote-<n>`) for `/app/` browser clients.
//!
//! Each session owns one hidden `remote-<n>` webview loading `remote-relay.html`,
//! forwards callbacks (`remote_relay_push`) into a bounded replay buffer + live
//! broadcast channel, and closes after 60 s without an SSE subscriber, on device
//! revocation / `app` grant removal, or when the listener stops.

use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::ipc::{CallbackFn, InvokeBody, InvokeResponse};
use tauri::webview::InvokeRequest;
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};
use tokio::sync::{broadcast, oneshot, watch};

use super::super::devices::DeviceStore;

pub const MAX_RELAY_SESSIONS: usize = 3;
pub const DISCONNECT_GRACE: Duration = Duration::from_secs(60);
pub const REPLAY_MAX_EVENTS: usize = 1_000;
pub const REPLAY_MAX_BYTES: usize = 4 * 1024 * 1024;
const BROADCAST_CAP: usize = 256;

#[derive(Clone, Debug)]
pub enum InvokeRequestBody {
    Json(serde_json::Value),
    Raw(Vec<u8>),
}

#[derive(Clone, Debug)]
pub enum InvokeResult {
    OkJson(String),
    OkRaw(Vec<u8>),
    Err(serde_json::Value),
}

pub trait RelayHost: Send + Sync {
    fn open_relay<'a>(
        &'a self,
        label: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>>;

    fn close_relay(&self, label: &str);

    fn invoke<'a>(
        &'a self,
        label: &'a str,
        cmd: &'a str,
        body: InvokeRequestBody,
        callback_id: u32,
        error_id: u32,
    ) -> Pin<Box<dyn Future<Output = Result<InvokeResult, String>> + Send + 'a>>;
}

pub struct AppRelayHost(pub tauri::AppHandle);

impl RelayHost for AppRelayHost {
    fn open_relay<'a>(
        &'a self,
        label: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
        Box::pin(async move {
            if self.0.get_webview_window(label).is_none() {
                WebviewWindowBuilder::new(
                    &self.0,
                    label,
                    WebviewUrl::App("remote-relay.html".into()),
                )
                .title("Terax Remote Relay")
                .visible(false)
                .skip_taskbar(true)
                .build()
                .map_err(|e| format!("build {label} failed: {e}"))?;
            }
            let Some(wv) = self.0.get_webview_window(label) else {
                return Err(format!("relay window {label} missing after build"));
            };
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                let url = wv.url().map(|u| u.to_string()).unwrap_or_default();
                if url.contains("remote-relay.html") {
                    break;
                }
                if Instant::now() >= deadline {
                    return Err(format!("timeout waiting for {label} to load remote-relay.html"));
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            tokio::time::sleep(Duration::from_millis(150)).await;
            Ok(())
        })
    }

    fn close_relay(&self, label: &str) {
        if let Some(window) = self.0.get_webview_window(label) {
            let _ = window.destroy();
        }
    }

    fn invoke<'a>(
        &'a self,
        label: &'a str,
        cmd: &'a str,
        body: InvokeRequestBody,
        callback_id: u32,
        error_id: u32,
    ) -> Pin<Box<dyn Future<Output = Result<InvokeResult, String>> + Send + 'a>> {
        Box::pin(async move {
            let webview = self
                .0
                .get_webview_window(label)
                .ok_or_else(|| format!("relay window {label} not found"))?;
            let webview = webview.as_ref().clone();
            let url = webview.url().map_err(|e| e.to_string())?;
            let (tx, rx) = oneshot::channel();
            let tx = Mutex::new(Some(tx));
            let invoke_body = match body {
                InvokeRequestBody::Json(v) => InvokeBody::Json(v),
                InvokeRequestBody::Raw(b) => InvokeBody::Raw(b),
            };
            let req = InvokeRequest {
                cmd: cmd.to_string(),
                callback: CallbackFn(callback_id),
                error: CallbackFn(error_id),
                url,
                body: invoke_body,
                headers: Default::default(),
                invoke_key: self.0.invoke_key().to_string(),
            };
            webview.clone().on_message(
                req,
                Box::new(move |_wv, _cmd, response, _cb, _err| {
                    let result = match response {
                        InvokeResponse::Ok(body) => match body {
                            tauri::ipc::InvokeResponseBody::Json(s) => InvokeResult::OkJson(s),
                            tauri::ipc::InvokeResponseBody::Raw(b) => InvokeResult::OkRaw(b),
                        },
                        InvokeResponse::Err(err) => InvokeResult::Err(err.0),
                    };
                    if let Some(sender) = tx.lock().unwrap().take() {
                        let _ = sender.send(result);
                    }
                }),
            );
            tokio::time::timeout(Duration::from_secs(30), rx)
                .await
                .map_err(|_| format!("timeout waiting for {cmd}"))?
                .map_err(|_| format!("responder dropped for {cmd}"))
        })
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct CallbackEvent {
    pub seq: u64,
    pub id: u32,
    pub payload: serde_json::Value,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    pub id: String,
    pub label: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionError {
    TooManySessions,
    NotFound,
    HostFailed(String),
}

struct SessionEntry {
    id: String,
    device_id: String,
    label: String,
    tx: broadcast::Sender<CallbackEvent>,
    history: VecDeque<CallbackEvent>,
    history_bytes: usize,
    next_seq: u64,
    subscribers: usize,
    last_disconnected: Option<Instant>,
}

pub enum ReplayResult {
    Events {
        events: Vec<CallbackEvent>,
        rx: broadcast::Receiver<CallbackEvent>,
    },
    Reset {
        rx: broadcast::Receiver<CallbackEvent>,
    },
}

pub struct SessionManager {
    host: Arc<dyn RelayHost>,
    sessions: Mutex<HashMap<String, SessionEntry>>,
    label_to_id: Mutex<HashMap<String, String>>,
    next_label_index: AtomicUsize,
    next_callback_id: AtomicU32,
}

impl SessionManager {
    pub fn new(host: Arc<dyn RelayHost>) -> Self {
        Self {
            host,
            sessions: Mutex::new(HashMap::new()),
            label_to_id: Mutex::new(HashMap::new()),
            next_label_index: AtomicUsize::new(0),
            next_callback_id: AtomicU32::new(100),
        }
    }

    pub fn next_callback_pair(&self) -> (u32, u32) {
        let cb = self.next_callback_id.fetch_add(2, Ordering::Relaxed);
        (cb, cb.wrapping_add(1))
    }

    pub async fn create_or_reuse(
        &self,
        device_id: &str,
        requested_id: Option<&str>,
    ) -> Result<SessionInfo, SessionError> {
        if let Some(req_id) = requested_id {
            let guard = self.sessions.lock().unwrap();
            if let Some(entry) = guard.get(req_id) {
                if entry.device_id == device_id {
                    return Ok(SessionInfo {
                        id: entry.id.clone(),
                        label: entry.label.clone(),
                    });
                }
            }
        }

        let label = {
            let guard = self.sessions.lock().unwrap();
            if guard.len() >= MAX_RELAY_SESSIONS {
                return Err(SessionError::TooManySessions);
            }
            let idx = self.next_label_index.fetch_add(1, Ordering::Relaxed);
            format!("remote-{idx}")
        };

        self.host
            .open_relay(&label)
            .await
            .map_err(SessionError::HostFailed)?;

        let id = random_hex(16);
        let (tx, _) = broadcast::channel(BROADCAST_CAP);
        let entry = SessionEntry {
            id: id.clone(),
            device_id: device_id.to_string(),
            label: label.clone(),
            tx,
            history: VecDeque::new(),
            history_bytes: 0,
            next_seq: 1,
            subscribers: 0,
            last_disconnected: Some(Instant::now()),
        };

        {
            let mut guard = self.sessions.lock().unwrap();
            if guard.len() >= MAX_RELAY_SESSIONS {
                drop(guard);
                self.host.close_relay(&label);
                return Err(SessionError::TooManySessions);
            }
            guard.insert(id.clone(), entry);
            self.label_to_id.lock().unwrap().insert(label.clone(), id.clone());
        }

        Ok(SessionInfo { id, label })
    }

    pub fn lookup(&self, session_id: &str, device_id: &str) -> Option<String> {
        let guard = self.sessions.lock().unwrap();
        guard
            .get(session_id)
            .filter(|e| e.device_id == device_id)
            .map(|e| e.label.clone())
    }

    pub fn push_callback(&self, label: &str, id: u32, payload: serde_json::Value) -> bool {
        let session_id = {
            let map = self.label_to_id.lock().unwrap();
            match map.get(label) {
                Some(s) => s.clone(),
                None => return false,
            }
        };
        let mut guard = self.sessions.lock().unwrap();
        let Some(entry) = guard.get_mut(&session_id) else {
            return false;
        };
        let seq = entry.next_seq;
        entry.next_seq += 1;
        let event = CallbackEvent { seq, id, payload };
        let approx_bytes = serde_json::to_string(&event.payload)
            .map(|s| s.len())
            .unwrap_or(64);
        entry.history.push_back(event.clone());
        entry.history_bytes += approx_bytes;
        while entry.history.len() > REPLAY_MAX_EVENTS || entry.history_bytes > REPLAY_MAX_BYTES {
            if let Some(evicted) = entry.history.pop_front() {
                let evicted_bytes = serde_json::to_string(&evicted.payload)
                    .map(|s| s.len())
                    .unwrap_or(64);
                entry.history_bytes = entry.history_bytes.saturating_sub(evicted_bytes);
            } else {
                break;
            }
        }
        let _ = entry.tx.send(event);
        true
    }

    pub fn subscribe(
        &self,
        session_id: &str,
        device_id: &str,
        last_event_id: Option<u64>,
    ) -> Result<ReplayResult, SessionError> {
        let mut guard = self.sessions.lock().unwrap();
        let entry = guard
            .get_mut(session_id)
            .filter(|e| e.device_id == device_id)
            .ok_or(SessionError::NotFound)?;
        entry.subscribers += 1;
        entry.last_disconnected = None;
        let rx = entry.tx.subscribe();

        if let Some(last_id) = last_event_id {
            let earliest_seq = entry.history.front().map(|e| e.seq).unwrap_or(entry.next_seq);
            if last_id + 1 < earliest_seq {
                return Ok(ReplayResult::Reset { rx });
            }
            let events: Vec<CallbackEvent> = entry
                .history
                .iter()
                .filter(|e| e.seq > last_id)
                .cloned()
                .collect();
            Ok(ReplayResult::Events { events, rx })
        } else {
            Ok(ReplayResult::Events {
                events: Vec::new(),
                rx,
            })
        }
    }

    pub fn unsubscribe(&self, session_id: &str) {
        let mut guard = self.sessions.lock().unwrap();
        if let Some(entry) = guard.get_mut(session_id) {
            entry.subscribers = entry.subscribers.saturating_sub(1);
            if entry.subscribers == 0 {
                entry.last_disconnected = Some(Instant::now());
            }
        }
    }

    pub async fn invoke(
        &self,
        session_id: &str,
        device_id: &str,
        cmd: &str,
        body: InvokeRequestBody,
    ) -> Result<InvokeResult, SessionError> {
        let label = self
            .lookup(session_id, device_id)
            .ok_or(SessionError::NotFound)?;
        let (cb, err) = self.next_callback_pair();
        self.host
            .invoke(&label, cmd, body, cb, err)
            .await
            .map_err(SessionError::HostFailed)
    }

    pub fn close_for_device(&self, device_id: &str) {
        let labels: Vec<String> = {
            let mut guard = self.sessions.lock().unwrap();
            let mut label_map = self.label_to_id.lock().unwrap();
            let ids: Vec<String> = guard
                .iter()
                .filter(|(_, e)| e.device_id == device_id)
                .map(|(id, _)| id.clone())
                .collect();
            let mut labels = Vec::with_capacity(ids.len());
            for id in ids {
                if let Some(entry) = guard.remove(&id) {
                    label_map.remove(&entry.label);
                    labels.push(entry.label);
                }
            }
            labels
        };
        for label in labels {
            self.host.close_relay(&label);
        }
    }

    pub fn close_all(&self) {
        let labels: Vec<String> = {
            let mut guard = self.sessions.lock().unwrap();
            let mut label_map = self.label_to_id.lock().unwrap();
            label_map.clear();
            guard.drain().map(|(_, e)| e.label).collect()
        };
        for label in labels {
            self.host.close_relay(&label);
        }
    }

    pub fn sweep_expired(&self, now: Instant) {
        let labels: Vec<String> = {
            let mut guard = self.sessions.lock().unwrap();
            let mut label_map = self.label_to_id.lock().unwrap();
            let expired: Vec<String> = guard
                .iter()
                .filter(|(_, e)| {
                    e.subscribers == 0
                        && e.last_disconnected
                            .map(|t| now.saturating_duration_since(t) >= DISCONNECT_GRACE)
                            .unwrap_or(false)
                })
                .map(|(id, _)| id.clone())
                .collect();
            let mut labels = Vec::with_capacity(expired.len());
            for id in expired {
                if let Some(entry) = guard.remove(&id) {
                    label_map.remove(&entry.label);
                    labels.push(entry.label);
                }
            }
            labels
        };
        for label in labels {
            self.host.close_relay(&label);
        }
    }

    pub fn len(&self) -> usize {
        self.sessions.lock().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Spawns the background reaper that closes idle relay sessions after 60 s,
/// closes sessions for devices whose `app` grant was removed or revoked, and
/// closes all sessions when the listener stops.
pub fn spawn_reaper(
    sessions: Arc<SessionManager>,
    devices: Arc<DeviceStore>,
    mut stop: watch::Receiver<bool>,
) {
    let mut dev_rx = devices.subscribe();
    tauri::async_runtime::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(5));
        loop {
            tokio::select! {
                _ = tick.tick() => {
                    sessions.sweep_expired(Instant::now());
                }
                changed = dev_rx.changed() => {
                    if changed.is_err() {
                        break;
                    }
                    let device_ids: Vec<String> = {
                        let guard = sessions.sessions.lock().unwrap();
                        guard.values().map(|e| e.device_id.clone()).collect()
                    };
                    for dev_id in device_ids {
                        if !devices.has_app(&dev_id) {
                            sessions.close_for_device(&dev_id);
                        }
                    }
                }
                _ = stop.changed() => {
                    if *stop.borrow() {
                        break;
                    }
                }
            }
        }
        sessions.close_all();
    });
}

fn random_hex(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    if getrandom::fill(&mut buf).is_err() {
        return format!("{:016x}", Instant::now().elapsed().as_nanos());
    }
    buf.iter().map(|b| format!("{b:02x}")).collect()
}

/// Forwards every callback (event or channel) received inside a `remote-*`
/// relay webview to Rust. Only callable by `remote-*` webviews.
#[tauri::command]
pub fn remote_relay_push(
    webview: tauri::Webview,
    state: tauri::State<'_, super::super::WebRemoteState>,
    id: u32,
    payload: serde_json::Value,
) -> Result<(), String> {
    if !webview.label().starts_with("remote-") {
        return Err("only relay webviews may call remote_relay_push".into());
    }
    if let Some(sessions) = state.relay_sessions() {
        sessions.push_callback(webview.label(), id, payload);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct FakeHost {
        opened: Mutex<Vec<String>>,
        closed: Mutex<Vec<String>>,
    }

    impl RelayHost for FakeHost {
        fn open_relay<'a>(
            &'a self,
            label: &'a str,
        ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
            Box::pin(async move {
                self.opened.lock().unwrap().push(label.to_string());
                Ok(())
            })
        }

        fn close_relay(&self, label: &str) {
            self.closed.lock().unwrap().push(label.to_string());
        }

        fn invoke<'a>(
            &'a self,
            _label: &'a str,
            _cmd: &'a str,
            _body: InvokeRequestBody,
            _callback_id: u32,
            _error_id: u32,
        ) -> Pin<Box<dyn Future<Output = Result<InvokeResult, String>> + Send + 'a>> {
            Box::pin(async move { Ok(InvokeResult::OkJson("null".into())) })
        }
    }

    #[tokio::test]
    async fn caps_at_three_sessions_and_reuses_owned_session() {
        let host = Arc::new(FakeHost::default());
        let mgr = SessionManager::new(host.clone());

        let s0 = mgr.create_or_reuse("dev-a", None).await.unwrap();
        let s1 = mgr.create_or_reuse("dev-a", None).await.unwrap();
        let s2 = mgr.create_or_reuse("dev-b", None).await.unwrap();
        assert_eq!(s0.label, "remote-0");
        assert_eq!(s1.label, "remote-1");
        assert_eq!(s2.label, "remote-2");

        // Reusing an owned session succeeds even at cap.
        let reused = mgr.create_or_reuse("dev-a", Some(&s0.id)).await.unwrap();
        assert_eq!(reused.id, s0.id);

        // Fourth distinct session is rejected.
        assert_eq!(
            mgr.create_or_reuse("dev-c", None).await.unwrap_err(),
            SessionError::TooManySessions
        );
    }

    #[tokio::test]
    async fn callbacks_replay_and_reset_when_evicted() {
        let host = Arc::new(FakeHost::default());
        let mgr = SessionManager::new(host);
        let s = mgr.create_or_reuse("dev-a", None).await.unwrap();

        assert!(mgr.push_callback(&s.label, 10, serde_json::json!({"a": 1})));
        assert!(mgr.push_callback(&s.label, 11, serde_json::json!({"a": 2})));

        match mgr.subscribe(&s.id, "dev-a", Some(1)).unwrap() {
            ReplayResult::Events { events, .. } => {
                assert_eq!(events.len(), 1);
                assert_eq!(events[0].seq, 2);
                assert_eq!(events[0].id, 11);
            }
            ReplayResult::Reset { .. } => panic!("expected replay"),
        }
        mgr.unsubscribe(&s.id);

        // Overflow REPLAY_MAX_EVENTS so seq 1 is evicted.
        for i in 0..REPLAY_MAX_EVENTS {
            assert!(mgr.push_callback(&s.label, 20, serde_json::json!(i)));
        }
        match mgr.subscribe(&s.id, "dev-a", Some(1)).unwrap() {
            ReplayResult::Reset { .. } => {}
            ReplayResult::Events { .. } => panic!("expected reset after eviction"),
        }
    }

    #[tokio::test]
    async fn closes_after_grace_and_on_device_revoke_or_close_all() {
        let host = Arc::new(FakeHost::default());
        let mgr = SessionManager::new(host.clone());
        let s0 = mgr.create_or_reuse("dev-a", None).await.unwrap();
        let s1 = mgr.create_or_reuse("dev-b", None).await.unwrap();

        // Active subscriber prevents expiry.
        let _sub = mgr.subscribe(&s0.id, "dev-a", None).unwrap();
        let future = Instant::now() + DISCONNECT_GRACE + Duration::from_secs(1);
        mgr.sweep_expired(future);
        assert!(mgr.lookup(&s0.id, "dev-a").is_some());
        assert!(mgr.lookup(&s1.id, "dev-b").is_none());

        // Disconnecting and sweeping after 60s closes s0.
        mgr.unsubscribe(&s0.id);
        let future2 = Instant::now() + DISCONNECT_GRACE + Duration::from_secs(1);
        mgr.sweep_expired(future2);
        assert!(mgr.is_empty());

        let s2 = mgr.create_or_reuse("dev-c", None).await.unwrap();
        mgr.close_for_device("dev-c");
        assert!(mgr.lookup(&s2.id, "dev-c").is_none());
        assert_eq!(
            host.closed.lock().unwrap().as_slice(),
            &["remote-1", "remote-0", "remote-2"]
        );
    }
}
