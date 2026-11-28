//! The request rules of the remote server, kept apart from the socket so
//! they can be tested: which Host may ask, which URL maps to a bundled file,
//! and what every response carries.

use std::net::IpAddr;

use hyper::header::{HeaderMap, ORIGIN};

/// Where the web client lives in the bundle, and the only page served.
pub const WEB_INDEX: &str = "web-index.html";
pub const WEB_INDEX_PATH: &str = "/web-index.html";
/// Hashed build output; everything else is not served.
pub const ASSETS_PREFIX: &str = "/assets/";
/// Files from Vite's `public/` that the app loads by an absolute path (the
/// block watermark's logo, the notification sound). Listed one by one so the
/// relay page, the settings page and the desktop index stay unreachable.
const PUBLIC_FILES: [&str; 2] = ["logo.png", "notification.mp3"];
/// Third-party notices, also from `public/`: one level, plain names only.
const LICENSES_PREFIX: &str = "/licenses/";

/// No inline script anywhere: the page and every bundle come from Terax's
/// own bundle, and the Ghostty core is WASM. `wasm-unsafe-eval` is what
/// instantiating an inline module needs in current browsers.
pub const CSP: &str = "default-src 'self'; \
script-src 'self' 'wasm-unsafe-eval'; \
style-src 'self' 'unsafe-inline'; \
img-src 'self' data: blob:; \
font-src 'self' data:; \
connect-src 'self'; \
worker-src 'self' blob:; \
media-src 'self' blob:; \
frame-ancestors 'none'; \
base-uri 'none'; \
form-action 'self'";

/// Which `Host` headers name this server. Only the names the user chose (this
/// machine, the bound address, a MagicDNS name) and IP literals pass: a
/// public DNS name that resolves here is how a website in the user's browser
/// would reach the server (DNS rebinding).
#[derive(Clone, Debug)]
pub struct HostPolicy {
    pub port: u16,
    /// Host names allowed beside IP literals, lowercase, without a port.
    pub names: Vec<String>,
    /// Any IP literal is fine (the server listens on every interface).
    pub any_ip: bool,
    /// Behind `tailscale serve`, the browser asks for `https://<name>/` with
    /// no port at all.
    pub allow_default_port: bool,
}

impl HostPolicy {
    pub fn loopback(port: u16) -> Self {
        Self {
            port,
            names: vec!["localhost".into(), "127.0.0.1".into(), "[::1]".into()],
            any_ip: false,
            allow_default_port: false,
        }
    }

    pub fn allows(&self, host: Option<&str>) -> bool {
        let Some(host) = host else { return false };
        let host = host.trim().to_ascii_lowercase();
        let Some((name, given_port)) = split_host_port(&host) else {
            return false;
        };
        if let Some(given) = given_port {
            if given != self.port {
                return false;
            }
        } else if !self.allow_default_port {
            return false;
        }
        if self.names.iter().any(|allowed| allowed == name) {
            return true;
        }
        match name
            .trim_start_matches('[')
            .trim_end_matches(']')
            .parse::<IpAddr>()
        {
            Ok(ip) => ip.is_loopback() || self.any_ip,
            Err(_) => false,
        }
    }
}

fn split_host_port(host: &str) -> Option<(&str, Option<u16>)> {
    let children = host.split_once(':');
    if host.starts_with('[') {
        let end = host.find(']')?;
        let rest = &host[end + 1..];
        let name = &host[..=end];
        if rest.is_empty() {
            return Some((name, None));
        }
        return Some((name, Some(rest.strip_prefix(':')?.parse().ok()?)));
    }
    match children {
        Some((name, port)) if !name.is_empty() && !port.contains(':') => {
            Some((name, Some(port.parse().ok()?)))
        }
        Some(_) => None,
        None => Some((host, None)),
    }
    .filter(|(name, _)| !name.is_empty())
}

/// The cookie a paired device carries. `HttpOnly` and `SameSite=Strict`, so
/// page script cannot read it and no other site can send it.
pub const DEVICE_COOKIE: &str = "terax_remote";
/// Every POST must carry this, so a plain HTML form cannot reach the API.
pub const REMOTE_HEADER: &str = "x-terax-remote";

pub fn cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    let header = headers.get(hyper::header::COOKIE)?.to_str().ok()?;
    header.split(';').find_map(|pair| {
        let (key, value) = pair.trim().split_once('=')?;
        (key == name).then(|| value.trim().to_string())
    })
}

/// A browser telling us another site made this request.
pub fn is_cross_site(headers: &HeaderMap) -> bool {
    headers
        .get("sec-fetch-site")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.eq_ignore_ascii_case("cross-site"))
}

/// The `Origin` names the same server the `Host` does. The scheme is not
/// compared: behind `tailscale serve` the browser speaks HTTPS while the
/// listener behind it speaks HTTP. A cross-site page cannot forge either.
pub fn same_origin(headers: &HeaderMap) -> bool {
    let Some(origin) = headers.get(ORIGIN).and_then(|value| value.to_str().ok()) else {
        return false;
    };
    let Some(host) = headers
        .get(hyper::header::HOST)
        .and_then(|value| value.to_str().ok())
    else {
        return false;
    };
    let origin = origin.trim().to_ascii_lowercase();
    let rest = origin
        .strip_prefix("https://")
        .or_else(|| origin.strip_prefix("http://"));
    rest.is_some_and(|rest| rest == host.trim().to_ascii_lowercase())
}

pub fn remote_header_present(headers: &HeaderMap) -> bool {
    headers.contains_key(REMOTE_HEADER)
}

/// The bundled file a URL names, or `None` for anything not part of the web
/// client. The result is an `asset_resolver` key, never a disk path, and it
/// can only be the page or a file under `assets/`.
pub fn asset_key(path: &str) -> Option<String> {
    if path == "/" || path == WEB_INDEX_PATH {
        return Some(WEB_INDEX.to_string());
    }
    if let Some(name) = path.strip_prefix('/').filter(|name| PUBLIC_FILES.contains(name)) {
        return Some(name.to_string());
    }
    if let Some(name) = path.strip_prefix(LICENSES_PREFIX) {
        let decoded = percent_encoding::percent_decode_str(name)
            .decode_utf8()
            .ok()?;
        if decoded.is_empty()
            || decoded.starts_with('.')
            || decoded.contains(['/', '\\', '\0', ':'])
        {
            return None;
        }
        return Some(format!("licenses/{decoded}"));
    }
    let rest = path.strip_prefix(ASSETS_PREFIX)?;
    if rest.is_empty() {
        return None;
    }
    let mut key = String::with_capacity(ASSETS_PREFIX.len() + rest.len());
    key.push_str("assets/");
    for segment in rest.split('/') {
        let decoded = percent_encoding::percent_decode_str(segment)
            .decode_utf8()
            .ok()?;
        if decoded.is_empty()
            || decoded.starts_with('.')
            || decoded.contains(['/', '\\', '\0', ':'])
        {
            return None;
        }
        key.push_str(&decoded);
        key.push('/');
    }
    key.pop();
    Some(key)
}

/// Hashed build output never changes; the HTML must never be stale.
pub fn is_immutable(path: &str) -> bool {
    path.starts_with(ASSETS_PREFIX)
}

pub fn content_type(key: &str) -> &'static str {
    match key.rsplit('.').next().unwrap_or_default() {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" | "map" => "application/json; charset=utf-8",
        "wasm" => "application/wasm",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "mp3" => "audio/mpeg",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_chosen_names_and_ip_literals_may_ask() {
        let loopback = HostPolicy::loopback(7600);
        assert!(loopback.allows(Some("localhost:7600")));
        assert!(loopback.allows(Some("127.0.0.1:7600")));
        assert!(loopback.allows(Some("[::1]:7600")));
        assert!(!loopback.allows(Some("localhost:7601")));
        assert!(!loopback.allows(Some("localhost")));
        assert!(!loopback.allows(Some("evil.example:7600")));
        assert!(!loopback.allows(None));

        let tailnet = HostPolicy {
            names: vec![
                "localhost".into(),
                "100.84.56.62".into(),
                "box.tail.ts.net".into(),
            ],
            ..loopback.clone()
        };
        assert!(tailnet.allows(Some("100.84.56.62:7600")));
        assert!(tailnet.allows(Some("box.tail.ts.net:7600")));
        assert!(!tailnet.allows(Some("100.84.56.63:7600")));
        assert!(tailnet.allows(Some("127.0.0.1:7600")));

        let lan = HostPolicy {
            any_ip: true,
            ..loopback.clone()
        };
        assert!(lan.allows(Some("192.168.1.4:7600")));
        assert!(!lan.allows(Some("evil.example:7600")));

        let tls = HostPolicy {
            allow_default_port: true,
            ..tailnet
        };
        assert!(tls.allows(Some("box.tail.ts.net")));
    }

    #[test]
    fn the_listed_public_files_are_served_and_nothing_else_at_the_root() {
        assert_eq!(asset_key("/logo.png"), Some("logo.png".into()));
        assert_eq!(asset_key("/notification.mp3"), Some("notification.mp3".into()));
        assert_eq!(
            asset_key("/licenses/terminal-symbols.txt"),
            Some("licenses/terminal-symbols.txt".into())
        );
        for bad in [
            "/remote-relay.html",
            "/settings.html",
            "/index.html",
            "/remote-runtime.js",
            "/licenses/",
            "/licenses/../web-index.html",
            "/licenses/%2e%2e/web-index.html",
            "/licenses/a/b.txt",
            "/licenses/.hidden",
            "/.logo.png",
            "/logo.png/x",
        ] {
            assert_eq!(asset_key(bad), None, "{bad}");
        }
        assert_eq!(content_type("logo.png"), "image/png");
        assert_eq!(content_type("notification.mp3"), "audio/mpeg");
        assert_eq!(content_type("licenses/x.txt"), "text/plain; charset=utf-8");
        assert!(!is_immutable("/logo.png"));
    }

    #[test]
    fn only_the_page_and_hashed_assets_map_to_files() {
        assert_eq!(asset_key("/"), Some("web-index.html".into()));
        assert_eq!(asset_key("/web-index.html"), Some("web-index.html".into()));
        assert_eq!(
            asset_key("/assets/index-a1b2.js"),
            Some("assets/index-a1b2.js".into())
        );
        assert_eq!(
            asset_key("/assets/fonts/mono.woff2"),
            Some("assets/fonts/mono.woff2".into())
        );
        for bad in [
            "/index.html",
            "/settings.html",
            "/assets/",
            "/assets/../web-index.html",
            "/assets/%2e%2e/x",
            "/assets/a%2Fb.js",
            "/assets/C:/x.js",
            "/assets/.env",
            "/../dist/web-index.html",
            "/api/tabs",
        ] {
            assert_eq!(asset_key(bad), None, "{bad}");
        }
    }

    #[test]
    fn cookies_are_found_by_name_and_post_checks_are_strict() {
        let mut headers = HeaderMap::new();
        headers.insert(
            hyper::header::COOKIE,
            "other=1; terax_remote=abc; third=2".parse().unwrap(),
        );
        assert_eq!(cookie(&headers, DEVICE_COOKIE).as_deref(), Some("abc"));
        assert_eq!(cookie(&headers, "missing"), None);

        headers.insert(hyper::header::HOST, "box.tail.ts.net:7600".parse().unwrap());
        headers.insert(ORIGIN, "https://box.tail.ts.net:7600".parse().unwrap());
        assert!(same_origin(&headers));
        headers.insert(ORIGIN, "http://box.tail.ts.net:7600".parse().unwrap());
        assert!(same_origin(&headers));
        headers.insert(ORIGIN, "https://evil.example".parse().unwrap());
        assert!(!same_origin(&headers));
        assert!(!same_origin(&HeaderMap::new()));

        headers.insert("sec-fetch-site", "cross-site".parse().unwrap());
        assert!(is_cross_site(&headers));
        headers.insert("sec-fetch-site", "same-origin".parse().unwrap());
        assert!(!is_cross_site(&headers));

        let mut post = HeaderMap::new();
        assert!(!remote_header_present(&post));
        post.insert(REMOTE_HEADER, "1".parse().unwrap());
        assert!(remote_header_present(&post));
    }

    #[test]
    fn html_is_never_cached_and_assets_are_immutable() {
        assert!(!is_immutable("/web-index.html"));
        assert!(is_immutable("/assets/x.js"));
        assert_eq!(
            content_type("assets/x.js"),
            "text/javascript; charset=utf-8"
        );
        assert_eq!(content_type("core.wasm"), "application/wasm");
        assert!(CSP.contains("frame-ancestors 'none'"));
        assert!(!CSP.contains("'unsafe-eval'"), "only wasm-unsafe-eval");
    }
}
