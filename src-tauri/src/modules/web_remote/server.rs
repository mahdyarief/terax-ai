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
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::header::{HeaderValue, CACHE_CONTROL, CONTENT_LENGTH, CONTENT_TYPE, HOST};
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use serde::{Deserialize, Serialize};
use tauri::Manager;
use tokio::sync::watch;

use super::http_rules::{self, HostPolicy};
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
    pub hosts: HostPolicy,
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
    match request.method() {
        &Method::GET | &Method::HEAD => serve_static(deps, &path, request.method() == Method::HEAD),
        _ => secure(text(
            StatusCode::METHOD_NOT_ALLOWED,
            "Only GET and HEAD are served here.",
        )),
    }
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

pub(crate) fn empty() -> Body {
    full(Vec::new())
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

    fn deps(port: u16) -> Deps {
        Deps {
            assets: Arc::new(Files),
            tabs: Arc::new(TabRegistry::new()),
            hosts: HostPolicy::loopback(port),
            viewers: Arc::new(AtomicUsize::new(0)),
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

    fn serve_for_test() -> (u16, watch::Sender<bool>) {
        let listener =
            tauri::async_runtime::block_on(tokio::net::TcpListener::bind("127.0.0.1:0")).unwrap();
        let port = listener.local_addr().unwrap().port();
        let (stop, stopped) = watch::channel(false);
        tauri::async_runtime::spawn(accept_loop(listener, deps(port), stopped));
        (port, stop)
    }

    #[test]
    fn the_server_serves_the_page_and_assets_to_its_own_host_only() {
        let (port, stop) = serve_for_test();
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
