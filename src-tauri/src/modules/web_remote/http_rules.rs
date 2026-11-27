//! The request rules of the remote server, kept apart from the socket so
//! they can be tested: which Host may ask, which URL maps to a bundled file,
//! and what every response carries.

use std::net::IpAddr;

/// Where the web client lives in the bundle, and the only page served.
pub const WEB_INDEX: &str = "web-index.html";
pub const WEB_INDEX_PATH: &str = "/web-index.html";
/// Hashed build output; everything else is not served.
pub const ASSETS_PREFIX: &str = "/assets/";

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

/// The bundled file a URL names, or `None` for anything not part of the web
/// client. The result is an `asset_resolver` key, never a disk path, and it
/// can only be the page or a file under `assets/`.
pub fn asset_key(path: &str) -> Option<String> {
    if path == "/" || path == WEB_INDEX_PATH {
        return Some(WEB_INDEX.to_string());
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
