//! SSE stream `GET /api/app/callbacks?session=<id>` with `Last-Event-ID` replay.

use std::sync::Arc;

use hyper::body::Incoming;
use hyper::{Request, Response, StatusCode};
use tokio::sync::{broadcast, mpsc};

use super::sessions::{CallbackEvent, ReplayResult, SessionError, SessionManager};
use super::super::server::{caller, secure, text, Body, Deps};
use super::super::sse;

struct UnsubscribeGuard {
    relay: Arc<SessionManager>,
    session_id: String,
}

impl Drop for UnsubscribeGuard {
    fn drop(&mut self) {
        self.relay.unsubscribe(&self.session_id);
    }
}

fn query_session(query: Option<&str>) -> Option<String> {
    let query = query?;
    for pair in query.split('&') {
        if let Some((k, v)) = pair.split_once('=') {
            if k == "session" && !v.is_empty() {
                return Some(v.to_string());
            }
        }
    }
    None
}

fn encode_callback(ev: &CallbackEvent) -> String {
    serde_json::json!({
        "id": ev.id,
        "payload": ev.payload,
    })
    .to_string()
}

pub(crate) fn handle_callbacks(deps: &Deps, request: Request<Incoming>) -> Response<Body> {
    let Some(caller) = caller(deps, request.headers()) else {
        return secure(text(StatusCode::UNAUTHORIZED, "Pair this device first."));
    };
    if !caller.grant.app {
        return secure(text(
            StatusCode::FORBIDDEN,
            "This device may not stream app callbacks.",
        ));
    }
    let Some(session_id) = query_session(request.uri().query()) else {
        return secure(text(StatusCode::BAD_REQUEST, "Missing session query."));
    };
    let Some(relay) = deps.relay.clone() else {
        return secure(text(
            StatusCode::SERVICE_UNAVAILABLE,
            "Relay sessions are unavailable.",
        ));
    };

    let last_id = sse::last_event_id(request.headers());
    let sub = match relay.subscribe(&session_id, &caller.id, last_id) {
        Ok(sub) => sub,
        Err(SessionError::NotFound) => {
            return secure(text(StatusCode::NOT_FOUND, "Session not found."));
        }
        Err(_) => {
            return secure(text(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Could not subscribe to callbacks.",
            ));
        }
    };

    let (tx, rx) = mpsc::channel(sse::STREAM_BUFFER);
    let devices = deps.devices.clone();
    let guard = sse::ViewerGuard::open(&deps.viewers);
    let unsub = UnsubscribeGuard {
        relay,
        session_id,
    };

    tauri::async_runtime::spawn(async move {
        let _guard = guard;
        let _unsub = unsub;
        let mut device_updates = devices.subscribe();
        let mut heartbeat = tokio::time::interval(sse::HEARTBEAT);
        heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        let mut rx_live = match sub {
            ReplayResult::Reset { .. } => {
                sse::send(&tx, sse::event(None, "reset", "{}"));
                return;
            }
            ReplayResult::Events { events, rx } => {
                for ev in events {
                    if tx.is_closed() {
                        return;
                    }
                    sse::send(&tx, sse::event(Some(ev.seq), "callback", &encode_callback(&ev)));
                }
                rx
            }
        };

        loop {
            tokio::select! {
                _ = tx.closed() => break,
                _ = heartbeat.tick() => {
                    sse::send(&tx, sse::comment("ping"));
                }
                msg = rx_live.recv() => {
                    match msg {
                        Ok(ev) => {
                            sse::send(&tx, sse::event(Some(ev.seq), "callback", &encode_callback(&ev)));
                        }
                        Err(broadcast::error::RecvError::Lagged(_)) => {
                            sse::send(&tx, sse::event(None, "reset", "{}"));
                            break;
                        }
                        Err(broadcast::error::RecvError::Closed) => break,
                    }
                }
                _ = device_updates.changed() => {
                    if !devices.has_app(&caller.id) {
                        break;
                    }
                }
            }
        }
    });

    secure(sse::response(rx))
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::atomic::AtomicUsize;
    use std::sync::Arc;

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::sync::watch;

    use super::super::sessions::{InvokeRequestBody, InvokeResult, RelayHost, SessionManager};
    use crate::modules::web_remote::devices::{DeviceStore, Grant, Pairing};
    use crate::modules::web_remote::http_rules::HostPolicy;
    use crate::modules::web_remote::server::{accept_loop, Assets, Deps, TabEvents};
    use crate::modules::web_remote::stream::{PtySessions, StreamLimits};
    use crate::modules::web_remote::tabs::TabRegistry;

    struct DummyHost;
    impl RelayHost for DummyHost {
        fn open_relay<'a>(
            &'a self,
            _label: &'a str,
        ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
            Box::pin(async { Ok(()) })
        }
        fn close_relay(&self, _label: &str) {}
        fn invoke<'a>(
            &'a self,
            _label: &'a str,
            _cmd: &'a str,
            _body: InvokeRequestBody,
            _headers: Vec<(String, String)>,
            _callback_id: u32,
            _error_id: u32,
        ) -> Pin<Box<dyn Future<Output = Result<InvokeResult, String>> + Send + 'a>> {
            Box::pin(async { Ok(InvokeResult::OkJson("null".into())) })
        }
    }
    struct NoAssets;
    impl Assets for NoAssets {
        fn read(&self, _: &str) -> Option<Vec<u8>> {
            None
        }
    }
    struct NoPtys;
    impl PtySessions for NoPtys {
        fn terminal(
            &self,
            _: u32,
        ) -> Option<Arc<dyn crate::modules::web_remote::stream::Terminal>> {
            None
        }
    }
    struct NoEvents;
    impl TabEvents for NoEvents {
        fn emit(&self, _: &str, _: &crate::modules::web_remote::server::TabAction) {}
    }

    #[tokio::test]
    async fn callbacks_stream_replays_by_last_event_id_and_delivers_live() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let devices = Arc::new(DeviceStore::memory());
        let (info, token) = devices.add("browser");
        assert!(devices.update(
            &info.id,
            Grant {
                input: false,
                tabs: false,
                app: true,
            }
        ));
        let relay = Arc::new(SessionManager::new(Arc::new(DummyHost)));
        let sess = relay.create_or_reuse(&info.id, None).await.unwrap();
        relay.push_callback(&sess.label, 10, serde_json::json!({"n": 1}));
        relay.push_callback(&sess.label, 11, serde_json::json!({"n": 2}));

        let (_stop_tx, stop_rx) = watch::channel(false);
        let deps = Deps {
            assets: Arc::new(NoAssets),
            tabs: Arc::new(TabRegistry::new()),
            hosts: HostPolicy::loopback(port),
            viewers: Arc::new(AtomicUsize::new(0)),
            devices: devices.clone(),
            pairing: Arc::new(Pairing::default()),
            sessions: Arc::new(NoPtys),
            limits: Arc::new(StreamLimits::default()),
            events: Arc::new(NoEvents),
            https: false,
            relay: Some(relay.clone()),
            schemes: None,
            ports: None,
            audit: None,
            rate_limit: None,
        };
        tokio::spawn(accept_loop(listener, deps, stop_rx));

        let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .unwrap();
        let req = format!(
            "GET /api/app/callbacks?session={} HTTP/1.1\r\nHost: localhost:{port}\r\nCookie: terax_remote={token}\r\nLast-Event-ID: 1\r\n\r\n",
            sess.id
        );
        stream.write_all(req.as_bytes()).await.unwrap();

        // Push a live event shortly after connecting.
        let relay_push = relay.clone();
        let label = sess.label.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            relay_push.push_callback(&label, 12, serde_json::json!({"n": 3}));
        });

        let mut buf = vec![0u8; 4096];
        let mut collected = String::new();
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(2);
        while tokio::time::Instant::now() < deadline && !collected.contains("id: 3") {
            let n = tokio::time::timeout(
                std::time::Duration::from_millis(500),
                stream.read(&mut buf),
            )
            .await
            .unwrap()
            .unwrap();
            if n == 0 {
                break;
            }
            collected.push_str(&String::from_utf8_lossy(&buf[..n]));
        }
        assert!(!collected.contains("id: 1\n"), "{collected}");
        assert!(collected.contains("id: 2\nevent: callback\n"), "{collected}");
        assert!(collected.contains("id: 3\nevent: callback\n"), "{collected}");
    }
}
