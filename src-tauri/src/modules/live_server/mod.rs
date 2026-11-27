//! A static file server for a folder, so a site can be opened at
//! `http://localhost:<port>` in the preview tab or any browser.
//!
//! What keeps it from serving more than the folder:
//! - It listens on 127.0.0.1 only, unless the user shares it on the local
//!   network for that server.
//! - Requests whose `Host` is not `localhost` or an IP literal are refused,
//!   so a website cannot reach it through DNS rebinding.
//! - Paths cannot leave the folder (`..`, encoded separators, drive names),
//!   dotfiles are never served, and every file is opened with
//!   `read_confined`, which checks where the opened file really is and
//!   applies the secret-path policy.
//! - It sends no CORS headers, so other origins cannot read its responses.
//!
//! Files are read through the workspace backend, so a folder on WSL or on an
//! SSH server is served the same way. HTML pages get a small script that
//! reloads them when a file in the folder changes.

mod http;

use std::collections::HashMap;
use std::convert::Infallible;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bytes::Bytes;
use http_body_util::Full;
use hyper::body::Incoming;
use hyper::header::{HeaderValue, CACHE_CONTROL, CONTENT_TYPE, HOST, LOCATION};
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use serde::Serialize;
use tauri::Manager;
use terax_workspace_core::confined::{Confine, ConfinedError, ConfinedRead};
use terax_workspace_core::workspace::EntryKind;
use tokio::sync::watch;

use crate::modules::workspace::{backend_for, resolve_path, same_spelling, within, WorkspaceEnv};

/// Largest file served; a site's assets are far smaller.
const MAX_FILE_BYTES: u64 = 256 * 1024 * 1024;
/// How long a page's reload request waits before it asks again.
const RELOAD_WAIT: Duration = Duration::from_secs(25);
/// Changes closer together than this reload the page once.
const RELOAD_DEBOUNCE: Duration = Duration::from_millis(150);

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveServerInfo {
    pub id: u32,
    /// The served folder, canonical, in its workspace's spelling.
    pub root: String,
    pub port: u16,
    /// `http://localhost:<port>/`.
    pub url: String,
    /// The addresses other devices use, when shared on the local network.
    pub lan_urls: Vec<String>,
    /// Whether changes made outside Terax reload the page too (a folder on
    /// this machine). Saves from the Terax editor always do.
    pub watching: bool,
}

struct Running {
    info: LiveServerInfo,
    workspace: WorkspaceEnv,
    lan: bool,
    stop: watch::Sender<bool>,
    version: Arc<watch::Sender<u64>>,
    _watcher: Option<notify::RecommendedWatcher>,
}

#[derive(Default)]
pub struct LiveServers {
    running: Mutex<HashMap<u32, Running>>,
    next_id: AtomicU32,
}

impl LiveServers {
    fn list(&self) -> Vec<LiveServerInfo> {
        let mut list: Vec<_> = self
            .running
            .lock()
            .unwrap()
            .values()
            .map(|r| r.info.clone())
            .collect();
        list.sort_by_key(|info| info.id);
        list
    }

    /// Reloads the pages of every server whose folder holds `path`.
    pub fn note_written(&self, path: &str, workspace: &WorkspaceEnv) {
        for running in self.running.lock().unwrap().values() {
            if &running.workspace == workspace && within(workspace, path, &running.info.root) {
                running.version.send_modify(|v| *v += 1);
            }
        }
    }
}

/// Context for one server's requests.
struct Site {
    root: String,
    workspace: WorkspaceEnv,
    port: u16,
    lan: bool,
    version: watch::Receiver<u64>,
}

/// Serves `path` (a folder, or the folder of a file) and returns the server.
/// A folder already served in the same way is returned as it is.
#[tauri::command]
pub async fn live_server_start(
    app: tauri::AppHandle,
    path: String,
    workspace: Option<WorkspaceEnv>,
    lan: Option<bool>,
) -> Result<LiveServerInfo, String> {
    let workspace = WorkspaceEnv::from_option(workspace);
    let lan = lan.unwrap_or(false);
    let root = {
        let workspace = workspace.clone();
        crate::modules::fs::blocking(move || served_folder(&path, &workspace)).await?
    };
    let state = app.state::<LiveServers>();
    let mut reuse_port = None;
    {
        let mut running = state.running.lock().unwrap();
        let same = running
            .iter()
            .find(|(_, r)| {
                r.workspace == workspace && same_spelling(&workspace, &r.info.root, &root)
            })
            .map(|(id, r)| (*id, r.lan));
        match same {
            Some((id, same_lan)) if same_lan == lan => return Ok(running[&id].info.clone()),
            // Sharing changes: start again on the same port when it is free.
            Some((id, _)) => {
                if let Some(old) = running.remove(&id) {
                    reuse_port = Some(old.info.port);
                    let _ = old.stop.send(true);
                }
            }
            None => {}
        }
    }

    let address = if lan {
        IpAddr::V4(Ipv4Addr::UNSPECIFIED)
    } else {
        IpAddr::V4(Ipv4Addr::LOCALHOST)
    };
    let reused = match reuse_port {
        Some(port) => tokio::net::TcpListener::bind(SocketAddr::new(address, port))
            .await
            .ok(),
        None => None,
    };
    let listener = match reused {
        Some(listener) => listener,
        None => tokio::net::TcpListener::bind(SocketAddr::new(address, 0))
            .await
            .map_err(|e| e.to_string())?,
    };
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();

    let (stop, stopped) = watch::channel(false);
    let (version, version_rx) = watch::channel(0u64);
    let version = Arc::new(version);
    let watcher = watch_folder(&root, &workspace, version.clone());
    let id = state.next_id.fetch_add(1, Ordering::Relaxed) + 1;
    let info = LiveServerInfo {
        id,
        root: root.clone(),
        port,
        url: format!("http://localhost:{port}/"),
        lan_urls: if lan { lan_urls(port) } else { Vec::new() },
        watching: watcher.is_some(),
    };
    let site = Arc::new(Site {
        root,
        workspace: workspace.clone(),
        port,
        lan,
        version: version_rx,
    });
    tauri::async_runtime::spawn(accept_loop(listener, site, stopped));
    state.running.lock().unwrap().insert(
        id,
        Running {
            info: info.clone(),
            workspace,
            lan,
            stop,
            version,
            _watcher: watcher,
        },
    );
    Ok(info)
}

#[tauri::command]
pub fn live_server_stop(state: tauri::State<'_, LiveServers>, id: u32) {
    if let Some(running) = state.running.lock().unwrap().remove(&id) {
        let _ = running.stop.send(true);
    }
}

#[tauri::command]
pub fn live_server_list(state: tauri::State<'_, LiveServers>) -> Vec<LiveServerInfo> {
    state.list()
}

/// The folder to serve: `path` itself, or the folder of a file. Never a
/// drive, the filesystem root or the home folder, which are not sites and
/// would put everything under them on a port.
pub(crate) fn served_folder(path: &str, workspace: &WorkspaceEnv) -> Result<String, String> {
    let ws = backend_for(workspace);
    let canonical = ws.canonicalize(path).map_err(|e| e.to_string())?;
    let meta = ws.metadata(&canonical).map_err(|e| e.to_string())?;
    let folder = match meta.kind {
        EntryKind::Dir => canonical,
        _ => parent_of(&canonical).ok_or("this file has no folder to serve")?,
    };
    crate::modules::fs::readable_in_workspace(&folder, workspace)?;
    let trimmed = folder.trim_end_matches(['/', '\\']);
    let is_root = trimmed.is_empty() || (trimmed.len() == 2 && trimmed.ends_with(':'));
    let is_home = !workspace.is_ssh()
        && dirs::home_dir().is_some_and(|home| resolve_path(&folder, workspace) == home);
    if is_root || is_home || (workspace.is_ssh() && is_remote_home(trimmed)) {
        return Err("Pick the site's own folder; a drive or home folder cannot be served.".into());
    }
    Ok(folder)
}

fn parent_of(path: &str) -> Option<String> {
    let cut = path.trim_end_matches(['/', '\\']).rfind(['/', '\\'])?;
    let parent = &path[..cut];
    Some(if parent.is_empty() || parent.ends_with(':') {
        format!("{parent}{}", &path[cut..=cut])
    } else {
        parent.to_string()
    })
}

/// `/root`, `/home/<user>` or `/Users/<user>` on a server.
fn is_remote_home(path: &str) -> bool {
    let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
    matches!(parts.as_slice(), ["root"] | ["home", _] | ["Users", _])
}

/// The addresses of this machine on the local network: the one the default
/// route uses. Connecting a UDP socket sends nothing.
fn lan_urls(port: u16) -> Vec<String> {
    std::net::UdpSocket::bind("0.0.0.0:0")
        .and_then(|socket| {
            socket.connect("192.0.2.1:9")?;
            socket.local_addr()
        })
        .ok()
        .map(|addr| addr.ip())
        .filter(|ip| !ip.is_loopback() && !ip.is_unspecified())
        .map(|ip| vec![format!("http://{ip}:{port}/")])
        .unwrap_or_default()
}

/// Bumps `version` when a file in a folder on this machine changes. A folder
/// on a server is not watched here; saves from the editor still reload it.
fn watch_folder(
    root: &str,
    workspace: &WorkspaceEnv,
    version: Arc<watch::Sender<u64>>,
) -> Option<notify::RecommendedWatcher> {
    use notify::Watcher;
    if workspace.is_ssh() {
        return None;
    }
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<()>();
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        let Ok(event) = event else { return };
        if matches!(event.kind, notify::EventKind::Access(_)) {
            return;
        }
        let ignored = |p: &std::path::PathBuf| {
            p.components().any(|c| {
                let c = c.as_os_str();
                c == ".git" || c == "node_modules"
            })
        };
        if !event.paths.is_empty() && event.paths.iter().all(ignored) {
            return;
        }
        let _ = tx.send(());
    })
    .ok()?;
    watcher
        .watch(
            &resolve_path(root, workspace),
            notify::RecursiveMode::Recursive,
        )
        .ok()?;
    tauri::async_runtime::spawn(async move {
        while rx.recv().await.is_some() {
            // Wait out the burst (a save, a build) before reloading.
            while let Ok(Some(())) = tokio::time::timeout(RELOAD_DEBOUNCE, rx.recv()).await {}
            version.send_modify(|v| *v += 1);
        }
    });
    Some(watcher)
}

async fn accept_loop(
    listener: tokio::net::TcpListener,
    site: Arc<Site>,
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
        let site = site.clone();
        let mut stopped = stopped.clone();
        tauri::async_runtime::spawn(async move {
            let service = hyper::service::service_fn(move |request| {
                let site = site.clone();
                async move { Ok::<_, Infallible>(respond(&site, request).await) }
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

async fn respond(site: &Site, request: Request<Incoming>) -> Response<Full<Bytes>> {
    if request.method() != Method::GET && request.method() != Method::HEAD {
        return plain(
            StatusCode::METHOD_NOT_ALLOWED,
            "Only GET and HEAD are served.",
        );
    }
    let host = request.headers().get(HOST).and_then(|h| h.to_str().ok());
    if !http::host_allowed(host, site.port, site.lan) {
        return plain(StatusCode::FORBIDDEN, "This host name is not allowed.");
    }
    let head = request.method() == Method::HEAD;
    let path = request.uri().path().to_string();
    if path == http::RELOAD_SCRIPT_PATH {
        return body(
            StatusCode::OK,
            "text/javascript; charset=utf-8",
            http::RELOAD_SCRIPT.into(),
            head,
        );
    }
    if path == http::RELOAD_WAIT_PATH {
        let seen = request
            .uri()
            .query()
            .and_then(|q| q.split('&').find_map(|kv| kv.strip_prefix("v=")))
            .and_then(|v| v.parse::<u64>().ok());
        let mut version = site.version.clone();
        if seen == Some(*version.borrow()) {
            let _ = tokio::time::timeout(RELOAD_WAIT, version.changed()).await;
        }
        let current = *version.borrow();
        return body(
            StatusCode::OK,
            "text/plain; charset=utf-8",
            current.to_string().into(),
            head,
        );
    }
    let Some(segments) = http::request_segments(&path) else {
        return plain(StatusCode::NOT_FOUND, "Not found.");
    };
    let root = site.root.clone();
    let workspace = site.workspace.clone();
    let served = tauri::async_runtime::spawn_blocking(move || {
        read_target(&root, &workspace, &path, &segments)
    })
    .await
    .unwrap_or(Err(Failure::Internal));
    match served {
        Ok(Served::File { name, bytes }) => {
            let bytes = if http::is_html(&name) {
                http::inject_reload(&bytes)
            } else {
                bytes
            };
            body(StatusCode::OK, http::content_type(&name), bytes, head)
        }
        Ok(Served::Listing(html)) => {
            let bytes = http::inject_reload(html.as_bytes());
            body(StatusCode::OK, "text/html; charset=utf-8", bytes, head)
        }
        Ok(Served::Redirect(to)) => {
            let mut response = plain(StatusCode::MOVED_PERMANENTLY, "");
            if let Ok(value) = HeaderValue::from_str(&to) {
                response.headers_mut().insert(LOCATION, value);
            }
            response
        }
        Err(Failure::NotFound) => plain(StatusCode::NOT_FOUND, "Not found."),
        Err(Failure::Refused) => plain(StatusCode::FORBIDDEN, "This file is not served."),
        Err(Failure::TooLarge) => plain(
            StatusCode::PAYLOAD_TOO_LARGE,
            "This file is too large to serve.",
        ),
        Err(Failure::Internal) => plain(
            StatusCode::INTERNAL_SERVER_ERROR,
            "The file could not be read.",
        ),
    }
}

pub(crate) enum Served {
    File { name: String, bytes: Vec<u8> },
    Listing(String),
    Redirect(String),
}

pub(crate) enum Failure {
    NotFound,
    Refused,
    TooLarge,
    Internal,
}

pub(crate) fn read_target(
    root: &str,
    workspace: &WorkspaceEnv,
    path: &str,
    segments: &[String],
) -> Result<Served, Failure> {
    let ws = backend_for(workspace);
    let target = http::join_root(root, segments);
    let meta = ws.metadata(&target).map_err(|_| Failure::NotFound)?;
    let file = if meta.kind == EntryKind::Dir {
        // Relative links on the page resolve against the folder only with
        // the slash; send the browser there first.
        if !path.ends_with('/') {
            return Ok(Served::Redirect(format!("{path}/")));
        }
        let canonical = ws.canonicalize(&target).map_err(|_| Failure::NotFound)?;
        if !within(workspace, &canonical, root) {
            return Err(Failure::Refused);
        }
        let index = http::join_root(&canonical, &["index.html".to_string()]);
        if ws.metadata(&index).is_ok_and(|m| m.kind == EntryKind::File) {
            index
        } else {
            crate::modules::fs::readable_in_workspace(&canonical, workspace)
                .map_err(|_| Failure::Refused)?;
            let entries = ws
                .read_dir(&canonical)
                .map_err(|_| Failure::Internal)?
                .into_iter()
                .map(|e| (e.name, e.kind == EntryKind::Dir))
                .collect();
            return Ok(Served::Listing(http::listing(path, entries)));
        }
    } else {
        target
    };
    if ws.metadata(&file).map_err(|_| Failure::NotFound)?.len > MAX_FILE_BYTES {
        return Err(Failure::TooLarge);
    }
    let read = ws
        .read_confined(
            &file,
            &ConfinedRead {
                roots: vec![Confine {
                    path: root.to_string(),
                    subtree: true,
                }],
                secret_policy: true,
                offset: 0,
                limit: MAX_FILE_BYTES,
            },
        )
        .map_err(|e| match e {
            ConfinedError::Io(e) if e.kind() == std::io::ErrorKind::NotFound => Failure::NotFound,
            ConfinedError::Io(_) => Failure::Internal,
            _ => Failure::Refused,
        })?;
    crate::modules::fs::readable_in_workspace(&read.real_path, workspace)
        .map_err(|_| Failure::Refused)?;
    let name = read
        .real_path
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or_default()
        .to_string();
    Ok(Served::File {
        name,
        bytes: read.bytes,
    })
}

fn body(
    status: StatusCode,
    content_type: &'static str,
    bytes: Vec<u8>,
    head: bool,
) -> Response<Full<Bytes>> {
    let length = bytes.len();
    let mut response = Response::new(Full::new(if head {
        Bytes::new()
    } else {
        Bytes::from(bytes)
    }));
    *response.status_mut() = status;
    let headers = response.headers_mut();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static(content_type));
    // Edits show on the next load, never a cached copy.
    headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(
        "X-Content-Type-Options",
        HeaderValue::from_static("nosniff"),
    );
    if head {
        headers.insert(hyper::header::CONTENT_LENGTH, HeaderValue::from(length));
    }
    response
}

fn plain(status: StatusCode, text: &'static str) -> Response<Full<Bytes>> {
    body(
        status,
        "text/plain; charset=utf-8",
        text.as_bytes().to_vec(),
        false,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_files_folder_is_its_parent_in_its_own_spelling() {
        assert_eq!(
            parent_of(r"D:\site\index.html").as_deref(),
            Some(r"D:\site")
        );
        assert_eq!(parent_of(r"D:\index.html").as_deref(), Some(r"D:\"));
        assert_eq!(
            parent_of("/home/me/site/index.html").as_deref(),
            Some("/home/me/site")
        );
        assert_eq!(parent_of("/index.html").as_deref(), Some("/"));
    }

    fn site() -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("index.html"), "<body>home</body>").unwrap();
        std::fs::create_dir(dir.path().join("docs")).unwrap();
        std::fs::write(dir.path().join("docs").join("guide.md"), "# guide").unwrap();
        std::fs::write(dir.path().join(".env"), "SECRET=1").unwrap();
        std::fs::write(dir.path().join("id_rsa"), "key").unwrap();
        let root = backend_for(&WorkspaceEnv::Local)
            .canonicalize(dir.path().to_str().unwrap())
            .unwrap();
        (dir, root)
    }

    fn get(root: &str, path: &str) -> Result<Served, Failure> {
        let segments = http::request_segments(path).ok_or(Failure::NotFound)?;
        read_target(root, &WorkspaceEnv::Local, path, &segments)
    }

    #[test]
    fn a_folder_serves_its_index_and_lists_a_folder_without_one() {
        let (_dir, root) = site();
        assert!(matches!(get(&root, "/"), Ok(Served::File { name, .. }) if name == "index.html"));
        assert!(matches!(get(&root, "/docs"), Ok(Served::Redirect(to)) if to == "/docs/"));
        match get(&root, "/docs/") {
            Ok(Served::Listing(html)) => assert!(html.contains("guide.md")),
            _ => panic!("expected a listing"),
        }
        assert!(matches!(get(&root, "/missing.js"), Err(Failure::NotFound)));
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

    #[test]
    fn the_server_answers_its_own_host_and_refuses_others() {
        let (_dir, root) = site();
        let listener =
            tauri::async_runtime::block_on(tokio::net::TcpListener::bind("127.0.0.1:0")).unwrap();
        let port = listener.local_addr().unwrap().port();
        let (stop, stopped) = watch::channel(false);
        let (_version, version) = watch::channel(0u64);
        let site = Arc::new(Site {
            root,
            workspace: WorkspaceEnv::Local,
            port,
            lan: false,
            version,
        });
        tauri::async_runtime::spawn(accept_loop(listener, site, stopped));

        let ok = raw_get(port, &format!("localhost:{port}"), "/");
        assert!(ok.starts_with("HTTP/1.1 200"), "{ok}");
        assert!(ok.to_ascii_lowercase().contains("cache-control: no-store"));
        assert!(!ok
            .to_ascii_lowercase()
            .contains("access-control-allow-origin"));
        assert!(ok.contains("home<script src=\"/__terax/live-reload.js\"></script></body>"));

        let rebound = raw_get(port, &format!("evil.example:{port}"), "/");
        assert!(rebound.starts_with("HTTP/1.1 403"), "{rebound}");
        let escaped = raw_get(port, &format!("localhost:{port}"), "/../x");
        assert!(escaped.starts_with("HTTP/1.1 404"), "{escaped}");
        let version = raw_get(port, &format!("localhost:{port}"), "/__terax/reload?v=");
        assert!(version.ends_with("\r\n\r\n0"), "{version}");
        let _ = stop.send(true);
    }

    #[test]
    fn secrets_are_never_served() {
        let (_dir, root) = site();
        assert!(get(&root, "/.env").is_err());
        assert!(matches!(get(&root, "/id_rsa"), Err(Failure::Refused)));
    }

    #[test]
    fn home_folders_on_a_server_are_not_sites() {
        assert!(is_remote_home("/root"));
        assert!(is_remote_home("/home/me"));
        assert!(is_remote_home("/Users/me"));
        assert!(!is_remote_home("/home/me/site"));
        assert!(!is_remote_home("/srv/www"));
    }
}
