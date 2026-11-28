use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::ipc::{CallbackFn, Channel, InvokeBody, InvokeResponse};
use tauri::webview::InvokeRequest;
use tauri::{Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
use tokio::sync::oneshot;

use super::WebRemoteState;
use crate::modules::pty::hub::Reply;

/// Whether `/spike/check` answers. Off unless `TERAX_REMOTE_SPIKE=1`, since
/// the route has no auth.
pub fn enabled() -> bool {
    std::env::var("TERAX_REMOTE_SPIKE").as_deref() == Ok("1")
}

/// Runs the checks for `/spike/check`.
pub trait SpikeRunner: Send + Sync {
    fn run(&self) -> std::pin::Pin<Box<dyn std::future::Future<Output = serde_json::Value> + Send + '_>>;
}

pub struct AppSpike(pub tauri::AppHandle);

impl SpikeRunner for AppSpike {
    fn run(&self) -> std::pin::Pin<Box<dyn std::future::Future<Output = serde_json::Value> + Send + '_>> {
        Box::pin(async move {
            let state = self.0.state::<WebRemoteState>();
            run_spike_checks(&self.0, &state).await
        })
    }
}

#[derive(Default)]
pub struct SpikeEvents {
    pub received: Mutex<Vec<(u32, serde_json::Value)>>,
}

#[tauri::command]
pub fn spike_relay_push(
    state: tauri::State<'_, WebRemoteState>,
    id: u32,
    payload: serde_json::Value,
) {
    state.spike.received.lock().unwrap().push((id, payload));
}

#[tauri::command]
pub fn spike_test_channel(on_msg: Channel<serde_json::Value>, large: bool) -> Result<(), String> {
    let text = if large {
        "x".repeat(12_000)
    } else {
        "small".to_string()
    };
    on_msg
        .send(serde_json::json!({ "kind": "spike", "large": large, "text": text }))
        .map_err(|e| e.to_string())
}

pub fn ensure_relay_window(app: &tauri::AppHandle) -> Result<tauri::Webview, String> {
    if let Some(wv) = app.get_webview_window("remote-0") {
        return Ok(wv.as_ref().clone());
    }
    let window = WebviewWindowBuilder::new(
        app,
        "remote-0",
        WebviewUrl::App("remote-relay.html".into()),
    )
    .title("Terax Remote Relay")
    .visible(false)
    .skip_taskbar(true)
    .build()
    .map_err(|e| format!("build remote-0 failed: {e}"))?;
    Ok(window.as_ref().clone())
}

async fn dispatch_invoke(
    app: &tauri::AppHandle,
    webview: &tauri::Webview,
    cmd: &str,
    args: serde_json::Value,
    callback_id: u32,
) -> Result<(bool, serde_json::Value), String> {
    let url = webview.url().map_err(|e| e.to_string())?;
    let (tx, rx) = oneshot::channel();
    let tx = std::sync::Mutex::new(Some(tx));
    let req = InvokeRequest {
        cmd: cmd.to_string(),
        callback: CallbackFn(callback_id),
        error: CallbackFn(callback_id.saturating_add(1)),
        url,
        body: InvokeBody::Json(args),
        headers: Default::default(),
        invoke_key: app.invoke_key().to_string(),
    };
    webview.clone().on_message(
        req,
        Box::new(move |_wv, _cmd, response, _cb, _err| {
            let result = match response {
                InvokeResponse::Ok(body) => match body {
                    tauri::ipc::InvokeResponseBody::Json(s) => {
                        let v = serde_json::from_str(&s).unwrap_or(serde_json::Value::String(s));
                        (true, v)
                    }
                    tauri::ipc::InvokeResponseBody::Raw(b) => {
                        (true, serde_json::json!({ "rawBytes": b.len() }))
                    }
                },
                InvokeResponse::Err(err) => (false, err.0),
            };
            if let Some(sender) = tx.lock().unwrap().take() {
                let _ = sender.send(result);
            }
        }),
    );
    tokio::time::timeout(Duration::from_secs(5), rx)
        .await
        .map_err(|_| format!("timeout waiting for {cmd}"))?
        .map_err(|_| format!("responder dropped for {cmd}"))
}

async fn wait_for_relay_id(
    state: &WebRemoteState,
    target_id: u32,
    timeout: Duration,
) -> Vec<serde_json::Value> {
    let deadline = Instant::now() + timeout;
    loop {
        {
            let guard = state.spike.received.lock().unwrap();
            let matches: Vec<_> = guard
                .iter()
                .filter(|(id, _)| *id == target_id)
                .map(|(_, v)| v.clone())
                .collect();
            if !matches.is_empty() {
                return matches;
            }
        }
        if Instant::now() >= deadline {
            return Vec::new();
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

struct SpikeReply(std::sync::Mutex<Option<oneshot::Sender<(u16, Vec<u8>)>>>);

impl Reply for SpikeReply {
    fn send(self: Box<Self>, status: u16, body: Vec<u8>) {
        if let Some(tx) = self.0.lock().unwrap().take() {
            let _ = tx.send((status, body));
        }
    }
}

pub async fn run_spike_checks(app: &tauri::AppHandle, state: &WebRemoteState) -> serde_json::Value {
    state.spike.received.lock().unwrap().clear();

    // 1. Hidden window remote-0
    let webview = match ensure_relay_window(app) {
        Ok(wv) => wv,
        Err(e) => {
            return serde_json::json!({ "all_passed": false, "error": e });
        }
    };
    let mut url = webview.url().map(|u| u.to_string()).unwrap_or_default();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !url.contains("remote-relay.html") && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(100)).await;
        url = webview.url().map(|u| u.to_string()).unwrap_or_default();
    }
    let check1 = url.contains("remote-relay.html");

    // Give the page script a moment to run after navigation commits.
    tokio::time::sleep(Duration::from_millis(300)).await;

    // 2. Invoke pty_list_shells + ACL denial check
    let invoke_ok = dispatch_invoke(app, &webview, "pty_list_shells", serde_json::json!({}), 10)
        .await;
    let acl_denied = dispatch_invoke(
        app,
        &webview,
        "plugin:window|set_always_on_top",
        serde_json::json!({ "label": "remote-0", "alwaysOnTop": true }),
        20,
    )
    .await;
    let check2 = matches!(&invoke_ok, Ok((true, v)) if v.is_array())
        && matches!(&acl_denied, Ok((false, _)));

    // 3. Event listen + emit + relay callback
    let listen_res = dispatch_invoke(
        app,
        &webview,
        "plugin:event|listen",
        serde_json::json!({
            "event": "fs:changed",
            "target": { "kind": "Any" },
            "handler": 900001
        }),
        30,
    )
    .await;
    let _ = app.emit("fs:changed", serde_json::json!({ "spike": "event-ok" }));
    let event_hits = wait_for_relay_id(state, 900001, Duration::from_secs(3)).await;
    let check3 = matches!(&listen_res, Ok((true, _))) && !event_hits.is_empty();

    // 4. Channel small + large (fetch path)
    let ch_small_res = dispatch_invoke(
        app,
        &webview,
        "spike_test_channel",
        serde_json::json!({ "onMsg": "__CHANNEL__:900002", "large": false }),
        40,
    )
    .await;
    let small_hits = wait_for_relay_id(state, 900002, Duration::from_secs(3)).await;

    let ch_large_res = dispatch_invoke(
        app,
        &webview,
        "spike_test_channel",
        serde_json::json!({ "onMsg": "__CHANNEL__:900003", "large": true }),
        50,
    )
    .await;
    let large_hits = wait_for_relay_id(state, 900003, Duration::from_secs(3)).await;
    let large_len = large_hits
        .iter()
        .filter_map(|v| v.get("message")?.get("text")?.as_str().map(|s| s.len()))
        .max()
        .unwrap_or(0);
    let check4 = matches!(&ch_small_res, Ok((true, _)))
        && !small_hits.is_empty()
        && matches!(&ch_large_res, Ok((true, _)))
        && large_len == 12_000;

    // 5. pty::stream_protocol core with remote-0 label
    let (tx, rx) = oneshot::channel();
    crate::modules::pty::protocol::dispatch_for_label(
        app,
        "remote-0",
        &tauri::http::Method::POST,
        "/ack",
        None,
        &[],
        Box::new(SpikeReply(std::sync::Mutex::new(Some(tx)))),
    );
    let scheme_res = tokio::time::timeout(Duration::from_secs(2), rx).await;
    let check5 = matches!(scheme_res, Ok(Ok((status, _))) if status == 400 || status == 403);

    serde_json::json!({
        "check1_window": { "passed": check1, "url": url },
        "check2_invoke_and_acl": {
            "passed": check2,
            "pty_list_shells": format!("{invoke_ok:?}"),
            "acl_denied": format!("{acl_denied:?}"),
        },
        "check3_event_relay": {
            "passed": check3,
            "listen": format!("{listen_res:?}"),
            "hits": event_hits,
        },
        "check4_channel": {
            "passed": check4,
            "small_hits": small_hits.len(),
            "large_len": large_len,
            "large_hits": large_hits.len(),
        },
        "check5_pty_scheme": {
            "passed": check5,
            "result": format!("{scheme_res:?}"),
        },
        "all_passed": check1 && check2 && check3 && check4 && check5,
    })
}

