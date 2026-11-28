//! HTTP routes `/scheme/{terax-pty,asset,terax-doc}/<session>/<rest>`.
//!
//! Calls the same Rust scheme handlers with the session's relay label so
//! per-window PTY, asset and HTML-preview grants keep working unchanged.

use std::future::Future;
use std::pin::Pin;
use std::sync::Mutex;

use bytes::Bytes;
use http_body_util::{BodyExt, Full, Limited};
use hyper::header::{CACHE_CONTROL, CONTENT_TYPE, HeaderName, HeaderValue};
use hyper::{Method, Request, Response, StatusCode};
use tauri::Manager;
use tokio::sync::oneshot;

use super::super::http_rules::{is_cross_site, same_origin};
use super::super::server::{caller, secure, text, Body, Deps};
use crate::modules::fs::assets::{self, AssetGrants};
use crate::modules::fs::preview::{self, HtmlPreviews};
use crate::modules::pty::hub::Reply;
use crate::modules::pty::protocol::dispatch_for_label;

const MAX_SCHEME_BODY_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug)]
pub struct SchemeResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

pub trait SchemeHandler: Send + Sync {
    fn handle<'a>(
        &'a self,
        scheme: &'a str,
        label: &'a str,
        method: &'a str,
        path_and_query: &'a str,
        headers: Vec<(String, String)>,
        body: Vec<u8>,
    ) -> Pin<Box<dyn Future<Output = SchemeResponse> + Send + 'a>>;
}

pub struct AppSchemeHandler(pub tauri::AppHandle);

struct HttpReply(Mutex<Option<oneshot::Sender<(u16, Vec<u8>)>>>);

impl Reply for HttpReply {
    fn send(self: Box<Self>, status: u16, body: Vec<u8>) {
        if let Some(tx) = self.0.lock().unwrap().take() {
            let _ = tx.send((status, body));
        }
    }
}

impl SchemeHandler for AppSchemeHandler {
    fn handle<'a>(
        &'a self,
        scheme: &'a str,
        label: &'a str,
        method: &'a str,
        path_and_query: &'a str,
        headers: Vec<(String, String)>,
        body: Vec<u8>,
    ) -> Pin<Box<dyn Future<Output = SchemeResponse> + Send + 'a>> {
        Box::pin(async move {
            match scheme {
                "terax-pty" => {
                    let http_method =
                        tauri::http::Method::from_bytes(method.as_bytes()).unwrap_or(tauri::http::Method::POST);
                    let path = path_and_query.split('?').next().unwrap_or(path_and_query);
                    let (tx, rx) = oneshot::channel();
                    dispatch_for_label(
                        &self.0,
                        label,
                        &http_method,
                        path,
                        None,
                        &body,
                        Box::new(HttpReply(Mutex::new(Some(tx)))),
                    );
                    let (status, resp_body) = rx.await.unwrap_or((503, b"PTY stream unavailable".to_vec()));
                    let content_type = if status == 200 {
                        "application/octet-stream"
                    } else {
                        "text/plain; charset=utf-8"
                    };
                    SchemeResponse {
                        status,
                        headers: vec![
                            ("Content-Type".into(), content_type.into()),
                            ("Cache-Control".into(), "no-store".into()),
                            ("X-Content-Type-Options".into(), "nosniff".into()),
                        ],
                        body: resp_body,
                    }
                }
                "asset" | "download" => {
                    let Some(req) = build_tauri_request(method, path_and_query, &headers, body) else {
                        return plain_scheme(400, "invalid request");
                    };
                    let app = self.0.clone();
                    let label = label.to_string();
                    let resp = tauri::async_runtime::spawn_blocking(move || {
                        assets::respond(&app.state::<AssetGrants>(), &label, &req)
                            .unwrap_or_else(|_| {
                                tauri::http::Response::builder()
                                    .status(403)
                                    .header("Content-Type", "text/plain")
                                    .header("Cache-Control", "no-store")
                                    .header("X-Content-Type-Options", "nosniff")
                                    .body(Vec::new())
                                    .unwrap()
                            })
                    })
                    .await
                    .unwrap_or_else(|_| {
                        tauri::http::Response::builder()
                            .status(500)
                            .body(Vec::new())
                            .unwrap()
                    });
                    let mut out = from_tauri_response(resp);
                    if scheme == "download" && out.status == 200 {
                        let name = attachment_filename(path_and_query);
                        out.headers.push((
                            "Content-Disposition".into(),
                            format!("attachment; filename=\"{name}\""),
                        ));
                    }
                    out
                }
                "terax-doc" => {
                    let Some(req) = build_tauri_request(method, path_and_query, &headers, body) else {
                        return plain_scheme(400, "invalid request");
                    };
                    let resp = preview::respond_for_label(
                        &self.0.state::<HtmlPreviews>(),
                        label,
                        &req,
                    );
                    from_tauri_response(resp)
                }
                _ => plain_scheme(404, "unknown scheme"),
            }
        })
    }
}

fn plain_scheme(status: u16, msg: &str) -> SchemeResponse {
    SchemeResponse {
        status,
        headers: vec![
            ("Content-Type".into(), "text/plain; charset=utf-8".into()),
            ("Cache-Control".into(), "no-store".into()),
            ("X-Content-Type-Options".into(), "nosniff".into()),
        ],
        body: msg.as_bytes().to_vec(),
    }
}

fn build_tauri_request(
    method: &str,
    path_and_query: &str,
    headers: &[(String, String)],
    body: Vec<u8>,
) -> Option<tauri::http::Request<Vec<u8>>> {
    let mut builder = tauri::http::Request::builder()
        .method(method)
        .uri(path_and_query);
    for (k, v) in headers {
        builder = builder.header(k.as_str(), v.as_str());
    }
    builder.body(body).ok()
}

fn from_tauri_response(resp: tauri::http::Response<Vec<u8>>) -> SchemeResponse {
    let status = resp.status().as_u16();
    let headers = resp
        .headers()
        .iter()
        .filter_map(|(k, v)| {
            let name = k.as_str();
            if name.eq_ignore_ascii_case("access-control-allow-origin")
                || name.eq_ignore_ascii_case("vary")
            {
                return None;
            }
            v.to_str().ok().map(|val| (name.to_string(), val.to_string()))
        })
        .collect();
    SchemeResponse {
        status,
        headers,
        body: resp.into_body(),
    }
}

fn attachment_filename(path_and_query: &str) -> String {
    let raw_path = path_and_query.split('?').next().unwrap_or(path_and_query);
    let decoded = percent_encoding::percent_decode_str(raw_path).decode_utf8_lossy();
    let base = decoded
        .rsplit(['/', '\\'])
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or("download");
    base.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Parses `/scheme/<scheme>/<session>/<rest>` into `(scheme, session, "/<rest>")`.
pub(crate) fn parse_scheme_path(path: &str) -> Option<(&str, &str, String)> {
    let rest = path.strip_prefix("/scheme/")?;
    let (scheme, after_scheme) = rest.split_once('/')?;
    if !matches!(scheme, "terax-pty" | "asset" | "terax-doc" | "download") {
        return None;
    }
    let (session, tail) = match after_scheme.split_once('/') {
        Some((s, t)) => (s, format!("/{t}")),
        None => (after_scheme, "/".to_string()),
    };
    if session.is_empty() {
        return None;
    }
    Some((scheme, session, tail))
}

pub(crate) async fn handle_scheme(deps: &Deps, request: Request<hyper::body::Incoming>) -> Response<Body> {
    let path = request.uri().path().to_string();
    let Some((scheme, session_id, inner_path)) = parse_scheme_path(&path) else {
        return secure(text(StatusCode::NOT_FOUND, "Unknown scheme route."));
    };
    let method = request.method().clone();
    if method == Method::POST {
        if scheme != "terax-pty" {
            return secure(text(StatusCode::METHOD_NOT_ALLOWED, "Method not allowed."));
        }
        if is_cross_site(request.headers()) || !same_origin(request.headers()) {
            return secure(text(StatusCode::FORBIDDEN, "Origin is not allowed."));
        }
    } else if method != Method::GET && method != Method::HEAD {
        return secure(text(StatusCode::METHOD_NOT_ALLOWED, "Method not allowed."));
    }

    let Some(caller) = caller(deps, request.headers()) else {
        return secure(text(StatusCode::UNAUTHORIZED, "Pair this device first."));
    };
    if !caller.grant.app {
        return secure(text(
            StatusCode::FORBIDDEN,
            "This device may not access app schemes.",
        ));
    }
    let Some(relay) = &deps.relay else {
        return secure(text(
            StatusCode::SERVICE_UNAVAILABLE,
            "Relay sessions are unavailable.",
        ));
    };
    let Some(label) = relay.lookup(session_id, &caller.id) else {
        return secure(text(StatusCode::NOT_FOUND, "Session not found."));
    };
    let Some(schemes) = &deps.schemes else {
        return secure(text(
            StatusCode::SERVICE_UNAVAILABLE,
            "Scheme handler is unavailable.",
        ));
    };

    let path_and_query = match request.uri().query() {
        Some(q) => format!("{inner_path}?{q}"),
        None => inner_path,
    };
    let fwd_headers: Vec<(String, String)> = request
        .headers()
        .iter()
        .filter_map(|(k, v)| {
            if k.as_str().eq_ignore_ascii_case("range") {
                v.to_str().ok().map(|s| (k.as_str().to_string(), s.to_string()))
            } else {
                None
            }
        })
        .collect();
    let body = if method == Method::POST {
        match Limited::new(request.into_body(), MAX_SCHEME_BODY_BYTES)
            .collect()
            .await
        {
            Ok(c) => c.to_bytes().to_vec(),
            Err(_) => {
                return secure(text(
                    StatusCode::PAYLOAD_TOO_LARGE,
                    "That request body is too large.",
                ));
            }
        }
    } else {
        Vec::new()
    };

    let res = schemes
        .handle(
            scheme,
            &label,
            method.as_str(),
            &path_and_query,
            fwd_headers,
            body,
        )
        .await;

    let status = StatusCode::from_u16(res.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let mut builder = Response::builder().status(status);
    let mut has_cache = false;
    let mut has_nosniff = false;
    for (k, v) in res.headers {
        if k.eq_ignore_ascii_case("cache-control") {
            has_cache = true;
        }
        if k.eq_ignore_ascii_case("x-content-type-options") {
            has_nosniff = true;
        }
        if let (Ok(name), Ok(val)) = (HeaderName::from_bytes(k.as_bytes()), HeaderValue::from_str(&v))
        {
            builder = builder.header(name, val);
        }
    }
    if !has_cache {
        builder = builder.header(CACHE_CONTROL, "no-store");
    }
    if !has_nosniff {
        builder = builder.header("x-content-type-options", "nosniff");
    }
    if builder.headers_ref().and_then(|h| h.get(CONTENT_TYPE)).is_none() {
        builder = builder.header(CONTENT_TYPE, "application/octet-stream");
    }
    let mut resp = builder.body(Full::new(Bytes::from(res.body)).boxed()).unwrap();
    if scheme == "terax-doc" {
        let h = resp.headers_mut();
        h.insert("X-Frame-Options", HeaderValue::from_static("SAMEORIGIN"));
        h.insert("Referrer-Policy", HeaderValue::from_static("no-referrer"));
        h.insert(
            "Cross-Origin-Resource-Policy",
            HeaderValue::from_static("same-origin"),
        );
        resp
    } else {
        secure(resp)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicUsize;
    use std::sync::{Arc, Mutex};

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::sync::watch;

    use super::super::sessions::{InvokeRequestBody, InvokeResult, RelayHost, SessionManager};
    use super::*;
    use crate::modules::web_remote::devices::{DeviceStore, Grant, Pairing};
    use crate::modules::web_remote::http_rules::HostPolicy;
    use crate::modules::web_remote::server::{accept_loop, Assets, TabEvents};
    use crate::modules::web_remote::stream::{PtySessions, StreamLimits};
    use crate::modules::web_remote::tabs::TabRegistry;

    struct MockScheme {
        seen: Mutex<Vec<(String, String, String, String)>>,
    }

    impl SchemeHandler for MockScheme {
        fn handle<'a>(
            &'a self,
            scheme: &'a str,
            label: &'a str,
            method: &'a str,
            path_and_query: &'a str,
            _headers: Vec<(String, String)>,
            _body: Vec<u8>,
        ) -> Pin<Box<dyn Future<Output = SchemeResponse> + Send + 'a>> {
            let s = scheme.to_string();
            let l = label.to_string();
            let m = method.to_string();
            let p = path_and_query.to_string();
            Box::pin(async move {
                self.seen.lock().unwrap().push((s, l, m, p));
                SchemeResponse {
                    status: 200,
                    headers: vec![("Content-Type".into(), "application/octet-stream".into())],
                    body: b"ok-bytes".to_vec(),
                }
            })
        }
    }

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
    async fn scheme_routes_require_app_grant_and_forward_session_label() {
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
        let mock = Arc::new(MockScheme {
            seen: Mutex::new(Vec::new()),
        });

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
            schemes: Some(mock.clone()),
            ports: None,
        };
        tokio::spawn(accept_loop(listener, deps, stop_rx));

        let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .unwrap();
        let req = format!(
            "GET /scheme/asset/{}/foo.png?token=abc HTTP/1.1\r\nHost: localhost:{port}\r\nCookie: terax_remote={token}\r\nConnection: close\r\n\r\n",
            sess.id
        );
        stream.write_all(req.as_bytes()).await.unwrap();
        let mut raw = Vec::new();
        stream.read_to_end(&mut raw).await.unwrap();
        let resp = String::from_utf8_lossy(&raw);
        assert!(resp.starts_with("HTTP/1.1 200"), "{resp}");
        assert!(
            resp.to_ascii_lowercase().contains("x-content-type-options: nosniff"),
            "{resp}"
        );
        assert!(resp.ends_with("ok-bytes"), "{resp}");

        let seen = mock.seen.lock().unwrap();
        assert_eq!(
            seen.as_slice(),
            &[(
                "asset".to_string(),
                sess.label,
                "GET".to_string(),
                "/foo.png?token=abc".to_string()
            )]
        );
    }

    #[test]
    fn download_route_parses_and_sanitizes_attachment_filename() {
        assert_eq!(
            parse_scheme_path("/scheme/download/sess1/path/to/my%20file.txt"),
            Some(("download", "sess1", "/path/to/my%20file.txt".to_string()))
        );
        assert_eq!(
            attachment_filename("/path/to/my%20file%22.txt?token=123"),
            "my_file_.txt"
        );
    }
}
