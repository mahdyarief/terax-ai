//! The remote access HTTP server: one listener, the bundled web client, and
//! (from Task 5 on) the authenticated API.
//!
//! Deny by default. The listener binds where the user chose, every request
//! must name an allowed Host, and nothing outside the web client's own files
//! is served.

use std::convert::Infallible;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use base64::Engine;
use bytes::Bytes;
use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, Full, Limited};
use hyper::body::Incoming;
use hyper::header::{HeaderValue, CACHE_CONTROL, CONTENT_LENGTH, CONTENT_TYPE, HOST, SET_COOKIE};
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, watch};

use super::devices::{Caller, DeviceStore, Pairing};
use super::http_rules::{
    self, cookie, is_cross_site, remote_header_present, same_origin, HostPolicy, DEVICE_COOKIE,
};
use super::sse;
use super::stream::{self, PtySessions, StreamLimits};
use super::tabs::TabRegistry;

/// Where a device reaches the server.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RemoteBind {
    /// This computer only (127.0.0.1).
    Loopback,
    /// This machine's Tailscale address (`100.64.0.0/10`), already
    /// WireGuard-encrypted.
    Tailscale,
    /// Every interface. Plain HTTP on the local network needs its own opt-in.
    All,
}

impl RemoteBind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Loopback => "loopback",
            Self::Tailscale => "tailscale",
            Self::All => "all",
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteAccessInfo {
    pub running: bool,
    pub bind: Option<RemoteBind>,
    pub port: Option<u16>,
    /// Addresses a browser on another device can open.
    pub urls: Vec<String>,
    /// Live event streams, across every paired device.
    pub viewers: usize,
    pub https: bool,
    pub allow_plain_lan: bool,
}

impl RemoteAccessInfo {
    pub fn off() -> Self {
        Self {
            running: false,
            bind: None,
            port: None,
            urls: Vec::new(),
            viewers: 0,
            https: false,
            allow_plain_lan: false,
        }
    }
}

pub(crate) struct Running {
    pub bind: RemoteBind,
    pub port: u16,
    pub urls: Vec<String>,
    pub allow_plain_lan: bool,
    pub https: bool,
    pub viewers: Arc<AtomicUsize>,
    pub stop: watch::Sender<bool>,
    /// Bumped by `tailscale serve` setup; reserved for Task 17.
    pub _serve: Option<()>,
}

impl Running {
    pub fn info(&self) -> RemoteAccessInfo {
        RemoteAccessInfo {
            running: true,
            bind: Some(self.bind),
            port: Some(self.port),
            urls: self.urls.clone(),
            viewers: self.viewers.load(Ordering::Relaxed),
            https: self.https,
            allow_plain_lan: self.allow_plain_lan,
        }
    }
}

/// Reads the web client out of Terax's own bundle.
pub(crate) trait Assets: Send + Sync {
    /// `key` is an `asset_resolver` key ("web-index.html", "assets/x.js").
    fn read(&self, key: &str) -> Option<Vec<u8>>;
}

pub(crate) struct BundledAssets(pub tauri::AppHandle);

impl Assets for BundledAssets {
    fn read(&self, key: &str) -> Option<Vec<u8>> {
        self.0
            .asset_resolver()
            .get(key.to_string())
            .map(|asset| asset.bytes.clone())
    }
}

/// What one request needs. Everything is a plain handle so the response path
/// can be tested without a running Tauri app.
#[derive(Clone)]
pub(crate) struct Deps {
    pub assets: Arc<dyn Assets>,
    pub tabs: Arc<TabRegistry>,
    pub hosts: HostPolicy,
    pub viewers: Arc<AtomicUsize>,
    pub devices: Arc<DeviceStore>,
    pub pairing: Arc<Pairing>,
    pub sessions: Arc<dyn PtySessions>,
    pub limits: Arc<StreamLimits>,
    /// Delivers tab actions to the window that owns the tab or space.
    pub events: Arc<dyn TabEvents>,
    /// Whether browsers reach us over HTTPS (through `tailscale serve`), so
    /// the device cookie can be `Secure`.
    pub https: bool,
    pub relay: Option<Arc<super::app::sessions::SessionManager>>,
}

pub(crate) type Body = BoxBody<Bytes, Infallible>;

pub(crate) async fn accept_loop(
    listener: tokio::net::TcpListener,
    deps: Deps,
    mut stopped: watch::Receiver<bool>,
) {
    loop {
        let stream = tokio::select! {
            _ = stopped.changed() => break,
            accepted = listener.accept() => match accepted {
                Ok((stream, _)) => stream,
                Err(_) => continue,
            },
        };
        let deps = deps.clone();
        let mut stopped = stopped.clone();
        tauri::async_runtime::spawn(async move {
            let service = hyper::service::service_fn(move |request| {
                let deps = deps.clone();
                async move { Ok::<_, Infallible>(respond(&deps, request).await) }
            });
            let connection = hyper::server::conn::http1::Builder::new()
                .serve_connection(TokioIo::new(stream), service);
            tokio::select! {
                _ = connection => {}
                _ = stopped.changed() => {}
            }
        });
    }
}

pub(crate) async fn respond(deps: &Deps, request: Request<Incoming>) -> Response<Body> {
    let host = request
        .headers()
        .get(HOST)
        .and_then(|value| value.to_str().ok());
    if !deps.hosts.allows(host) {
        return secure(text(StatusCode::FORBIDDEN, "This host is not allowed."));
    }
    let path = request.uri().path().to_string();
    if request.method() == Method::POST {
        return post(deps, request, &path).await;
    }
    if request.method() != Method::GET && request.method() != Method::HEAD {
        return secure(text(
            StatusCode::METHOD_NOT_ALLOWED,
            "Only GET and HEAD are served here.",
        ));
    }
    if path == "/api/me" {
        return match caller(deps, request.headers()) {
            Some(caller) => secure(json(StatusCode::OK, &device_json(&caller))),
            None => secure(text(StatusCode::UNAUTHORIZED, "Pair this device first.")),
        };
    }
    if path == "/api/tabs" {
        return tabs_stream(deps, request.headers());
    }
    if let Some((id, "stream")) = pty_route(&path) {
        return pty_stream(deps, request.headers(), id);
    }
    serve_static(deps, &path, request.method() == Method::HEAD)
}

/// The whole registry, on connect and on every change. A heartbeat keeps the
/// connection open through idle proxies, and a revocation ends the stream at
/// once instead of waiting for the next request.
fn tabs_stream(deps: &Deps, headers: &hyper::header::HeaderMap) -> Response<Body> {
    let Some(caller) = caller(deps, headers) else {
        return secure(text(StatusCode::UNAUTHORIZED, "Pair this device first."));
    };
    let (tx, rx) = mpsc::channel(sse::STREAM_BUFFER);
    let tabs = deps.tabs.clone();
    let devices = deps.devices.clone();
    let guard = sse::ViewerGuard::open(&deps.viewers);
    tauri::async_runtime::spawn(async move {
        let _guard = guard;
        let mut updates = tabs.subscribe();
        let mut device_updates = devices.subscribe();
        let mut heartbeat = tokio::time::interval(sse::HEARTBEAT);
        heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let send_snapshot = |tx: &mpsc::Sender<sse::Item>| {
            let snapshot = tabs.snapshot();
            if let Ok(data) = serde_json::to_string(&*snapshot) {
                sse::send(tx, sse::event(None, "tabs", &data));
            }
        };
        send_snapshot(&tx);
        loop {
            tokio::select! {
                _ = tx.closed() => break,
                _ = heartbeat.tick() => sse::send(&tx, sse::comment("ping")),
                changed = updates.changed() => {
                    if changed.is_err() {
                        break;
                    }
                    send_snapshot(&tx);
                }
                _ = device_updates.changed() => {
                    if !devices.contains(&caller.id) {
                        break;
                    }
                }
            }
        }
    });
    secure(sse::response(rx))
}

/// Largest input a browser may take in one write. A paste larger than this
/// is split by the client.
pub(crate) const MAX_INPUT_BYTES: usize = 64 * 1024;

/// What a browser asked a window to do with its tabs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "action")]
pub enum TabAction {
    #[serde(rename = "new")]
    New {
        #[serde(rename = "spaceId")]
        space_id: String,
        /// `terminal` or `blocks`.
        mode: String,
    },
    #[serde(rename = "close")]
    Close {
        #[serde(rename = "tabId")]
        tab_id: u32,
    },
}

/// The windows' inbox. Implemented over Tauri events in the app and over a
/// recorder in tests.
pub(crate) trait TabEvents: Send + Sync {
    fn emit(&self, window: &str, action: &TabAction);
}

pub(crate) struct AppTabEvents(pub tauri::AppHandle);

impl TabEvents for AppTabEvents {
    fn emit(&self, window: &str, action: &TabAction) {
        use tauri::Emitter;
        if let Err(error) = self.0.emit_to(window, "remote:tab-action", action) {
            log::debug!("remote tab action to {window} failed: {error}");
        }
    }
}

/// `/api/pty/<id>/<action>`.
fn pty_route(path: &str) -> Option<(u32, &str)> {
    let rest = path.strip_prefix("/api/pty/")?;
    let (id, action) = rest.split_once('/')?;
    Some((id.parse().ok()?, action))
}

/// One terminal's output. Only a PTY some published tab lists can be
/// streamed, and only by a paired device.
fn pty_stream(deps: &Deps, headers: &hyper::header::HeaderMap, id: u32) -> Response<Body> {
    let Some(caller) = caller(deps, headers) else {
        return secure(text(StatusCode::UNAUTHORIZED, "Pair this device first."));
    };
    if !stream::listed(&deps.tabs, id) {
        return secure(text(StatusCode::NOT_FOUND, "No such terminal."));
    }
    let Some(terminal) = deps.sessions.terminal(id) else {
        return secure(text(StatusCode::NOT_FOUND, "No such terminal."));
    };
    let Some(slot) = deps.limits.acquire(&caller.id) else {
        return secure(text(
            StatusCode::TOO_MANY_REQUESTS,
            "Too many open terminals on this device.",
        ));
    };
    let resume = sse::last_event_id(headers);
    let (tx, rx) = mpsc::channel(sse::STREAM_BUFFER);
    let devices = deps.devices.clone();
    let device_id = caller.id.clone();
    let guard = sse::ViewerGuard::open(&deps.viewers);
    tauri::async_runtime::spawn(async move {
        let _guard = guard;
        stream::run(terminal, resume, tx, devices, device_id, slot).await;
    });
    secure(sse::response(rx))
}

/// Every POST is same-origin, marked by the web client, and not cross-site.
/// Pairing is not exempt: the page that pairs was served from here.
async fn post(deps: &Deps, request: Request<Incoming>, path: &str) -> Response<Body> {
    if is_cross_site(request.headers()) {
        return secure(text(
            StatusCode::FORBIDDEN,
            "Cross-site requests are refused.",
        ));
    }
    if !same_origin(request.headers()) {
        return secure(text(StatusCode::FORBIDDEN, "This origin is not allowed."));
    }
    if !remote_header_present(request.headers()) {
        return secure(text(
            StatusCode::FORBIDDEN,
            "Missing the web client's request header.",
        ));
    }
    match path {
        "/api/pair" => pair(deps, request).await,
        "/api/tabs" => tabs_action(deps, request).await,
        _ => match pty_route(path) {
            Some((id, "input")) => pty_input(deps, request, id).await,
            _ => secure(text(StatusCode::NOT_FOUND, "Not found.")),
        },
    }
}

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "lowercase")]
enum TabActionRequest {
    New {
        #[serde(rename = "spaceId")]
        space_id: String,
        #[serde(default)]
        mode: String,
    },
    Close {
        #[serde(rename = "tabId")]
        tab_id: u32,
    },
}

/// A new or closed tab is forward to the window that hosts it, which runs its
/// own new-tab or close-tab code; the registry then follows its next report.
async fn tabs_action(deps: &Deps, request: Request<Incoming>) -> Response<Body> {
    let Some(caller) = caller(deps, request.headers()) else {
        return secure(text(StatusCode::UNAUTHORIZED, "Pair this device first."));
    };
    if !caller.grant.tabs {
        return secure(text(
            StatusCode::FORBIDDEN,
            "This device may not change tabs.",
        ));
    }
    let body = match read_body(request, 4096).await {
        Ok(body) => body,
        Err(response) => return response,
    };
    let Ok(action) = serde_json::from_slice::<TabActionRequest>(&body) else {
        return secure(text(StatusCode::BAD_REQUEST, "Unknown tab action."));
    };
    let (window, event) = match action {
        TabActionRequest::New { space_id, mode } => {
            let Some(window) = deps.tabs.window_for_space(&space_id) else {
                return secure(text(StatusCode::NOT_FOUND, "No such workspace."));
            };
            let mode = if mode == "blocks" {
                "blocks"
            } else {
                "terminal"
            };
            (
                window,
                TabAction::New {
                    space_id,
                    mode: mode.to_string(),
                },
            )
        }
        TabActionRequest::Close { tab_id } => {
            let Some((window, _tab)) = deps.tabs.tab(tab_id) else {
                return secure(text(StatusCode::NOT_FOUND, "No such tab."));
            };
            (window, TabAction::Close { tab_id })
        }
    };
    deps.events.emit(&window, &event);
    secure(json(
        StatusCode::OK,
        &serde_json::json!({ "ok": true, "window": window }),
    ))
}

/// Raw bytes typed at a browser, written through the session's ordered
/// writer queue exactly as the desktop's own keystrokes are.
async fn pty_input(deps: &Deps, request: Request<Incoming>, id: u32) -> Response<Body> {
    let Some(caller) = caller(deps, request.headers()) else {
        return secure(text(StatusCode::UNAUTHORIZED, "Pair this device first."));
    };
    if !caller.grant.input {
        return secure(text(
            StatusCode::FORBIDDEN,
            "This device may watch but not type.",
        ));
    }
    if !stream::listed(&deps.tabs, id) {
        return secure(text(StatusCode::NOT_FOUND, "No such terminal."));
    }
    let Some(terminal) = deps.sessions.terminal(id) else {
        return secure(text(StatusCode::NOT_FOUND, "No such terminal."));
    };
    let bytes = match read_body(request, MAX_INPUT_BYTES).await {
        Ok(bytes) => bytes,
        Err(response) => return response,
    };
    match terminal.write_remote(bytes).await {
        Ok(()) => secure(json(StatusCode::OK, &serde_json::json!({ "ok": true }))),
        Err(error) => {
            log::debug!("remote input to pty {id} failed: {error}");
            secure(text(
                StatusCode::CONFLICT,
                "The terminal is not taking input.",
            ))
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PairRequest {
    code: String,
    device_name: String,
}

async fn pair(deps: &Deps, request: Request<Incoming>) -> Response<Body> {
    let body = match read_body(request, 4096).await {
        Ok(body) => body,
        Err(response) => return response,
    };
    let Ok(pairing) = serde_json::from_slice::<PairRequest>(&body) else {
        return secure(text(StatusCode::BAD_REQUEST, "Expected a code and a name."));
    };
    let name = pairing.device_name.trim();
    if name.is_empty() {
        return secure(text(StatusCode::BAD_REQUEST, "Give this device a name."));
    }
    if let Err(error) = deps.pairing.consume(&pairing.code) {
        return secure(text(StatusCode::FORBIDDEN, error.message()));
    }
    let (info, token) = deps.devices.add(name);
    let mut response = secure(json(StatusCode::OK, &info));
    let cookie = cookie_header(&token, deps.https);
    if let Ok(value) = HeaderValue::from_str(&cookie) {
        response.headers_mut().insert(SET_COOKIE, value);
    }
    log::info!("remote device paired: {}", info.name);
    response
}

fn cookie_header(token: &str, secure: bool) -> String {
    let secure = if secure { "; Secure" } else { "" };
    format!("{DEVICE_COOKIE}={token}; Path=/; HttpOnly; SameSite=Strict; Max-Age=31536000{secure}")
}

/// The device behind a request, from its cookie. A revoked device's cookie no
/// longer resolves, and its requests get 401.
fn caller(deps: &Deps, headers: &hyper::header::HeaderMap) -> Option<Caller> {
    let token = cookie(headers, DEVICE_COOKIE)?;
    deps.devices.authenticate(&token)
}

fn device_json(caller: &Caller) -> serde_json::Value {
    serde_json::json!({
        "id": caller.id,
        "name": caller.name,
        "grant": {
            "input": caller.grant.input,
            "tabs": caller.grant.tabs,
            "app": caller.grant.app,
        },
    })
}

async fn read_body(request: Request<Incoming>, limit: usize) -> Result<Vec<u8>, Response<Body>> {
    let collected = Limited::new(request.into_body(), limit)
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

fn serve_static(deps: &Deps, path: &str, head: bool) -> Response<Body> {
    let Some(key) = http_rules::asset_key(path) else {
        return secure(text(StatusCode::NOT_FOUND, "Not found."));
    };
    let Some(bytes) = deps.assets.read(&key) else {
        return secure(text(StatusCode::NOT_FOUND, "Not found."));
    };
    secure(asset_response(
        StatusCode::OK,
        http_rules::content_type(&key),
        if http_rules::is_immutable(path) {
            "public, max-age=31536000, immutable"
        } else {
            "no-store"
        },
        bytes,
        head,
    ))
}

pub(crate) fn asset_response(
    status: StatusCode,
    content_type: &'static str,
    cache: &'static str,
    bytes: Vec<u8>,
    head: bool,
) -> Response<Body> {
    let length = bytes.len();
    let mut response = Response::new(full(if head { Vec::new() } else { bytes }));
    *response.status_mut() = status;
    let headers = response.headers_mut();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static(content_type));
    headers.insert(CACHE_CONTROL, HeaderValue::from_static(cache));
    headers.insert(CONTENT_LENGTH, HeaderValue::from(length));
    response
}

pub(crate) fn text(status: StatusCode, message: &'static str) -> Response<Body> {
    asset_response(
        status,
        "text/plain; charset=utf-8",
        "no-store",
        message.as_bytes().to_vec(),
        false,
    )
}

pub(crate) fn json<T: Serialize>(status: StatusCode, value: &T) -> Response<Body> {
    let body = serde_json::to_vec(value).unwrap_or_else(|_| b"{}".to_vec());
    asset_response(
        status,
        "application/json; charset=utf-8",
        "no-store",
        body,
        false,
    )
}

/// Every response, including errors: no framing, no sniffing, no referrer,
/// and a CSP that allows no inline script.
pub(crate) fn secure(mut response: Response<Body>) -> Response<Body> {
    let headers = response.headers_mut();
    headers.insert(
        "Content-Security-Policy",
        HeaderValue::from_static(http_rules::CSP),
    );
    headers.insert("X-Frame-Options", HeaderValue::from_static("DENY"));
    headers.insert(
        "X-Content-Type-Options",
        HeaderValue::from_static("nosniff"),
    );
    headers.insert("Referrer-Policy", HeaderValue::from_static("no-referrer"));
    headers.insert(
        "Cross-Origin-Resource-Policy",
        HeaderValue::from_static("same-origin"),
    );
    response
}

pub(crate) fn full(bytes: Vec<u8>) -> Body {
    Full::new(Bytes::from(bytes)).boxed()
}

/// A base64 body, for replay snapshots and terminal output.
pub(crate) fn base64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// This machine's IPv4 on `100.64.0.0/10`, when Tailscale is up. Connecting a
/// UDP socket sends nothing.
pub fn tailscale_ipv4() -> Option<Ipv4Addr> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("100.100.100.100:9").ok()?;
    match socket.local_addr().ok()?.ip() {
        IpAddr::V4(ip) if is_tailscale(ip) => Some(ip),
        _ => None,
    }
}

pub fn is_tailscale(ip: Ipv4Addr) -> bool {
    let octets = ip.octets();
    octets[0] == 100 && (64..=127).contains(&octets[1])
}

/// The address this machine uses for the default route, when it is not
/// loopback or a Tailscale one.
fn lan_ipv4() -> Option<Ipv4Addr> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("192.0.2.1:9").ok()?;
    match socket.local_addr().ok()?.ip() {
        IpAddr::V4(ip) if !ip.is_loopback() && !ip.is_unspecified() && !is_tailscale(ip) => {
            Some(ip)
        }
        _ => None,
    }
}

pub(crate) struct Bound {
    pub listener: tokio::net::TcpListener,
    pub port: u16,
    pub hosts: HostPolicy,
    pub urls: Vec<String>,
}

/// Opens the listener where `bind` says, and works out the Host names and
/// URLs that go with it.
pub(crate) async fn bind(
    bind: RemoteBind,
    port: u16,
    allow_plain_lan: bool,
    extra_names: Vec<String>,
    https: bool,
) -> Result<Bound, String> {
    let (address, mut names) = match bind {
        RemoteBind::Loopback => (
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            vec!["localhost".to_string(), "127.0.0.1".into(), "[::1]".into()],
        ),
        RemoteBind::Tailscale => {
            let ip = tailscale_ipv4().ok_or_else(|| {
                "No Tailscale address on this machine (100.64.0.0/10). Is Tailscale running?"
                    .to_string()
            })?;
            (
                IpAddr::V4(ip),
                vec!["localhost".to_string(), ip.to_string(), "[::1]".into()],
            )
        }
        RemoteBind::All => {
            if !allow_plain_lan {
                return Err(
                    "Binding every interface over plain HTTP needs its own opt-in.".to_string(),
                );
            }
            (
                IpAddr::V4(Ipv4Addr::UNSPECIFIED),
                vec!["localhost".to_string(), "127.0.0.1".into(), "[::1]".into()],
            )
        }
    };
    names.extend(extra_names.iter().cloned());
    let listener = tokio::net::TcpListener::bind(SocketAddr::new(address, port))
        .await
        .map_err(|e| format!("Could not listen on {address}:{port}: {e}"))?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let scheme = if https { "https" } else { "http" };
    let mut urls = match bind {
        RemoteBind::Loopback | RemoteBind::All => {
            vec![format!("{scheme}://localhost:{port}/web-index.html")]
        }
        RemoteBind::Tailscale => {
            vec![format!("{scheme}://{address}:{port}/web-index.html")]
        }
    };
    if bind == RemoteBind::All {
        if let Some(ip) = lan_ipv4() {
            urls.push(format!("http://{ip}:{port}/web-index.html"));
        }
    }
    let hosts = HostPolicy {
        port,
        names,
        any_ip: bind == RemoteBind::All,
        allow_default_port: https,
    };
    Ok(Bound {
        listener,
        port,
        hosts,
        urls,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Files;

    impl Assets for Files {
        fn read(&self, key: &str) -> Option<Vec<u8>> {
            match key {
                "web-index.html" => Some(b"<html>web</html>".to_vec()),
                "assets/app.js" => Some(b"console.log(1)".to_vec()),
                _ => None,
            }
        }
    }

    struct NoSessions;

    impl PtySessions for NoSessions {
        fn terminal(&self, _id: u32) -> Option<Arc<dyn stream::Terminal>> {
            None
        }
    }

    fn deps(port: u16) -> Deps {
        Deps {
            assets: Arc::new(Files),
            tabs: Arc::new(TabRegistry::new()),
            hosts: HostPolicy::loopback(port),
            viewers: Arc::new(AtomicUsize::new(0)),
            devices: Arc::new(crate::modules::web_remote::devices::DeviceStore::memory()),
            pairing: Arc::new(crate::modules::web_remote::devices::Pairing::default()),
            sessions: Arc::new(NoSessions),
            limits: Arc::new(StreamLimits::default()),
            events: Arc::new(NoEvents),
            https: false,
            relay: None,
        }
    }

    fn raw_get(port: u16, host: &str, path: &str) -> String {
        use std::io::{Read, Write};
        let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(
            stream,
            "GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n"
        )
        .unwrap();
        let mut out = String::new();
        stream.read_to_string(&mut out).unwrap();
        out
    }

    fn serve_for_test() -> (u16, watch::Sender<bool>, Deps) {
        serve_with(Arc::new(NoSessions))
    }

    fn serve_with(sessions: Arc<dyn PtySessions>) -> (u16, watch::Sender<bool>, Deps) {
        serve_full(sessions, Arc::new(NoEvents))
    }

    fn serve_full(
        sessions: Arc<dyn PtySessions>,
        events: Arc<dyn TabEvents>,
    ) -> (u16, watch::Sender<bool>, Deps) {
        let listener =
            tauri::async_runtime::block_on(tokio::net::TcpListener::bind("127.0.0.1:0")).unwrap();
        let port = listener.local_addr().unwrap().port();
        let (stop, stopped) = watch::channel(false);
        let mut deps = deps(port);
        deps.sessions = sessions;
        deps.events = events;
        tauri::async_runtime::spawn(accept_loop(listener, deps.clone(), stopped));
        (port, stop, deps)
    }

    struct NoEvents;

    impl TabEvents for NoEvents {
        fn emit(&self, _window: &str, _action: &TabAction) {}
    }

    #[derive(Default)]
    struct RecordingEvents {
        sent: std::sync::Mutex<Vec<(String, TabAction)>>,
    }

    impl TabEvents for RecordingEvents {
        fn emit(&self, window: &str, action: &TabAction) {
            self.sent
                .lock()
                .unwrap()
                .push((window.to_string(), action.clone()));
        }
    }

    #[derive(Default)]
    struct RecordingTerminal {
        writes: std::sync::Mutex<Vec<u8>>,
    }

    impl stream::Terminal for RecordingTerminal {
        fn subscribe(&self) -> watch::Receiver<u64> {
            let (_tx, rx) = watch::channel(0u64);
            rx
        }

        fn snapshot(&self) -> crate::modules::pty::ReplaySnapshot {
            crate::modules::pty::ReplaySnapshot {
                events: Vec::new(),
                sequence: 0,
                cols: 80,
                rows: 24,
                truncated: false,
            }
        }

        fn replay_after(
            &self,
            _checkpoint: u64,
        ) -> Result<Vec<crate::modules::pty::ReplayEvent>, crate::modules::pty::ReplayError>
        {
            Ok(Vec::new())
        }

        fn exit_code(&self) -> Option<i32> {
            None
        }

        fn theme(&self) -> Option<crate::modules::pty::TerminalColors> {
            None
        }

        fn write_remote<'a>(
            &'a self,
            bytes: Vec<u8>,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send + 'a>>
        {
            self.writes.lock().unwrap().extend_from_slice(&bytes);
            Box::pin(async { Ok(()) })
        }
    }

    struct OneTerminal(Arc<RecordingTerminal>);

    impl PtySessions for OneTerminal {
        fn terminal(&self, _id: u32) -> Option<Arc<dyn stream::Terminal>> {
            Some(self.0.clone())
        }
    }

    fn publish_pty(deps: &Deps, pty_id: u32) {
        deps.tabs.publish(
            "main",
            vec![crate::modules::web_remote::tabs::RemoteTab {
                id: 1,
                title: "shell".into(),
                kind: "terminal".into(),
                space_id: Some("default".into()),
                workspace: None,
                mode: Some("terminal".into()),
                panes: vec![crate::modules::web_remote::tabs::RemotePane {
                    leaf_id: 1,
                    pty_id: Some(pty_id),
                    active: true,
                }],
                agent: None,
            }],
        );
    }

    fn raw_request(port: u16, request: &str) -> String {
        use std::io::{Read, Write};
        let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream.write_all(request.as_bytes()).unwrap();
        let mut out = String::new();
        stream.read_to_string(&mut out).unwrap();
        out
    }

    fn post(port: u16, host: &str, path: &str, extra: &[(&str, &str)], body: &str) -> String {
        let origin = format!("http://{host}");
        let mut headers = vec![
            ("Host", host.to_string()),
            ("Origin", origin),
            ("x-terax-remote", "1".to_string()),
            ("Content-Type", "application/json".to_string()),
            ("Content-Length", body.len().to_string()),
            ("Connection", "close".to_string()),
        ];
        for (name, value) in extra {
            headers.retain(|(existing, _)| !existing.eq_ignore_ascii_case(name));
            // An empty value means "leave this header out".
            if !value.is_empty() {
                headers.push((name, value.to_string()));
            }
        }
        let headers: String = headers
            .into_iter()
            .map(|(name, value)| format!("{name}: {value}\r\n"))
            .collect();
        raw_request(
            port,
            &format!("POST {path} HTTP/1.1\r\n{headers}\r\n{body}"),
        )
    }

    fn set_cookie(response: &str) -> Option<String> {
        response.lines().find_map(|line| {
            if !line.to_ascii_lowercase().starts_with("set-cookie:") {
                return None;
            }
            Some(line[11..].trim().split(';').next()?.to_string())
        })
    }

    #[test]
    fn the_server_serves_the_page_and_assets_to_its_own_host_only() {
        let (port, stop, _deps) = serve_for_test();
        let page = raw_get(port, &format!("localhost:{port}"), "/web-index.html");
        assert!(page.starts_with("HTTP/1.1 200"), "{page}");
        let lower = page.to_ascii_lowercase();
        assert!(lower.contains("cache-control: no-store"));
        assert!(lower.contains("content-security-policy:"));
        assert!(lower.contains("x-frame-options: deny"));
        assert!(lower.contains("x-content-type-options: nosniff"));
        assert!(lower.contains("referrer-policy: no-referrer"));
        assert!(!lower.contains("access-control-allow-origin"));

        let asset = raw_get(port, &format!("localhost:{port}"), "/assets/app.js");
        assert!(asset.starts_with("HTTP/1.1 200"), "{asset}");
        assert!(asset
            .to_ascii_lowercase()
            .contains("cache-control: public, max-age=31536000, immutable"));

        let rebound = raw_get(port, &format!("evil.example:{port}"), "/web-index.html");
        assert!(rebound.starts_with("HTTP/1.1 403"), "{rebound}");
        for missing in [
            "/settings.html",
            "/assets/../web-index.html",
            "/assets/nope.js",
        ] {
            let out = raw_get(port, &format!("localhost:{port}"), missing);
            assert!(out.starts_with("HTTP/1.1 404"), "{missing}: {out}");
        }
        let _ = stop.send(true);
    }

    #[test]
    fn pairing_sets_a_cookie_and_revoking_the_device_ends_its_access() {
        let (port, stop, deps) = serve_for_test();
        let host = format!("localhost:{port}");
        let code = deps.pairing.start();
        let cookie = format!("{{\"code\":\"{code}\",\"deviceName\":\"phone\"}}");
        let response = post(port, &host, "/api/pair", &[], &cookie);
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
        let device_cookie = set_cookie(&response).expect("a device cookie");
        assert!(device_cookie.starts_with("terax_remote="));
        assert!(response.contains("HttpOnly") && response.contains("SameSite=Strict"));
        assert!(!response.contains("Secure"), "plain HTTP cannot set Secure");

        let me = raw_request(
            port,
            &format!(
                "GET /api/me HTTP/1.1\r\nHost: {host}\r\nCookie: {device_cookie}\r\nConnection: close\r\n\r\n"
            ),
        );
        assert!(me.starts_with("HTTP/1.1 200"), "{me}");
        assert!(me.contains("\"phone\""));

        // The same code cannot pair twice.
        let again = post(port, &host, "/api/pair", &[], &cookie);
        assert!(again.starts_with("HTTP/1.1 403"), "{again}");

        for device in deps.devices.list() {
            deps.devices.revoke(&device.id);
        }
        let revoked = raw_request(
            port,
            &format!(
                "GET /api/me HTTP/1.1\r\nHost: {host}\r\nCookie: {device_cookie}\r\nConnection: close\r\n\r\n"
            ),
        );
        assert!(revoked.starts_with("HTTP/1.1 401"), "{revoked}");
        let _ = stop.send(true);
    }

    #[test]
    fn a_post_without_the_client_header_or_from_another_site_is_refused() {
        let (port, stop, _deps) = serve_for_test();
        let host = format!("localhost:{port}");
        let body = "{\"code\":\"AAAAAA\",\"deviceName\":\"phone\"}";

        let no_header = post(port, &host, "/api/pair", &[("x-terax-remote", "")], body);
        assert!(
            no_header.starts_with("HTTP/1.1 403"),
            "missing header: {no_header}"
        );
        let cross = post(
            port,
            &host,
            "/api/pair",
            &[("Sec-Fetch-Site", "cross-site")],
            body,
        );
        assert!(cross.starts_with("HTTP/1.1 403"), "cross-site: {cross}");
        let wrong_origin = post(
            port,
            &host,
            "/api/pair",
            &[("Origin", "https://evil.example")],
            body,
        );
        assert!(
            wrong_origin.starts_with("HTTP/1.1 403"),
            "origin: {wrong_origin}"
        );
        let _ = stop.send(true);
    }

    #[test]
    fn a_tab_publish_reaches_a_connected_viewer() {
        let (port, stop, deps) = serve_for_test();
        let host = format!("localhost:{port}");
        let (_info, token) = deps.devices.add("phone");
        let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        {
            use std::io::Write;
            write!(
                stream,
                "GET /api/tabs HTTP/1.1\r\nHost: {host}\r\nCookie: terax_remote={token}\r\nAccept: text/event-stream\r\n\r\n"
            )
            .unwrap();
        }
        let first = read_until(&mut stream, "\"version\":0");
        assert!(first.starts_with("HTTP/1.1 200"), "{first}");
        assert!(first.contains("event: tabs"), "{first}");

        deps.tabs.publish(
            "main",
            vec![crate::modules::web_remote::tabs::RemoteTab {
                id: 1,
                title: "shell".into(),
                kind: "terminal".into(),
                space_id: Some("default".into()),
                workspace: None,
                mode: Some("terminal".into()),
                panes: Vec::new(),
                agent: None,
            }],
        );
        let second = read_until(&mut stream, "\"version\":1");
        assert!(second.contains("\"title\":\"shell\""), "{second}");
        let _ = stop.send(true);
    }

    fn read_until(stream: &mut std::net::TcpStream, needle: &str) -> String {
        use std::io::Read;
        let mut seen = String::new();
        let mut buffer = [0u8; 4096];
        while !seen.contains(needle) {
            match stream.read(&mut buffer) {
                Ok(0) => break,
                Ok(n) => seen.push_str(&String::from_utf8_lossy(&buffer[..n])),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(error) => panic!("stream read failed: {error}"),
            }
        }
        seen
    }

    #[test]
    fn an_unlisted_or_missing_terminal_is_not_found() {
        let (port, stop, deps) = serve_for_test();
        let host = format!("localhost:{port}");
        let (_info, token) = deps.devices.add("phone");
        let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        {
            use std::io::Write;
            write!(
                stream,
                "GET /api/pty/9/stream HTTP/1.1\r\nHost: {host}\r\nCookie: terax_remote={token}\r\n\r\n"
            )
            .unwrap();
        }
        let response = read_until(&mut stream, "\r\n\r\n");
        assert!(response.starts_with("HTTP/1.1 404"), "{response}");
        let _ = stop.send(true);
    }

    #[test]
    fn input_needs_the_grant_and_arrives_in_order() {
        let terminal = Arc::new(RecordingTerminal::default());
        let (port, stop, deps) = serve_with(Arc::new(OneTerminal(terminal.clone())));
        let host = format!("localhost:{port}");
        publish_pty(&deps, 7);
        let (info, token) = deps.devices.add("phone");

        let refused = post(
            port,
            &host,
            "/api/pty/7/input",
            &[("Cookie", &format!("terax_remote={token}"))],
            "abc",
        );
        assert!(refused.starts_with("HTTP/1.1 403"), "{refused}");
        assert!(terminal.writes.lock().unwrap().is_empty());

        assert!(deps.devices.update(
            &info.id,
            crate::modules::web_remote::devices::Grant {
                input: true,
                tabs: false,
                app: false,
            }
        ));
        for chunk in ["abc", "def"] {
            let response = post(
                port,
                &host,
                "/api/pty/7/input",
                &[("Cookie", &format!("terax_remote={token}"))],
                chunk,
            );
            assert!(response.starts_with("HTTP/1.1 200"), "{response}");
        }
        assert_eq!(&*terminal.writes.lock().unwrap(), b"abcdef");

        let unlisted = post(
            port,
            &host,
            "/api/pty/8/input",
            &[("Cookie", &format!("terax_remote={token}"))],
            "x",
        );
        assert!(unlisted.starts_with("HTTP/1.1 404"), "{unlisted}");
        let _ = stop.send(true);
    }

    #[test]
    fn tab_actions_are_routed_to_the_window_that_hosts_them() {
        let events = Arc::new(RecordingEvents::default());
        let (port, stop, deps) = serve_full(Arc::new(NoSessions), events.clone());
        let host = format!("localhost:{port}");
        publish_pty(&deps, 7);
        deps.tabs.publish(
            "space-b",
            vec![crate::modules::web_remote::tabs::RemoteTab {
                id: 2,
                title: "other".into(),
                kind: "terminal".into(),
                space_id: Some("space-b".into()),
                workspace: None,
                mode: Some("blocks".into()),
                panes: Vec::new(),
                agent: None,
            }],
        );
        let (info, token) = deps.devices.add("phone");
        let cookie = format!("terax_remote={token}");
        let new_tab = "{\"action\":\"new\",\"spaceId\":\"space-b\",\"mode\":\"blocks\"}";

        let denied = post(port, &host, "/api/tabs", &[("Cookie", &cookie)], new_tab);
        assert!(denied.starts_with("HTTP/1.1 403"), "{denied}");
        assert!(events.sent.lock().unwrap().is_empty());

        assert!(deps.devices.update(
            &info.id,
            crate::modules::web_remote::devices::Grant {
                input: false,
                tabs: true,
                app: false,
            }
        ));
        let created = post(port, &host, "/api/tabs", &[("Cookie", &cookie)], new_tab);
        assert!(created.starts_with("HTTP/1.1 200"), "{created}");
        let closed = post(
            port,
            &host,
            "/api/tabs",
            &[("Cookie", &cookie)],
            "{\"action\":\"close\",\"tabId\":1}",
        );
        assert!(closed.starts_with("HTTP/1.1 200"), "{closed}");

        let sent = events.sent.lock().unwrap().clone();
        assert_eq!(sent.len(), 2);
        assert_eq!(sent[0].0, "space-b");
        assert_eq!(
            sent[0].1,
            TabAction::New {
                space_id: "space-b".into(),
                mode: "blocks".into()
            }
        );
        assert_eq!(sent[1].0, "main");
        assert_eq!(sent[1].1, TabAction::Close { tab_id: 1 });

        let unknown = post(
            port,
            &host,
            "/api/tabs",
            &[("Cookie", &cookie)],
            "{\"action\":\"new\",\"spaceId\":\"nope\"}",
        );
        assert!(unknown.starts_with("HTTP/1.1 404"), "{unknown}");
        let _ = stop.send(true);
    }

    #[test]
    fn only_tailscale_addresses_count_as_the_tailnet() {
        assert!(is_tailscale(Ipv4Addr::new(100, 64, 0, 1)));
        assert!(is_tailscale(Ipv4Addr::new(100, 127, 255, 254)));
        assert!(!is_tailscale(Ipv4Addr::new(100, 63, 0, 1)));
        assert!(!is_tailscale(Ipv4Addr::new(100, 128, 0, 1)));
        assert!(!is_tailscale(Ipv4Addr::new(192, 168, 1, 4)));
    }

    #[test]
    fn plain_http_on_every_interface_needs_the_second_opt_in() {
        let runtime =
            tauri::async_runtime::block_on(bind(RemoteBind::All, 0, false, Vec::new(), false));
        assert!(runtime.is_err());
    }
}
