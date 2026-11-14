//! Sandboxed HTML preview served over a custom URI scheme.
//!
//! Preview content is published from the renderer (the live editor buffer), not
//! read from disk by the scheme handler. Tokens are unguessable, scoped to the
//! publishing webview, and consumed on first successful load so a stale token
//! cannot be reused from another document or window.

use std::collections::HashMap;
use std::sync::Mutex;

use tauri::http::{Request, Response};
use tauri::{Manager, Runtime, UriSchemeContext, UriSchemeResponder};

const MAX_PREVIEW_BYTES: usize = 8 * 1024 * 1024;
const MAX_PREVIEWS: usize = 32;

/// No network, no parent access, no forms, no plugins. Inline scripts stay
/// allowed so a local report can draw its own charts, but the frame's opaque
/// origin plus `default-src 'none'` keeps them away from Tauri IPC, storage and
/// filesystem resources.
const CSP: &str = "default-src 'none'; script-src 'unsafe-inline' 'unsafe-eval'; style-src 'unsafe-inline'; img-src data: blob:; font-src data:; connect-src 'none'; form-action 'none'; base-uri 'none'; frame-src 'none'; object-src 'none'";

struct Preview {
    owner: String,
    sequence: u64,
    html: Vec<u8>,
}

#[derive(Default)]
pub struct HtmlPreviews {
    state: Mutex<PreviewState>,
}

#[derive(Default)]
struct PreviewState {
    entries: HashMap<String, Preview>,
    next_sequence: u64,
}

/// Publish one HTML document and return its single-use token. The renderer
/// turns the token into a scheme URL with `convertFileSrc`, which spells the
/// host differently per platform.
#[tauri::command]
pub async fn fs_publish_html(
    app: tauri::AppHandle,
    webview: tauri::Webview,
    html: String,
) -> Result<String, String> {
    let bytes = html.into_bytes();
    if bytes.len() > MAX_PREVIEW_BYTES {
        return Err(format!(
            "HTML preview is limited to {} MB.",
            MAX_PREVIEW_BYTES / 1024 / 1024
        ));
    }
    let owner = webview.label().to_owned();
    let token = random_token()?;
    let key = format!("{owner}:{token}");
    let state = app.state::<HtmlPreviews>();
    {
        let mut previews = state
            .state
            .lock()
            .map_err(|_| "HTML preview state is unavailable".to_string())?;
        previews.next_sequence += 1;
        let sequence = previews.next_sequence;
        previews
            .entries
            .insert(key, Preview { owner, sequence, html: bytes });
        if previews.entries.len() > MAX_PREVIEWS {
            // Drop the oldest publishes first; older frames have either loaded
            // (and removed themselves) or were superseded by a newer edit.
            let mut keys: Vec<(String, u64)> = previews
                .entries
                .iter()
                .map(|(key, preview)| (key.clone(), preview.sequence))
                .collect();
            keys.sort_unstable_by_key(|(_, sequence)| *sequence);
            for (key, _) in keys.into_iter().take(previews.entries.len() - MAX_PREVIEWS) {
                previews.entries.remove(&key);
            }
        }
    }
    Ok(token)
}

/// Release a preview that was published but never loaded, e.g. when the frame
/// unmounts while its edit was still being debounced.
#[tauri::command]
pub async fn fs_drop_html(
    app: tauri::AppHandle,
    webview: tauri::Webview,
    token: String,
) -> Result<(), String> {
    if !is_token(&token) {
        return Err("invalid preview token".to_string());
    }
    let token = token.to_ascii_lowercase();
    let key = format!("{}:{token}", webview.label());
    let state = app.state::<HtmlPreviews>();
    let mut previews = state
        .state
        .lock()
        .map_err(|_| "HTML preview state is unavailable".to_string())?;
    previews.entries.remove(&key);
    Ok(())
}

pub fn serve<R: Runtime>(
    context: UriSchemeContext<'_, R>,
    request: Request<Vec<u8>>,
    responder: UriSchemeResponder,
) {
    let app = context.app_handle().clone();
    let owner = context.webview_label().to_owned();
    responder.respond(respond(&app, &owner, &request));
}

fn respond<R: Runtime>(
    app: &tauri::AppHandle<R>,
    owner: &str,
    request: &Request<Vec<u8>>,
) -> Response<Vec<u8>> {
    let token = match parse_token(&request.uri().to_string()) {
        Some(token) => token,
        None => return plain(404),
    };
    let key = format!("{owner}:{token}");
    let state = app.state::<HtmlPreviews>();
    // A HEAD probe must not consume the single-use token, so answer it by
    // looking the entry up without removing it.
    if request.method() == "HEAD" {
        let found = match state.state.lock() {
            Ok(previews) => previews
                .entries
                .get(&key)
                .is_some_and(|preview| preview.owner == owner),
            Err(_) => return plain(500),
        };
        return plain(if found { 200 } else { 404 });
    }
    let html = match state.state.lock() {
        Ok(mut previews) => match previews.entries.remove(&key) {
            Some(preview) if preview.owner == owner => preview.html,
            _ => return plain(404),
        },
        Err(_) => return plain(500),
    };
    Response::builder()
        .status(200)
        .header("Content-Type", "text/html; charset=utf-8")
        .header("Content-Security-Policy", CSP)
        .header("X-Content-Type-Options", "nosniff")
        .header("Cache-Control", "no-store")
        .header("Referrer-Policy", "no-referrer")
        .body(html)
        .unwrap_or_else(|_| plain(500))
}

fn plain(status: u16) -> Response<Vec<u8>> {
    Response::builder()
        .status(status)
        .header("Content-Type", "text/plain; charset=utf-8")
        .header("Cache-Control", "no-store")
        .body(Vec::new())
        .expect("static response")
}

/// Extract the token from a preview request.
///
/// `convertFileSrc` yields `terax-doc://<token>/...` on macOS/Linux and
/// `http://terax-doc.localhost/<token>/...` on Windows; a bare request path is
/// accepted too because `request.uri()` may be relative.
fn parse_token(url: &str) -> Option<String> {
    url.split(|c: char| matches!(c, '/' | '?' | '#' | ':' | '.'))
        .find(|segment| is_token(segment))
        .map(|segment| segment.to_ascii_lowercase())
}

fn is_token(candidate: &str) -> bool {
    candidate.len() == 32 && candidate.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn random_token() -> Result<String, String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|error| format!("generate preview token: {error}"))?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_32_lowercase_hex_and_unique() {
        let first = random_token().unwrap();
        let second = random_token().unwrap();
        assert_eq!(first.len(), 32);
        assert!(first.bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert_eq!(first, first.to_ascii_lowercase());
        assert_ne!(first, second);
    }

    #[test]
    fn parse_token_accepts_only_our_urls() {
        let token = "0123456789abcdef0123456789abcdef";
        assert_eq!(
            parse_token(&format!("terax-doc://{token}/preview.html")),
            Some(token.to_string())
        );
        assert_eq!(
            parse_token(&format!("terax-doc://{}/preview.html", token.to_uppercase())),
            Some(token.to_string())
        );
        assert_eq!(parse_token(&format!("terax-doc://{token}")), Some(token.to_string()));
        assert_eq!(parse_token("asset://localhost/x"), None);
        assert_eq!(parse_token("terax-doc://short/preview.html"), None);
        assert_eq!(parse_token("terax-doc://../etc/passwd"), None);
    }

    #[test]
    fn csp_blocks_network_and_frames() {
        assert!(CSP.contains("default-src 'none'"));
        assert!(CSP.contains("connect-src 'none'"));
        assert!(CSP.contains("frame-src 'none'"));
        assert!(CSP.contains("object-src 'none'"));
    }
}
