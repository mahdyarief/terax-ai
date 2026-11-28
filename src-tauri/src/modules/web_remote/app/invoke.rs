//! HTTP handlers for `/api/app/session` and `/api/app/invoke`.

use bytes::Bytes;
use http_body_util::{BodyExt, Full, Limited};
use hyper::header::{CONTENT_TYPE, HOST};
use hyper::{Request, Response, StatusCode};
use serde::Deserialize;

use super::policy::{self, CommandClass, Transport};
use super::sessions::{InvokeRequestBody, InvokeResult, SessionError};
use super::super::server::{caller, json, secure, text, Body, Deps};

const MAX_READ_BYTES: usize = 64 * 1024;
const MAX_WRITE_BYTES: usize = 1024 * 1024;
const MAX_CONTROL_BYTES: usize = 8 * 1024 * 1024;
const MAX_SECRET_BYTES: usize = 16 * 1024;

pub(crate) fn transport_for(deps: &Deps, headers: &hyper::header::HeaderMap) -> Transport {
    if deps.https {
        return Transport::Https;
    }
    let host = headers
        .get(HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let name = host
        .strip_prefix('[')
        .and_then(|s| s.split(']').next())
        .unwrap_or_else(|| host.split(':').next().unwrap_or(""));
    if matches!(name, "localhost" | "127.0.0.1" | "::1") {
        Transport::Loopback
    } else {
        Transport::PlainRemote
    }
}

fn max_body_for(class: CommandClass) -> usize {
    match class {
        CommandClass::Read => MAX_READ_BYTES,
        CommandClass::Write => MAX_WRITE_BYTES,
        CommandClass::Control => MAX_CONTROL_BYTES,
        CommandClass::Secret => MAX_SECRET_BYTES,
        CommandClass::Native => 0,
    }
}

#[derive(Deserialize)]
struct SessionRequest {
    #[serde(default)]
    session: Option<String>,
}

pub(crate) async fn handle_session(deps: &Deps, request: Request<Incoming>) -> Response<Body> {
    let Some(caller) = caller(deps, request.headers()) else {
        return secure(text(StatusCode::UNAUTHORIZED, "Pair this device first."));
    };
    if !caller.grant.app {
        return secure(text(
            StatusCode::FORBIDDEN,
            "This device may not open the full app.",
        ));
    }
    let Some(relay) = &deps.relay else {
        return secure(text(
            StatusCode::SERVICE_UNAVAILABLE,
            "Relay sessions are unavailable.",
        ));
    };
    let body = match read_limited(request.into_body(), 4096).await {
        Ok(b) => b,
        Err(resp) => return resp,
    };
    let requested = if body.is_empty() {
        None
    } else {
        serde_json::from_slice::<SessionRequest>(&body)
            .ok()
            .and_then(|r| r.session)
    };
    match relay.create_or_reuse(&caller.id, requested.as_deref()).await {
        Ok(info) => secure(json(StatusCode::OK, &info)),
        Err(SessionError::TooManySessions) => secure(text(
            StatusCode::TOO_MANY_REQUESTS,
            "Too many active remote app sessions.",
        )),
        Err(SessionError::NotFound) => secure(text(StatusCode::NOT_FOUND, "Session not found.")),
        Err(SessionError::HostFailed(err)) => {
            secure(text(StatusCode::INTERNAL_SERVER_ERROR, &err))
        }
    }
}

#[derive(Deserialize)]
struct InvokeJsonPayload {
    session: String,
    cmd: String,
    #[serde(default = "default_args")]
    args: serde_json::Value,
}

fn default_args() -> serde_json::Value {
    serde_json::json!({})
}

use hyper::body::Incoming;

pub(crate) async fn handle_invoke(deps: &Deps, request: Request<Incoming>) -> Response<Body> {
    let Some(caller) = caller(deps, request.headers()) else {
        return secure(text(StatusCode::UNAUTHORIZED, "Pair this device first."));
    };
    if !caller.grant.app {
        return secure(text(
            StatusCode::FORBIDDEN,
            "This device may not invoke app commands.",
        ));
    }
    let Some(relay) = &deps.relay else {
        return secure(text(
            StatusCode::SERVICE_UNAVAILABLE,
            "Relay sessions are unavailable.",
        ));
    };

    let transport = transport_for(deps, request.headers());
    let header_cmd = request
        .headers()
        .get("x-terax-cmd")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let header_session = request
        .headers()
        .get("x-terax-session")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);

    let (session_id, cmd, body) = if let (Some(cmd), Some(session_id)) = (header_cmd, header_session)
    {
        let Some(class) = policy::class(&cmd) else {
            return secure(text(StatusCode::FORBIDDEN, "Command is not allowed."));
        };
        if !policy::allowed_class(class, &cmd, caller.grant.app, transport) {
            return secure(text(StatusCode::FORBIDDEN, "Command is not allowed."));
        }
        let bytes = match read_limited(request.into_body(), max_body_for(class)).await {
            Ok(b) => b,
            Err(resp) => return resp,
        };
        (session_id, cmd, InvokeRequestBody::Raw(bytes))
    } else {
        let bytes = match read_limited(request.into_body(), MAX_CONTROL_BYTES).await {
            Ok(b) => b,
            Err(resp) => return resp,
        };
        let Ok(payload) = serde_json::from_slice::<InvokeJsonPayload>(&bytes) else {
            return secure(text(StatusCode::BAD_REQUEST, "Invalid invoke payload."));
        };
        let Some(class) = policy::class(&payload.cmd) else {
            return secure(text(StatusCode::FORBIDDEN, "Command is not allowed."));
        };
        if !policy::allowed_class(class, &payload.cmd, caller.grant.app, transport) {
            return secure(text(StatusCode::FORBIDDEN, "Command is not allowed."));
        }
        if bytes.len() > max_body_for(class) {
            return secure(text(
                StatusCode::PAYLOAD_TOO_LARGE,
                "That request body is too large.",
            ));
        }
        (
            payload.session,
            payload.cmd,
            InvokeRequestBody::Json(payload.args),
        )
    };

    match relay.invoke(&session_id, &caller.id, &cmd, body).await {
        Ok(InvokeResult::OkJson(s)) => secure(
            Response::builder()
                .status(StatusCode::OK)
                .header(CONTENT_TYPE, "application/json")
                .body(Full::new(Bytes::from(s)).boxed())
                .unwrap(),
        ),
        Ok(InvokeResult::OkRaw(b)) => secure(
            Response::builder()
                .status(StatusCode::OK)
                .header(CONTENT_TYPE, "application/octet-stream")
                .body(Full::new(Bytes::from(b)).boxed())
                .unwrap(),
        ),
        Ok(InvokeResult::Err(err)) => secure(json(StatusCode::BAD_REQUEST, &err)),
        Err(SessionError::NotFound) => secure(text(StatusCode::NOT_FOUND, "Session not found.")),
        Err(SessionError::TooManySessions) => secure(text(
            StatusCode::TOO_MANY_REQUESTS,
            "Too many active remote app sessions.",
        )),
        Err(SessionError::HostFailed(err)) => secure(text(StatusCode::BAD_GATEWAY, &err)),
    }
}

async fn read_limited(body: Incoming, limit: usize) -> Result<Vec<u8>, Response<Body>> {
    let collected = Limited::new(body, limit)
        .collect()
        .await
        .map_err(|_| {
            secure(text(
                StatusCode::PAYLOAD_TOO_LARGE,
                "That request body is too large.",
            ))
        })?;
    Ok(collected.to_bytes().to_vec())
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::{Arc, Mutex};

    use super::super::sessions::{RelayHost, SessionManager};
    use super::*;

    struct MockHost {
        calls: Mutex<Vec<(String, String)>>,
    }

    impl MockHost {
        fn new() -> Self {
            Self {
                calls: Mutex::new(Vec::new()),
            }
        }
    }

    impl RelayHost for MockHost {
        fn open_relay<'a>(
            &'a self,
            _label: &'a str,
        ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
            Box::pin(async { Ok(()) })
        }

        fn close_relay(&self, _label: &str) {}

        fn invoke<'a>(
            &'a self,
            label: &'a str,
            cmd: &'a str,
            body: InvokeRequestBody,
            _callback_id: u32,
            _error_id: u32,
        ) -> Pin<Box<dyn Future<Output = Result<InvokeResult, String>> + Send + 'a>> {
            let label = label.to_string();
            let cmd = cmd.to_string();
            Box::pin(async move {
                self.calls.lock().unwrap().push((label, cmd.clone()));
                match body {
                    InvokeRequestBody::Raw(b) => Ok(InvokeResult::OkRaw(b)),
                    InvokeRequestBody::Json(_) => {
                        if cmd == "pty_list_shells" {
                            Ok(InvokeResult::OkJson("[\"bash\"]".into()))
                        } else {
                            Ok(InvokeResult::Err(serde_json::json!("boom")))
                        }
                    }
                }
            })
        }
    }

    #[tokio::test]
    async fn invoke_enforces_grant_policy_and_forwards_json_and_raw() {
        use std::sync::atomic::AtomicUsize;
        use tokio::sync::watch;

        use crate::modules::web_remote::devices::{DeviceStore, Grant, Pairing};
        use crate::modules::web_remote::http_rules::HostPolicy;
        use crate::modules::web_remote::server::{accept_loop, Assets, TabEvents};
        use crate::modules::web_remote::stream::{PtySessions, StreamLimits};
        use crate::modules::web_remote::tabs::TabRegistry;

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

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let devices = Arc::new(DeviceStore::memory());
        let (info, token) = devices.add("browser");
        let host = Arc::new(MockHost::new());
        let relay = Arc::new(SessionManager::new(host.clone()));
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
            relay: Some(relay),
        };
        tokio::spawn(accept_loop(listener, deps, stop_rx));

        let client = reqwest_free_post(port, &token);

        // No app grant -> 403
        let (status, _) = client("/api/app/session", &[], b"{}").await;
        assert_eq!(status, 403);

        // Grant app -> 200
        assert!(devices.update(
            &info.id,
            Grant {
                input: false,
                tabs: false,
                app: true,
            }
        ));
        let (status, body) = client("/api/app/session", &[], b"{}").await;
        assert_eq!(status, 200);
        let sess: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let session_id = sess["id"].as_str().unwrap().to_string();

        // Native command -> 403
        let native_body = serde_json::json!({
            "session": session_id,
            "cmd": "open_settings_window",
            "args": {}
        });
        let (status, _) = client(
            "/api/app/invoke",
            &[],
            serde_json::to_vec(&native_body).unwrap().as_slice(),
        )
        .await;
        assert_eq!(status, 403);

        // Allowed JSON command -> 200
        let ok_body = serde_json::json!({
            "session": session_id,
            "cmd": "pty_list_shells",
            "args": {}
        });
        let (status, body) = client(
            "/api/app/invoke",
            &[],
            serde_json::to_vec(&ok_body).unwrap().as_slice(),
        )
        .await;
        assert_eq!(status, 200);
        assert_eq!(body, b"[\"bash\"]");

        // Error response -> 400
        let err_body = serde_json::json!({
            "session": session_id,
            "cmd": "pty_diagnostics",
            "args": {}
        });
        let (status, body) = client(
            "/api/app/invoke",
            &[],
            serde_json::to_vec(&err_body).unwrap().as_slice(),
        )
        .await;
        assert_eq!(status, 400);
        assert_eq!(body, b"\"boom\"");

        // Raw invoke -> 200 raw bytes
        let (status, body) = client(
            "/api/app/invoke",
            &[
                ("X-Terax-Session", &session_id),
                ("X-Terax-Cmd", "fs_write_file"),
            ],
            b"\x01\x02\x03",
        )
        .await;
        assert_eq!(status, 200);
        assert_eq!(body, b"\x01\x02\x03");
    }

    fn reqwest_free_post(
        port: u16,
        token: &str,
    ) -> impl Fn(
        &str,
        &[(&str, &str)],
        &[u8],
    ) -> Pin<Box<dyn Future<Output = (u16, Vec<u8>)> + Send>> {
        let token = token.to_string();
        move |path: &str, extra: &[(&str, &str)], body: &[u8]| {
            let path = path.to_string();
            let extra: Vec<(String, String)> = extra
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect();
            let body = body.to_vec();
            let token = token.clone();
            Box::pin(async move {
                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
                    .await
                    .unwrap();
                let mut req = format!(
                    "POST {path} HTTP/1.1\r\nHost: localhost:{port}\r\nOrigin: http://localhost:{port}\r\nX-Terax-Remote: 1\r\nCookie: terax_remote={token}\r\nContent-Length: {}\r\nConnection: close\r\n",
                    body.len()
                );
                for (k, v) in extra {
                    req.push_str(&format!("{k}: {v}\r\n"));
                }
                req.push_str("\r\n");
                stream.write_all(req.as_bytes()).await.unwrap();
                stream.write_all(&body).await.unwrap();
                let mut raw = Vec::new();
                stream.read_to_end(&mut raw).await.unwrap();
                let split = raw
                    .windows(4)
                    .position(|w| w == b"\r\n\r\n")
                    .unwrap();
                let head = String::from_utf8_lossy(&raw[..split]);
                let status: u16 = head
                    .lines()
                    .next()
                    .unwrap()
                    .split_whitespace()
                    .nth(1)
                    .unwrap()
                    .parse()
                    .unwrap();
                (status, raw[split + 4..].to_vec())
            })
        }
    }
}
