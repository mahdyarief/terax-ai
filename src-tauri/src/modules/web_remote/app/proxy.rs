//! Reverse proxy `/proxy/<port>/<rest>` to `127.0.0.1:<port>` for ports
//! Terax itself reports (live servers and detected dev servers), including
//! WebSocket upgrades for HMR.

use std::collections::HashSet;
use std::sync::Mutex;

use http_body_util::BodyExt;
use hyper::body::Incoming;
use hyper::header::{CONNECTION, HOST, HeaderValue, UPGRADE};
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use tauri::Manager;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use super::super::server::{caller, full, secure, text, Body, Deps};
use crate::modules::live_server::LiveServers;

pub trait PortAllowlist: Send + Sync {
    fn is_allowed(&self, port: u16) -> bool;
}

#[derive(Default)]
pub struct ReportedPorts(Mutex<HashSet<u16>>);

impl ReportedPorts {
    pub fn replace(&self, ports: impl IntoIterator<Item = u16>) {
        let mut guard = self.0.lock().unwrap();
        guard.clear();
        for p in ports {
            if p > 0 {
                guard.insert(p);
            }
        }
    }

    pub fn contains(&self, port: u16) -> bool {
        self.0.lock().unwrap().contains(&port)
    }
}

pub struct AppPortAllowlist(pub tauri::AppHandle);

impl PortAllowlist for AppPortAllowlist {
    fn is_allowed(&self, port: u16) -> bool {
        if let Some(live) = self.0.try_state::<LiveServers>() {
            if live.list().iter().any(|s| s.port == port) {
                return true;
            }
        }
        if let Some(remote) = self.0.try_state::<super::super::WebRemoteState>() {
            if remote.reported_ports.contains(port) {
                return true;
            }
        }
        false
    }
}

#[tauri::command]
pub fn remote_report_ports(
    state: tauri::State<'_, super::super::WebRemoteState>,
    ports: Vec<u16>,
) {
    state.reported_ports.replace(ports);
}

pub(crate) fn parse_proxy_path(path: &str) -> Option<(u16, String)> {
    let rest = path.strip_prefix("/proxy/")?;
    let (port_str, tail) = match rest.split_once('/') {
        Some((p, t)) => (p, format!("/{t}")),
        None => (rest, "/".to_string()),
    };
    let port: u16 = port_str.parse().ok()?;
    if port == 0 {
        return None;
    }
    Some((port, tail))
}

fn is_websocket_upgrade(headers: &hyper::HeaderMap) -> bool {
    let has_upgrade = headers
        .get(CONNECTION)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.to_ascii_lowercase().contains("upgrade"));
    let is_ws = headers
        .get(UPGRADE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.eq_ignore_ascii_case("websocket"));
    has_upgrade && is_ws
}

pub(crate) async fn handle_proxy(deps: &Deps, mut request: Request<Incoming>) -> Response<Body> {
    let path = request.uri().path().to_string();
    let Some((port, inner_path)) = parse_proxy_path(&path) else {
        return secure(text(StatusCode::NOT_FOUND, "Not found."));
    };
    let Some(caller) = caller(deps, request.headers()) else {
        return secure(text(StatusCode::UNAUTHORIZED, "Pair this device first."));
    };
    if !caller.grant.app {
        return secure(text(
            StatusCode::FORBIDDEN,
            "This device may not access the port proxy.",
        ));
    }
    let allowed = deps.ports.as_ref().is_some_and(|p| p.is_allowed(port));
    if !allowed {
        return secure(text(
            StatusCode::FORBIDDEN,
            "Port is not reported by Terax.",
        ));
    }

    let path_and_query = match request.uri().query() {
        Some(q) => format!("{inner_path}?{q}"),
        None => inner_path,
    };

    if is_websocket_upgrade(request.headers()) {
        return handle_ws_upgrade(port, &path_and_query, &mut request).await;
    }

    let stream = match TcpStream::connect(("127.0.0.1", port)).await {
        Ok(s) => s,
        Err(_) => {
            return secure(text(
                StatusCode::BAD_GATEWAY,
                "Could not connect to local server.",
            ));
        }
    };
    let (mut sender, conn) = match hyper::client::conn::http1::handshake(TokioIo::new(stream)).await
    {
        Ok(pair) => pair,
        Err(_) => {
            return secure(text(
                StatusCode::BAD_GATEWAY,
                "Upstream handshake failed.",
            ));
        }
    };
    tokio::spawn(async move {
        let _ = conn.await;
    });

    let mut out_req = Request::builder()
        .method(request.method().clone())
        .uri(&path_and_query);
    for (k, v) in request.headers() {
        if k == HOST {
            continue;
        }
        out_req = out_req.header(k, v);
    }
    out_req = out_req.header(HOST, format!("127.0.0.1:{port}"));
    let out_req = match out_req.body(request.into_body()) {
        Ok(r) => r,
        Err(_) => return secure(text(StatusCode::BAD_REQUEST, "Invalid request.")),
    };

    let upstream = match sender.send_request(out_req).await {
        Ok(r) => r,
        Err(_) => {
            return secure(text(
                StatusCode::BAD_GATEWAY,
                "Upstream request failed.",
            ));
        }
    };

    let (mut parts, body) = upstream.into_parts();
    parts.headers.remove("x-frame-options");
    parts.headers.remove("content-security-policy");
    parts.headers.insert("X-Frame-Options", HeaderValue::from_static("SAMEORIGIN"));
    parts
        .headers
        .insert("Referrer-Policy", HeaderValue::from_static("no-referrer"));
    parts.headers.insert(
        "Cross-Origin-Resource-Policy",
        HeaderValue::from_static("same-origin"),
    );
    let boxed: Body = body
        .map_err(|_| -> std::convert::Infallible { unreachable!() })
        .boxed();
    Response::from_parts(parts, boxed)
}

async fn handle_ws_upgrade(
    port: u16,
    path_and_query: &str,
    request: &mut Request<Incoming>,
) -> Response<Body> {
    let mut upstream = match TcpStream::connect(("127.0.0.1", port)).await {
        Ok(s) => s,
        Err(_) => {
            return secure(text(
                StatusCode::BAD_GATEWAY,
                "Could not connect to local WebSocket server.",
            ));
        }
    };

    let mut raw_req = format!(
        "{} {} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n",
        request.method(),
        path_and_query
    );
    for (k, v) in request.headers() {
        if k == HOST {
            continue;
        }
        if let Ok(val) = v.to_str() {
            raw_req.push_str(&format!("{}: {}\r\n", k.as_str(), val));
        }
    }
    raw_req.push_str("\r\n");
    if upstream.write_all(raw_req.as_bytes()).await.is_err() {
        return secure(text(StatusCode::BAD_GATEWAY, "WebSocket upgrade failed."));
    }

    let mut resp_buf = Vec::with_capacity(1024);
    let mut tmp = [0u8; 512];
    let head_end = loop {
        match upstream.read(&mut tmp).await {
            Ok(0) | Err(_) => {
                return secure(text(StatusCode::BAD_GATEWAY, "WebSocket handshake closed."));
            }
            Ok(n) => {
                resp_buf.extend_from_slice(&tmp[..n]);
                if let Some(pos) = resp_buf.windows(4).position(|w| w == b"\r\n\r\n") {
                    break pos;
                }
                if resp_buf.len() > 16 * 1024 {
                    return secure(text(StatusCode::BAD_GATEWAY, "WebSocket headers too large."));
                }
            }
        }
    };

    let head = String::from_utf8_lossy(&resp_buf[..head_end]);
    let mut lines = head.split("\r\n");
    let status_line = lines.next().unwrap_or("");
    if !status_line.contains(" 101 ") {
        return secure(text(StatusCode::BAD_GATEWAY, "Upstream rejected WebSocket."));
    }

    let mut builder = Response::builder().status(StatusCode::SWITCHING_PROTOCOLS);
    for line in lines {
        if let Some((k, v)) = line.split_once(':') {
            builder = builder.header(k.trim(), v.trim());
        }
    }
    let leftover = resp_buf[head_end + 4..].to_vec();
    let on_upgrade = hyper::upgrade::on(request);

    tokio::spawn(async move {
        if let Ok(upgraded) = on_upgrade.await {
            let mut client = TokioIo::new(upgraded);
            if !leftover.is_empty() && client.write_all(&leftover).await.is_err() {
                return;
            }
            let _ = tokio::io::copy_bidirectional(&mut client, &mut upstream).await;
        }
    });

    builder.body(full(Vec::new())).unwrap()
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicUsize;
    use std::sync::Arc;

    use tokio::sync::watch;

    use super::*;
    use crate::modules::web_remote::devices::{DeviceStore, Grant, Pairing};
    use crate::modules::web_remote::http_rules::HostPolicy;
    use crate::modules::web_remote::server::{accept_loop, Assets, TabEvents};
    use crate::modules::web_remote::stream::{PtySessions, StreamLimits};
    use crate::modules::web_remote::tabs::TabRegistry;

    struct StaticPorts(Vec<u16>);
    impl PortAllowlist for StaticPorts {
        fn is_allowed(&self, port: u16) -> bool {
            self.0.contains(&port)
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
    async fn proxy_enforces_app_grant_allowlist_and_forwards_http_and_ws() {
        // Start a tiny local upstream server that answers HTTP and 101 WebSocket.
        let upstream_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let upstream_port = upstream_listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = upstream_listener.accept().await else {
                    break;
                };
                tokio::spawn(async move {
                    let mut buf = [0u8; 2048];
                    let n = sock.read(&mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]);
                    if req.to_ascii_lowercase().contains("upgrade: websocket") {
                        let _ = sock
                            .write_all(
                                b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\r\n",
                            )
                            .await;
                        let mut echo = [0u8; 64];
                        if let Ok(m) = sock.read(&mut echo).await {
                            let _ = sock.write_all(&echo[..m]).await;
                        }
                    } else {
                        let _ = sock
                            .write_all(
                                b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 5\r\n\r\nhello",
                            )
                            .await;
                    }
                });
            }
        });

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let devices = Arc::new(DeviceStore::memory());
        let (info, token) = devices.add("browser");
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
            relay: None,
            schemes: None,
            ports: Some(Arc::new(StaticPorts(vec![upstream_port]))),
        };
        tokio::spawn(accept_loop(listener, deps, stop_rx));

        // 1. No app grant -> 403
        let mut s = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        s.write_all(
            format!("GET /proxy/{upstream_port}/ HTTP/1.1\r\nHost: localhost:{port}\r\nCookie: terax_remote={token}\r\nConnection: close\r\n\r\n").as_bytes(),
        )
        .await
        .unwrap();
        let mut raw = Vec::new();
        s.read_to_end(&mut raw).await.unwrap();
        assert!(String::from_utf8_lossy(&raw).starts_with("HTTP/1.1 403"));

        // Grant app
        assert!(devices.update(
            &info.id,
            Grant {
                input: false,
                tabs: false,
                app: true,
            }
        ));

        // 2. Unlisted port -> 403
        let mut s = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        let forbidden_port = upstream_port.wrapping_add(1).max(1025);
        s.write_all(
            format!("GET /proxy/{forbidden_port}/ HTTP/1.1\r\nHost: localhost:{port}\r\nCookie: terax_remote={token}\r\nConnection: close\r\n\r\n").as_bytes(),
        )
        .await
        .unwrap();
        let mut raw = Vec::new();
        s.read_to_end(&mut raw).await.unwrap();
        assert!(String::from_utf8_lossy(&raw).starts_with("HTTP/1.1 403"));

        // 3. Allowed HTTP -> 200 "hello"
        let mut s = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        s.write_all(
            format!("GET /proxy/{upstream_port}/index.html HTTP/1.1\r\nHost: localhost:{port}\r\nCookie: terax_remote={token}\r\nConnection: close\r\n\r\n").as_bytes(),
        )
        .await
        .unwrap();
        let mut raw = Vec::new();
        s.read_to_end(&mut raw).await.unwrap();
        let text = String::from_utf8_lossy(&raw);
        assert!(text.starts_with("HTTP/1.1 200"), "{text}");
        assert!(text.ends_with("hello"), "{text}");

        // 4. WebSocket upgrade -> 101 + bidirectional echo
        let mut s = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        s.write_all(
            format!("GET /proxy/{upstream_port}/ws HTTP/1.1\r\nHost: localhost:{port}\r\nCookie: terax_remote={token}\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n").as_bytes(),
        )
        .await
        .unwrap();
        let mut buf = [0u8; 512];
        let n = s.read(&mut buf).await.unwrap();
        assert!(String::from_utf8_lossy(&buf[..n]).starts_with("HTTP/1.1 101"));
        s.write_all(b"ping").await.unwrap();
        let m = s.read(&mut buf).await.unwrap();
        assert_eq!(&buf[..m], b"ping");
    }
}
