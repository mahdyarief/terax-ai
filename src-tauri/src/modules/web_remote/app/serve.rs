//! Serves `/app/` (the desktop `index.html` with `/app/remote-runtime.js`
//! injected first in `<head>`) and `/app/*` bundle files for devices with
//! the `app` grant.

use hyper::header::HeaderMap;
use hyper::{Response, StatusCode};

use super::super::http_rules::{self, WEB_INDEX};
use super::super::server::{asset_response, caller, secure, text, Body, Deps};

const RUNTIME_SCRIPT_TAG: &str = "<script src=\"/app/remote-runtime.js\"></script>";

pub(crate) fn inject_runtime(html: &[u8]) -> Vec<u8> {
    let text = String::from_utf8_lossy(html);
    if let Some(pos) = text.to_ascii_lowercase().find("<head>") {
        let insert_at = pos + "<head>".len();
        let mut out = String::with_capacity(text.len() + RUNTIME_SCRIPT_TAG.len() + 1);
        out.push_str(&text[..insert_at]);
        out.push_str(RUNTIME_SCRIPT_TAG);
        out.push_str(&text[insert_at..]);
        return out.into_bytes();
    }
    let mut out = String::with_capacity(RUNTIME_SCRIPT_TAG.len() + text.len());
    out.push_str(RUNTIME_SCRIPT_TAG);
    out.push_str(&text);
    out.into_bytes()
}

fn safe_app_key(rest: &str) -> Option<String> {
    if rest.is_empty() || rest.contains("..") || rest.contains('\\') || rest.starts_with('/') {
        return None;
    }
    Some(rest.to_string())
}

pub(crate) fn handle_app(
    deps: &Deps,
    headers: &HeaderMap,
    path: &str,
    head: bool,
) -> Response<Body> {
    let has_app = caller(deps, headers).is_some_and(|c| c.grant.app);
    if !has_app {
        if path == "/app" || path == "/app/" || path == "/app/index.html" {
            return match deps.assets.read(WEB_INDEX) {
                Some(bytes) => secure(asset_response(
                    StatusCode::OK,
                    "text/html; charset=utf-8",
                    "no-store",
                    bytes,
                    head,
                )),
                None => secure(text(StatusCode::FORBIDDEN, "App access is not granted.")),
            };
        }
        return secure(text(StatusCode::FORBIDDEN, "App access is not granted."));
    }

    if path == "/app" || path == "/app/" || path == "/app/index.html" {
        let Some(html) = deps.assets.read("index.html") else {
            return secure(text(StatusCode::NOT_FOUND, "Not found."));
        };
        let injected = inject_runtime(&html);
        return secure(asset_response(
            StatusCode::OK,
            "text/html; charset=utf-8",
            "no-store",
            injected,
            head,
        ));
    }

    let Some(rest) = path.strip_prefix("/app/") else {
        return secure(text(StatusCode::NOT_FOUND, "Not found."));
    };
    let Some(key) = safe_app_key(rest) else {
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

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicUsize;
    use std::sync::Arc;

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::sync::watch;

    use crate::modules::web_remote::devices::{DeviceStore, Grant, Pairing};
    use crate::modules::web_remote::http_rules::HostPolicy;
    use crate::modules::web_remote::server::{accept_loop, Assets, Deps, TabEvents};
    use crate::modules::web_remote::stream::{PtySessions, StreamLimits};
    use crate::modules::web_remote::tabs::TabRegistry;

    struct FakeAssets;
    impl Assets for FakeAssets {
        fn read(&self, key: &str) -> Option<Vec<u8>> {
            match key {
                "index.html" => Some(b"<!doctype html><html><head><title>Terax</title></head><body></body></html>".to_vec()),
                "web-index.html" => Some(b"<!doctype html><html><head><title>Pair</title></head></html>".to_vec()),
                "remote-runtime.js" => Some(b"window.__TERAX_REMOTE__=true;".to_vec()),
                _ => None,
            }
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

    async fn get(port: u16, path: &str, cookie: Option<&str>) -> (u16, String, String) {
        let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .unwrap();
        let mut req = format!("GET {path} HTTP/1.1\r\nHost: localhost:{port}\r\n");
        if let Some(c) = cookie {
            req.push_str(&format!("Cookie: terax_remote={c}\r\n"));
        }
        req.push_str("Connection: close\r\n\r\n");
        stream.write_all(req.as_bytes()).await.unwrap();
        let mut raw = Vec::new();
        stream.read_to_end(&mut raw).await.unwrap();
        let text = String::from_utf8_lossy(&raw).into_owned();
        let (head, body) = text.split_once("\r\n\r\n").unwrap();
        let status: u16 = head
            .lines()
            .next()
            .unwrap()
            .split_whitespace()
            .nth(1)
            .unwrap()
            .parse()
            .unwrap();
        (status, head.to_string(), body.to_string())
    }

    #[tokio::test]
    async fn app_route_requires_app_grant_and_injects_runtime_script() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let devices = Arc::new(DeviceStore::memory());
        let (info, token) = devices.add("browser");
        let (_stop_tx, stop_rx) = watch::channel(false);

        let deps = Deps {
            assets: Arc::new(FakeAssets),
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
            ports: None,
            audit: None,
            rate_limit: None,
        };
        tokio::spawn(accept_loop(listener, deps, stop_rx));

        // Unpaired / no app grant gets pairing page on /app/ and 403 on /app/remote-runtime.js.
        let (status, _, body) = get(port, "/app/", None).await;
        assert_eq!(status, 200);
        assert!(body.contains("<title>Pair</title>"), "{body}");
        let (status, _, _) = get(port, "/app/remote-runtime.js", Some(&token)).await;
        assert_eq!(status, 403);

        // Grant app -> gets index.html with injected script and CSP without tauri:// or ipc://.
        assert!(devices.update(
            &info.id,
            Grant {
                input: false,
                tabs: false,
                app: true,
            }
        ));
        let (status, head, body) = get(port, "/app/", Some(&token)).await;
        assert_eq!(status, 200);
        assert!(
            body.contains("<head><script src=\"/app/remote-runtime.js\"></script><title>Terax</title>"),
            "{body}"
        );
        let lower_head = head.to_ascii_lowercase();
        assert!(lower_head.contains("connect-src 'self'"), "{head}");
        assert!(lower_head.contains("worker-src 'self' blob:"), "{head}");
        assert!(lower_head.contains("wasm-unsafe-eval"), "{head}");
        assert!(!lower_head.contains("tauri://"), "{head}");
        assert!(!lower_head.contains("ipc://"), "{head}");

        let (status, _, body) = get(port, "/app/remote-runtime.js", Some(&token)).await;
        assert_eq!(status, 200);
        assert!(body.contains("__TERAX_REMOTE__"), "{body}");
    }
}
