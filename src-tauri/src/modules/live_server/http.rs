//! The request rules of the live server, kept apart from the socket so they
//! can be tested: which hosts may ask, which paths map to files, and what the
//! server sends back for them.

use std::net::IpAddr;

/// Where the reload script is served, and where it waits for a change. Both
/// sit under a name no site uses, and nothing under it is read from disk.
pub const RELOAD_SCRIPT_PATH: &str = "/__terax/live-reload.js";
pub const RELOAD_WAIT_PATH: &str = "/__terax/reload";

/// Whether a request's `Host` header names this server. Only `localhost` and
/// IP literals pass: a DNS name that resolves to 127.0.0.1 is how a website
/// in the user's browser would read this server (DNS rebinding), so names are
/// refused. On the local network any IP literal is fine, since the server is
/// then reachable by IP anyway; without it, only loopback.
pub fn host_allowed(host: Option<&str>, port: u16, lan: bool) -> bool {
    let Some(host) = host else { return false };
    let host = host.trim().to_ascii_lowercase();
    let Some((name, given_port)) = split_host_port(&host) else {
        return false;
    };
    if given_port != port {
        return false;
    }
    if name == "localhost" {
        return true;
    }
    match name
        .trim_start_matches('[')
        .trim_end_matches(']')
        .parse::<IpAddr>()
    {
        Ok(ip) => ip.is_loopback() || lan,
        Err(_) => false,
    }
}

fn split_host_port(host: &str) -> Option<(&str, u16)> {
    let colon = if host.starts_with('[') {
        let end = host.find(']')?;
        (host.as_bytes().get(end + 1) == Some(&b':')).then_some(end + 1)?
    } else {
        host.rfind(':')?
    };
    let port = host[colon + 1..].parse().ok()?;
    Some((&host[..colon], port))
}

/// The path segments of a request below the served folder, or `None` when
/// the path tries to leave it or names something hidden. Decoding happens per
/// segment, so `%2F` cannot smuggle in a separator.
pub fn request_segments(path: &str) -> Option<Vec<String>> {
    let rest = path.strip_prefix('/')?;
    let mut segments = Vec::new();
    for raw in rest.split('/') {
        let segment = percent_encoding::percent_decode_str(raw)
            .decode_utf8()
            .ok()?
            .into_owned();
        if segment.is_empty() || segment == "." {
            continue;
        }
        if segment == ".."
            || segment.contains(['/', '\\', '\0', ':'])
            // Dotfiles (.git, .env, .ssh, ...) are never part of a site.
            || (segment.starts_with('.') && segment != ".well-known")
        {
            return None;
        }
        segments.push(segment);
    }
    Some(segments)
}

/// Joins request segments onto the served folder in that folder's spelling.
pub fn join_root(root: &str, segments: &[String]) -> String {
    let sep = if root.contains('\\') { '\\' } else { '/' };
    let mut out = root.trim_end_matches(['/', '\\']).to_string();
    for segment in segments {
        out.push(sep);
        out.push_str(segment);
    }
    if segments.is_empty() && out.is_empty() {
        out.push(sep);
    }
    out
}

/// The content type for a file, by its extension.
pub fn content_type(name: &str) -> &'static str {
    let ext = name
        .rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" | "cjs" => "text/javascript; charset=utf-8",
        "json" | "map" => "application/json; charset=utf-8",
        "webmanifest" => "application/manifest+json; charset=utf-8",
        "txt" | "md" => "text/plain; charset=utf-8",
        "csv" => "text/csv; charset=utf-8",
        "xml" => "application/xml; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "bmp" => "image/bmp",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "wasm" => "application/wasm",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "ogg" => "audio/ogg",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "pdf" => "application/pdf",
        _ => "application/octet-stream",
    }
}

pub fn is_html(name: &str) -> bool {
    content_type(name).starts_with("text/html")
}

/// Adds the reload script to a page, before its last `</body>` or at the end.
pub fn inject_reload(html: &[u8]) -> Vec<u8> {
    let tag = format!("<script src=\"{RELOAD_SCRIPT_PATH}\"></script>");
    let lower = html.to_ascii_lowercase();
    let at = lower
        .windows(7)
        .rposition(|w| w == b"</body>")
        .unwrap_or(html.len());
    let mut out = Vec::with_capacity(html.len() + tag.len());
    out.extend_from_slice(&html[..at]);
    out.extend_from_slice(tag.as_bytes());
    out.extend_from_slice(&html[at..]);
    out
}

/// Waits for the next change and reloads the page. `seen` starts empty, so
/// the first answer only tells the page the current version.
pub const RELOAD_SCRIPT: &str = r#"(() => {
  let seen = "";
  const wait = (ms) => new Promise((r) => setTimeout(r, ms));
  const poll = async () => {
    try {
      const res = await fetch("/__terax/reload?v=" + encodeURIComponent(seen), { cache: "no-store" });
      const version = await res.text();
      if (seen && version !== seen) {
        location.reload();
        return;
      }
      seen = version;
    } catch {
      await wait(2000);
    }
    poll();
  };
  poll();
})();
"#;

/// A plain listing for a folder without an `index.html`, folders first.
pub fn listing(request_path: &str, mut entries: Vec<(String, bool)>) -> String {
    entries.retain(|(name, _)| !name.starts_with('.'));
    entries.sort_by(|a, b| {
        b.1.cmp(&a.1)
            .then_with(|| a.0.to_lowercase().cmp(&b.0.to_lowercase()))
    });
    let title = escape(request_path);
    let mut out = format!(
        "<!doctype html><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width\"><title>{title}</title>\
<style>body{{font:14px system-ui,sans-serif;margin:2rem;color-scheme:light dark}}a{{text-decoration:none}}li{{margin:.2rem 0}}</style>\
<h1 style=\"font-size:1rem\">Index of {title}</h1><ul>"
    );
    if request_path != "/" {
        out.push_str("<li><a href=\"../\">../</a></li>");
    }
    for (name, dir) in entries {
        let slash = if dir { "/" } else { "" };
        let href = percent_encoding::utf8_percent_encode(&name, HREF_SET).to_string();
        out.push_str(&format!(
            "<li><a href=\"{href}{slash}\">{}{slash}</a></li>",
            escape(&name)
        ));
    }
    out.push_str("</ul>");
    out
}

const HREF_SET: &percent_encoding::AsciiSet = &percent_encoding::NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~');

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_localhost_and_ip_literals_with_our_port_may_ask() {
        assert!(host_allowed(Some("localhost:5500"), 5500, false));
        assert!(host_allowed(Some("127.0.0.1:5500"), 5500, false));
        assert!(host_allowed(Some("[::1]:5500"), 5500, false));
        assert!(!host_allowed(Some("localhost:5501"), 5500, false));
        assert!(!host_allowed(Some("localhost"), 5500, false));
        assert!(!host_allowed(None, 5500, false));
        // DNS rebinding: a name that resolves to loopback is still refused.
        assert!(!host_allowed(Some("evil.example:5500"), 5500, false));
        assert!(!host_allowed(Some("evil.example:5500"), 5500, true));
        // A LAN address only when the server is shared on the network.
        assert!(!host_allowed(Some("192.168.1.4:5500"), 5500, false));
        assert!(host_allowed(Some("192.168.1.4:5500"), 5500, true));
    }

    #[test]
    fn request_paths_stay_inside_the_folder() {
        assert_eq!(request_segments("/"), Some(vec![]));
        assert_eq!(
            request_segments("/css/site%20main.css"),
            Some(vec!["css".into(), "site main.css".into()])
        );
        assert_eq!(
            request_segments("/a/./b"),
            Some(vec!["a".into(), "b".into()])
        );
        assert_eq!(request_segments("/../etc/passwd"), None);
        assert_eq!(request_segments("/a/%2e%2e/b"), None);
        assert_eq!(request_segments("/a%2Fb"), None);
        assert_eq!(request_segments("/a%5Cb"), None);
        assert_eq!(request_segments("/C:/Windows"), None);
        assert_eq!(request_segments("/.git/config"), None);
        assert_eq!(request_segments("/.env"), None);
        assert_eq!(request_segments("/%00"), None);
        assert_eq!(
            request_segments("/.well-known/x"),
            Some(vec![".well-known".into(), "x".into()])
        );
        assert_eq!(request_segments("relative"), None);
    }

    #[test]
    fn segments_join_in_the_folders_spelling() {
        let segs = vec!["css".to_string(), "a.css".to_string()];
        assert_eq!(join_root(r"D:\site\", &segs), r"D:\site\css\a.css");
        assert_eq!(join_root("/home/me/site", &segs), "/home/me/site/css/a.css");
        assert_eq!(join_root("/home/me/site/", &[]), "/home/me/site");
    }

    #[test]
    fn modules_wasm_and_fonts_get_their_types() {
        assert_eq!(content_type("app.MJS"), "text/javascript; charset=utf-8");
        assert_eq!(content_type("core.wasm"), "application/wasm");
        assert_eq!(content_type("f.woff2"), "font/woff2");
        assert_eq!(content_type("noext"), "application/octet-stream");
        assert!(is_html("index.HTM"));
    }

    #[test]
    fn the_reload_script_goes_before_the_last_body_end() {
        let out = String::from_utf8(inject_reload(b"<html><BODY>x</BODY></html>")).unwrap();
        assert_eq!(
            out,
            "<html><BODY>x<script src=\"/__terax/live-reload.js\"></script></BODY></html>"
        );
        let out = String::from_utf8(inject_reload(b"<p>x")).unwrap();
        assert!(out.ends_with("<script src=\"/__terax/live-reload.js\"></script>"));
    }

    #[test]
    fn a_listing_escapes_names_and_hides_dotfiles() {
        let html = listing(
            "/docs/",
            vec![
                ("b.html".into(), false),
                (".env".into(), false),
                ("<x>".into(), false),
                ("img".into(), true),
            ],
        );
        assert!(!html.contains(".env"));
        assert!(html.contains("&lt;x&gt;"));
        assert!(html.contains("href=\"%3Cx%3E\""));
        let img = html.find("img/").unwrap();
        assert!(img < html.find("b.html").unwrap());
        assert!(html.contains("href=\"../\""));
    }
}
