use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::Serialize;
use terax_workspace_core::paths::to_canon;

// Short TTL keeps the auth-check TOCTOU window tight while still coalescing the
// burst of canonicalize calls within a single panel refresh (~100ms).
const CANONICAL_TTL: Duration = Duration::from_secs(1);
const CANONICAL_CACHE_CAP: usize = 256;

struct CanonicalEntry {
    canonical: PathBuf,
    inserted_at: Instant,
}

/// How far a chain of derived roots may nest (a worktree of a worktree's repo).
const MAX_DERIVED_DEPTH: usize = 4;

/// One authorized place: the workspace it lives in and its canonical path in
/// that workspace's spelling, `/`-separated (`C:/Users/x` for a local
/// workspace on Windows, `/home/x` inside a WSL distro or on a remote host).
/// Keying grants by workspace keeps a root in one workspace from ever
/// covering the same text in another.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Location {
    pub env: WorkspaceEnv,
    pub path: String,
}

impl Location {
    /// The location a host path names: a `\\wsl.localhost\<distro>\...` or
    /// `\\wsl$\<distro>\...` path (verbatim or not) is inside that distro,
    /// anything else is local. `None` when the path is not valid Unicode: its
    /// lossy spelling could equal another, real path's, so it names nothing.
    pub fn of_host(path: &Path) -> Option<Self> {
        path.to_str()?;
        Some(location_of_canon(&to_canon(path), cfg!(windows)))
    }

    /// The location a canonical workspace path names. Inside a WSL distro,
    /// `/mnt/<drive>/...` is the Windows drive itself (drvfs), which the host
    /// reaches as `X:\\...`, so it is local: the same place `of_host` finds.
    pub fn in_workspace(env: &WorkspaceEnv, canonical: &str) -> Self {
        location_in_workspace(env, canonical, cfg!(windows))
    }

    /// True when `other` is this location or lies beneath it.
    pub fn contains(&self, other: &Location) -> bool {
        same_env(&self.env, &other.env) && path_within(&other.path, &self.path)
    }
}

fn inexact() -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        "path cannot be represented exactly",
    )
}

/// Pure, with `windows` a parameter, so the WSL spellings are tested on every
/// host. `canon` is a `to_canon` string.
fn location_of_canon(canon: &str, windows: bool) -> Location {
    if windows {
        for host in ["//wsl.localhost/", "//wsl$/"] {
            let Some(head) = canon.get(..host.len()) else {
                continue;
            };
            if !head.eq_ignore_ascii_case(host) {
                continue;
            }
            let rest = &canon[host.len()..];
            let (distro, tail) = rest.split_once('/').unwrap_or((rest, ""));
            if !distro.is_empty() {
                return Location {
                    env: WorkspaceEnv::Wsl {
                        distro: distro.to_string(),
                    },
                    path: format!("/{tail}"),
                };
            }
        }
    }
    Location {
        env: WorkspaceEnv::Local,
        path: canon.to_string(),
    }
}

/// Pure, with `windows` a parameter, like `location_of_canon`.
fn location_in_workspace(env: &WorkspaceEnv, canonical: &str, windows: bool) -> Location {
    if windows && matches!(env, WorkspaceEnv::Wsl { .. }) {
        if let Some(path) = drvfs_drive_path(canonical) {
            return Location {
                env: WorkspaceEnv::Local,
                path,
            };
        }
    }
    Location {
        env: env.clone(),
        path: canonical.to_string(),
    }
}

/// `/mnt/c` -> `C:/`, `/mnt/c/x` -> `C:/x`; `None` for anything else.
fn drvfs_drive_path(path: &str) -> Option<String> {
    let rest = path.strip_prefix("/mnt/")?;
    let (drive, tail) = rest.split_once('/').unwrap_or((rest, ""));
    let mut letters = drive.chars();
    let letter = letters.next().filter(char::is_ascii_alphabetic)?;
    if letters.next().is_some() {
        return None;
    }
    Some(format!("{}:/{tail}", letter.to_ascii_uppercase()))
}

/// True when the canonical workspace path `path` is `root` or lies beneath it.
pub fn within(env: &WorkspaceEnv, path: &str, root: &str) -> bool {
    Location::in_workspace(env, root).contains(&Location::in_workspace(env, path))
}

/// Whether two workspace paths are spelled as the same entry. On this
/// machine (local, WSL) by their host spelling, as before; on a server, where
/// there is no host spelling (every remote path resolves to the same
/// unreachable one), by the path itself.
pub fn same_spelling(env: &WorkspaceEnv, a: &str, b: &str) -> bool {
    if env.is_ssh() {
        a.trim_end_matches('/') == b.trim_end_matches('/')
    } else {
        resolve_path(a, env) == resolve_path(b, env)
    }
}

/// Whether `path` is absolute in its workspace: on a server it starts at
/// `/`, or at a drive (`C:/`) when the server runs Windows.
pub fn is_absolute_in(env: &WorkspaceEnv, path: &str) -> bool {
    if env.is_ssh() {
        let bytes = path.as_bytes();
        path.starts_with('/')
            || (bytes.len() >= 3
                && bytes[0].is_ascii_alphabetic()
                && bytes[1] == b':'
                && matches!(bytes[2], b'/' | b'\\'))
    } else {
        resolve_path(path, env).is_absolute()
    }
}

/// WSL distro names are case-insensitive to `wsl.exe` and the UNC provider.
fn same_env(a: &WorkspaceEnv, b: &WorkspaceEnv) -> bool {
    match (a, b) {
        (WorkspaceEnv::Wsl { distro: a }, WorkspaceEnv::Wsl { distro: b }) => {
            a.eq_ignore_ascii_case(b)
        }
        _ => a == b,
    }
}

/// Component-wise prefix on `/`-separated paths: `/a/b` is within `/a`, but
/// `/a/bc` is not. A trailing separator on either side is ignored.
fn path_within(path: &str, root: &str) -> bool {
    let root = root.trim_end_matches('/');
    let path = path.trim_end_matches('/');
    path == root || path.strip_prefix(root).is_some_and(|rest| rest.starts_with('/'))
}

#[derive(Default)]
pub struct WorkspaceRegistry {
    roots: Mutex<HashSet<Location>>,
    /// Roots that exist only because another authorized root vouches for them
    /// (a linked worktree and its parent repository): child -> parent. They
    /// stop counting the moment they are revoked or the parent is no longer
    /// authorized.
    derived: Mutex<HashMap<Location, Location>>,
    canonical_cache: Mutex<HashMap<PathBuf, CanonicalEntry>>,
    /// The same short-lived cache for canonical workspace paths.
    workspace_cache: Mutex<HashMap<(WorkspaceEnv, String), (String, Instant)>>,
}

/// `path` canonicalized when it exists, otherwise its deepest existing ancestor
/// canonicalized with the missing tail re-attached: the form a grant on it was
/// stored under, even though the directory itself is gone.
fn canonical_or_resolved(path: &Path) -> PathBuf {
    let mut tail: Vec<&std::ffi::OsStr> = Vec::new();
    let mut cursor = path;
    loop {
        if let Ok(mut found) = std::fs::canonicalize(cursor) {
            for name in tail.iter().rev() {
                found.push(name);
            }
            return found;
        }
        match (cursor.file_name(), cursor.parent()) {
            (Some(name), Some(parent)) => {
                tail.push(name);
                cursor = parent;
            }
            _ => return path.to_path_buf(),
        }
    }
}

/// A comparison key for a location: on Windows the verbatim prefix is dropped,
/// both separators are one, and case does not matter (all three are properties
/// of the file system there); elsewhere only a trailing separator is ignored.
/// Pure, with `windows` a parameter, so it is tested on every host.
/// How a grant is matched when revoking: local paths on Windows compare like
/// that file system does; paths inside a distro are Linux paths.
fn location_key(location: &Location) -> String {
    let windows = cfg!(windows) && location.env == WorkspaceEnv::Local;
    normalize_location(&location.path, windows)
}

fn normalize_location(path: &str, windows: bool) -> String {
    if !windows {
        return path.trim_end_matches('/').to_string();
    }
    let stripped = if let Some(rest) = path.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = path.strip_prefix(r"\\?\") {
        rest.to_string()
    } else {
        path.to_string()
    };
    stripped
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_ascii_lowercase()
}

fn covered(
    target: &Location,
    roots: &HashSet<Location>,
    derived: &HashMap<Location, Location>,
    depth: usize,
) -> bool {
    if roots.iter().any(|root| root.contains(target)) {
        return true;
    }
    if depth >= MAX_DERIVED_DEPTH {
        return false;
    }
    derived
        .iter()
        .any(|(child, parent)| child.contains(target) && covered(parent, roots, derived, depth + 1))
}

impl WorkspaceRegistry {
    pub fn authorize<P: AsRef<Path>>(&self, path: P) -> std::io::Result<PathBuf> {
        let canonical = std::fs::canonicalize(path.as_ref())?;
        let location = Location::of_host(&canonical).ok_or_else(inexact)?;
        let mut set = self.roots.lock().expect("workspace registry poisoned");
        set.insert(location);
        Ok(canonical)
    }

    /// Authorizes `path` in `env` and returns its canonical workspace path.
    /// Local and WSL paths are resolved on this machine, as `authorize`
    /// does. A server's folder is resolved on the server (it must be a
    /// directory), and the host's agent is then confined to every root
    /// authorized on it, now and after each reconnect.
    pub fn authorize_in(&self, env: &WorkspaceEnv, path: &str) -> std::io::Result<String> {
        let WorkspaceEnv::Ssh { host } = env else {
            let canonical = self.authorize(resolve_path(path, env))?;
            return Ok(crate::modules::fs::to_canon(&canonical));
        };
        let registry = crate::modules::remote::registry::global()
            .ok_or_else(|| crate::modules::remote::registry::not_connected(host))?;
        let canonical = registry.canonical_dir(host, path)?;
        let roots = {
            let mut set = self.roots.lock().expect("workspace registry poisoned");
            set.insert(Location {
                env: env.clone(),
                path: canonical.clone(),
            });
            let mut roots: Vec<String> = set
                .iter()
                .filter(|root| root.env == *env)
                .map(|root| root.path.clone())
                .collect();
            roots.sort();
            roots
        };
        registry.set_roots(host, roots)?;
        Ok(canonical)
    }

    pub fn is_authorized(&self, target: &Path) -> bool {
        let set = self.roots.lock().expect("workspace registry poisoned");
        let derived = self.derived.lock().expect("workspace registry poisoned");
        Location::of_host(target).is_some_and(|target| covered(&target, &set, &derived, 0))
    }

    /// Authorizes `child` on the strength of `parent`, which must itself be
    /// authorized. Revoking the child (or losing the parent) removes the grant.
    ///
    /// A child that is already authorized, by a root or by a live grant, keeps
    /// what it has: replacing `task -> main` by `task -> task`, as listing the
    /// worktrees from inside the task itself would, leaves a grant whose parent
    /// covers nothing. The checks and the insertion share one critical section.
    pub fn authorize_derived(&self, child: &Path, parent: &Path) -> std::io::Result<PathBuf> {
        let child = std::fs::canonicalize(child)?;
        let parent = Location::of_host(&std::fs::canonicalize(parent)?).ok_or_else(inexact)?;
        let child_location = Location::of_host(&child).ok_or_else(inexact)?;
        let roots = self.roots.lock().expect("workspace registry poisoned");
        let mut derived = self.derived.lock().expect("workspace registry poisoned");
        if !covered(&parent, &roots, &derived, 0) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "parent is not an authorized root",
            ));
        }
        if !covered(&child_location, &roots, &derived, 0) {
            derived.insert(child_location, parent);
        }
        Ok(child)
    }

    /// Drops a derived grant. The path need not exist any more, and may be
    /// spelled differently from the canonical form the grant was stored under
    /// (git prints `C:/Users/x/wt`; the registry holds `\\?\C:\Users\x\wt`).
    pub fn revoke_derived(&self, child: &Path) -> bool {
        let wanted: Vec<(WorkspaceEnv, String)> =
            [Location::of_host(child), Location::of_host(&canonical_or_resolved(child))]
                .into_iter()
                .flatten()
                .map(|location| {
                    let key = location_key(&location);
                    (location.env, key)
                })
                .collect();
        let mut derived = self.derived.lock().expect("workspace registry poisoned");
        let before = derived.len();
        derived.retain(|key, _| {
            let spelled = location_key(key);
            !wanted
                .iter()
                .any(|(env, path)| same_env(env, &key.env) && *path == spelled)
        });
        derived.len() != before
    }

    /// Whether the canonical workspace path `canonical` in `env` lies inside
    /// an authorized root.
    pub fn covers(&self, env: &WorkspaceEnv, canonical: &str) -> bool {
        let set = self.roots.lock().expect("workspace registry poisoned");
        let derived = self.derived.lock().expect("workspace registry poisoned");
        covered(&Location::in_workspace(env, canonical), &set, &derived, 0)
    }

    /// `ws.canonicalize(path)`, coalesced over the same short window as
    /// `canonicalize_cached`.
    pub fn canonicalize_in(
        &self,
        ws: &dyn terax_workspace_core::workspace::Workspace,
        path: &str,
    ) -> std::io::Result<String> {
        let key = (ws.env().clone(), path.to_string());
        {
            let cache = self.workspace_cache.lock().expect("canonical cache poisoned");
            if let Some((canonical, at)) = cache.get(&key) {
                if at.elapsed() < CANONICAL_TTL {
                    return Ok(canonical.clone());
                }
            }
        }
        let canonical = ws.canonicalize(path)?;
        let mut cache = self.workspace_cache.lock().expect("canonical cache poisoned");
        if cache.len() >= CANONICAL_CACHE_CAP {
            cache.retain(|_, (_, at)| at.elapsed() < CANONICAL_TTL);
            if cache.len() >= CANONICAL_CACHE_CAP {
                cache.clear();
            }
        }
        cache.insert(key, (canonical.clone(), Instant::now()));
        Ok(canonical)
    }

    pub fn canonicalize_cached<P: AsRef<Path>>(&self, path: P) -> std::io::Result<PathBuf> {
        let key = path.as_ref().to_path_buf();
        {
            let cache = self
                .canonical_cache
                .lock()
                .expect("canonical cache poisoned");
            if let Some(entry) = cache.get(&key) {
                if entry.inserted_at.elapsed() < CANONICAL_TTL {
                    return Ok(entry.canonical.clone());
                }
            }
        }
        let canonical = std::fs::canonicalize(&key)?;
        let mut cache = self
            .canonical_cache
            .lock()
            .expect("canonical cache poisoned");
        if cache.len() >= CANONICAL_CACHE_CAP {
            cache.retain(|_, entry| entry.inserted_at.elapsed() < CANONICAL_TTL);
            if cache.len() >= CANONICAL_CACHE_CAP {
                cache.clear();
            }
        }
        cache.insert(
            key,
            CanonicalEntry {
                canonical: canonical.clone(),
                inserted_at: Instant::now(),
            },
        );
        Ok(canonical)
    }
}

// `None` means "use bootstrapped default". `Some` is canonicalized to defeat
// symlink/`..` traversal and must sit under an authorized root.
pub fn authorize_spawn_cwd(
    registry: &WorkspaceRegistry,
    cwd: Option<&str>,
    workspace: &WorkspaceEnv,
) -> Result<Option<PathBuf>, String> {
    // Even with no cwd: the caller would start its process on this machine.
    if workspace.is_ssh() {
        return Err(terax_workspace_core::env::not_on_this_machine().to_string());
    }
    let Some(cwd) = cwd.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    let resolved = resolve_path(cwd, workspace);
    let canonical =
        std::fs::canonicalize(&resolved).map_err(|e| format!("cwd not accessible: {e}"))?;
    if !canonical.is_dir() {
        return Err(format!("cwd is not a directory: {}", canonical.display()));
    }
    if !registry.is_authorized(&canonical) {
        return Err(format!(
            "cwd is outside the authorized workspace: {}",
            canonical.display()
        ));
    }
    Ok(Some(canonical))
}

// User-initiated terminal spawn: canonicalize, require a real dir, and register
// it as a root instead of rejecting paths outside existing roots.
pub fn authorize_user_spawn_cwd(
    registry: &WorkspaceRegistry,
    cwd: Option<&str>,
    workspace: &WorkspaceEnv,
) -> Result<Option<PathBuf>, String> {
    if workspace.is_ssh() {
        return Err(terax_workspace_core::env::not_on_this_machine().to_string());
    }
    let Some(cwd) = cwd.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    let resolved = resolve_path(cwd, workspace);
    let canonical =
        std::fs::canonicalize(&resolved).map_err(|e| format!("cwd not accessible: {e}"))?;
    if !canonical.is_dir() {
        return Err(format!("cwd is not a directory: {}", canonical.display()));
    }
    registry.authorize(&canonical).map_err(|e| e.to_string())?;
    Ok(Some(canonical))
}

// A saved cwd can be stale or from another environment (e.g. a Windows path in
// a now-WSL space); the terminal must still open, so fall back to home.
pub fn user_spawn_cwd_or_home(
    registry: &WorkspaceRegistry,
    cwd: Option<&str>,
    workspace: &WorkspaceEnv,
) -> Option<String> {
    let cwd = cwd.map(str::trim).filter(|s| !s.is_empty())?;
    match authorize_user_spawn_cwd(registry, Some(cwd), workspace) {
        Ok(_) => Some(cwd.to_owned()),
        Err(e) => {
            log::warn!("pty cwd {cwd:?} unusable in {workspace:?} ({e}); opening home");
            None
        }
    }
}

pub fn bootstrap_registry(registry: &WorkspaceRegistry) {
    let _ = registry.authorize(resolve_launch_dir());
    if let Some(home) = dirs::home_dir() {
        let _ = registry.authorize(home);
    }
}

#[tauri::command]
pub async fn workspace_authorize(
    app: tauri::AppHandle,
    path: String,
    workspace: Option<WorkspaceEnv>,
) -> Result<String, String> {
    // Canonicalizing touches the disk (possibly a network volume).
    crate::modules::fs::blocking(move || {
        let workspace = WorkspaceEnv::from_option(workspace);
        tauri::Manager::state::<WorkspaceRegistry>(&app)
            .authorize_in(&workspace, &path)
            .map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub async fn workspace_current_dir(app: tauri::AppHandle) -> Result<String, String> {
    crate::modules::fs::blocking(move || {
        let launch = resolve_launch_dir();
        let canonical = tauri::Manager::state::<WorkspaceRegistry>(&app)
            .authorize(&launch)
            .map_err(|e| e.to_string())?;
        Ok(crate::modules::fs::to_canon(&canonical))
    })
    .await
}

#[tauri::command]
pub async fn workspace_pick_folder(
    default_path: Option<String>,
) -> Result<Option<String>, String> {
    crate::modules::fs::blocking(move || {
        let mut dialog = rfd::FileDialog::new().set_title("Open Folder");
        if let Some(path) = default_path.filter(|p| !p.trim().is_empty()) {
            let p = PathBuf::from(path);
            if p.is_dir() {
                dialog = dialog.set_directory(&p);
            } else if let Some(parent) = p.parent().filter(|parent| parent.is_dir()) {
                dialog = dialog.set_directory(parent);
            }
        }
        let selected = dialog.pick_folder();
        Ok(selected.map(|p| crate::modules::fs::to_canon(&p)))
    })
    .await
}

// Snapshotted once at app startup so the live `current_dir()` drifting later
// (file dialogs, plugin chdir) can't shift the value seen by IPC or spawn.
static LAUNCH_CWD: OnceLock<Option<PathBuf>> = OnceLock::new();

pub fn init_launch_cwd(cli_dir: Option<&str>) {
    LAUNCH_CWD.get_or_init(|| resolve_launch_cwd(cli_dir, std::env::current_dir().ok()));
}

fn resolve_launch_cwd(cli_dir: Option<&str>, env_cwd: Option<PathBuf>) -> Option<PathBuf> {
    if let Some(dir) = cli_dir {
        let p = PathBuf::from(dir);
        if p.is_dir() {
            return Some(p);
        }
    }
    env_cwd.filter(|p| is_usable_launch_dir(p))
}

pub fn launch_cwd_snapshot() -> Option<PathBuf> {
    LAUNCH_CWD.get().and_then(|o| o.clone())
}

fn resolve_launch_dir() -> PathBuf {
    if let Some(cwd) = launch_cwd_snapshot() {
        return cwd;
    }
    if let Some(cwd) = std::env::current_dir()
        .ok()
        .filter(|p| is_usable_launch_dir(p))
    {
        return cwd;
    }
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"))
}

fn is_usable_launch_dir(path: &Path) -> bool {
    if !path.is_dir() || path == Path::new("/") {
        return false;
    }
    if is_executable_dir(path) {
        return false;
    }
    let s = path.to_string_lossy();
    if s.contains(".app/Contents/") {
        return false;
    }
    // The AppImage mount (/tmp/.mount_*) is not a real working directory.
    #[cfg(target_os = "linux")]
    if std::env::var_os("APPDIR").is_some_and(|appdir| path.starts_with(&appdir)) {
        return false;
    }
    if cfg!(debug_assertions) && path.file_name().and_then(|s| s.to_str()) == Some("src-tauri") {
        return false;
    }
    true
}

fn is_executable_dir(path: &Path) -> bool {
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    let Some(exe_dir) = exe.parent() else {
        return false;
    };
    match (std::fs::canonicalize(path), std::fs::canonicalize(exe_dir)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

#[cfg(target_os = "linux")]
const APPIMAGE_PATH_VARS: &[&str] = &[
    "LD_LIBRARY_PATH",
    "PATH",
    "XDG_DATA_DIRS",
    "GST_PLUGIN_SYSTEM_PATH",
    "GST_PLUGIN_SYSTEM_PATH_1_0",
    "GST_PLUGIN_PATH",
    "GI_TYPELIB_PATH",
    "GDK_PIXBUF_MODULEDIR",
    "GIO_MODULE_DIR",
    "GSETTINGS_SCHEMA_DIR",
];

#[cfg(target_os = "linux")]
const APPIMAGE_VALUE_VARS: &[&str] = &[
    "GDK_PIXBUF_MODULE_FILE",
    "LD_PRELOAD",
    "FONTCONFIG_FILE",
    "FONTCONFIG_PATH",
];

#[cfg(target_os = "linux")]
const APPIMAGE_MARKER_VARS: &[&str] = &["APPDIR", "APPIMAGE", "ARGV0"];

pub fn appimage_env_overrides() -> Vec<(&'static str, Option<OsString>)> {
    #[cfg(target_os = "linux")]
    {
        let Some(appdir) = std::env::var_os("APPDIR") else {
            return Vec::new();
        };
        compute_appimage_env_overrides(Path::new(&appdir), |k| std::env::var_os(k))
    }
    #[cfg(not(target_os = "linux"))]
    {
        Vec::new()
    }
}

#[cfg(target_os = "linux")]
fn compute_appimage_env_overrides(
    appdir: &Path,
    read: impl Fn(&str) -> Option<OsString>,
) -> Vec<(&'static str, Option<OsString>)> {
    let mut out = Vec::new();

    for &key in APPIMAGE_PATH_VARS {
        let Some(val) = read(key) else { continue };
        let original: Vec<PathBuf> = std::env::split_paths(&val).collect();
        let kept: Vec<PathBuf> = original
            .iter()
            .filter(|p| !p.as_os_str().is_empty() && !p.starts_with(appdir))
            .cloned()
            .collect();
        if kept.len() == original.len() {
            continue; // nothing AppImage-injected; leave as-is
        }
        match std::env::join_paths(&kept) {
            Ok(joined) if !kept.is_empty() => out.push((key, Some(joined))),
            _ => out.push((key, None)),
        }
    }

    for &key in APPIMAGE_VALUE_VARS {
        if read(key).is_some_and(|v| Path::new(&v).starts_with(appdir)) {
            out.push((key, None));
        }
    }

    for &key in APPIMAGE_MARKER_VARS {
        if read(key).is_some() {
            out.push((key, None));
        }
    }

    out
}

// The environment model and its path mapping live in the Tauri-free core so
// the remote agent can share them.
pub use terax_workspace_core::env::{resolve_path, WorkspaceEnv};
#[cfg(windows)]
pub use terax_workspace_core::env::{
    validate_wsl_distro_name, wsl_drvfs_to_windows, wsl_path_to_host, wsl_path_to_unc,
};

/// The backend that serves a workspace. Every workspace is opened directly
/// today; remote SSH workspaces will return their live connection here.
/// The `Workspace` that reaches `env`. A server's is its live connection;
/// without one, every operation fails with `NotConnected`.
pub fn backend_for(env: &WorkspaceEnv) -> std::sync::Arc<dyn terax_workspace_core::workspace::Workspace> {
    match env {
        // The host's live connection; anything else is `NotConnected`,
        // never this machine.
        WorkspaceEnv::Ssh { host } => match crate::modules::remote::registry::global()
            .and_then(|registry| registry.workspace(host))
        {
            Some(workspace) => workspace,
            None => std::sync::Arc::new(terax_workspace_core::unavailable::Unavailable::new(
                env.clone(),
                format!("not connected to {host}"),
            )),
        },
        _ => std::sync::Arc::new(terax_workspace_core::host::HostWorkspace::new(env.clone())),
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct WslDistro {
    pub name: String,
    pub default: bool,
    pub running: bool,
}

#[cfg(windows)]
pub fn decode_command_output(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0xff, 0xfe]) || looks_utf16le(bytes) {
        let start = if bytes.starts_with(&[0xff, 0xfe]) {
            2
        } else {
            0
        };
        let units: Vec<u16> = bytes[start..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_le_bytes(*c))
            .collect();
        String::from_utf16_lossy(&units)
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    }
}

#[cfg(windows)]
fn looks_utf16le(bytes: &[u8]) -> bool {
    if bytes.len() < 4 || !bytes.len().is_multiple_of(2) {
        return false;
    }
    let nul_odd = bytes.iter().skip(1).step_by(2).filter(|b| **b == 0).count();
    nul_odd * 2 >= bytes.len() / 2
}

#[cfg(windows)]
fn run_wsl(args: &[&str]) -> Result<String, String> {
    let mut cmd = std::process::Command::new("wsl.exe");
    cmd.args(args);
    crate::modules::proc::hide_console(&mut cmd);
    let out = cmd.output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        let stderr = decode_command_output(&out.stderr);
        return Err(stderr.trim().to_string());
    }
    Ok(decode_command_output(&out.stdout))
}

#[cfg(windows)]
pub(crate) fn wsl_exec_capture(
    distro: &str,
    program: &str,
    args: &[&str],
) -> Result<String, String> {
    validate_wsl_distro_name(distro)?;
    let mut cmd = std::process::Command::new("wsl.exe");
    cmd.arg("-d")
        .arg(distro)
        .arg("--exec")
        .arg(program)
        .args(args);
    crate::modules::proc::hide_console(&mut cmd);
    let out = cmd.output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        let stderr = decode_command_output(&out.stderr);
        return Err(stderr.trim().to_string());
    }
    Ok(decode_command_output(&out.stdout))
}

#[cfg(windows)]
fn run_wsl_sh(distro: &str, script: &str) -> Result<String, String> {
    // Probe helpers must avoid login-shell startup files. User `.profile`
    // output on stdout would corrupt the parsed value (`$HOME`, login shell).
    wsl_exec_capture(distro, "sh", &["-c", script])
}

#[cfg(windows)]
pub(crate) fn normalize_wsl_value(output: String, fallback: &str) -> String {
    let value = output
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("");
    if value.is_empty() {
        fallback.to_string()
    } else {
        value.to_string()
    }
}

#[cfg(windows)]
fn list_distros_blocking() -> Result<Vec<WslDistro>, String> {
    let out = run_wsl(&["--list", "--verbose"])?;
    let mut distros = Vec::new();
    for raw in out.lines().skip(1) {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        let default = line.starts_with('*');
        let line = line.trim_start_matches('*').trim();
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 3 {
            continue;
        }
        let state_idx = parts.len() - 2;
        let name = parts[..state_idx].join(" ");
        let state = parts[state_idx];
        distros.push(WslDistro {
            name,
            default,
            running: state.eq_ignore_ascii_case("Running"),
        });
    }
    Ok(distros)
}

#[tauri::command]
pub async fn wsl_list_distros() -> Result<Vec<WslDistro>, String> {
    #[cfg(not(windows))]
    {
        Ok(Vec::new())
    }
    #[cfg(windows)]
    {
        tauri::async_runtime::spawn_blocking(list_distros_blocking)
            .await
            .map_err(|e| e.to_string())?
    }
}

#[tauri::command]
pub async fn wsl_default_distro() -> Result<Option<String>, String> {
    #[cfg(not(windows))]
    {
        Ok(None)
    }
    #[cfg(windows)]
    {
        tauri::async_runtime::spawn_blocking(|| {
            let distros = list_distros_blocking()?;
            Ok(distros
                .iter()
                .find(|d| d.default)
                .map(|d| d.name.clone())
                .or_else(|| distros.first().map(|d| d.name.clone())))
        })
        .await
        .map_err(|e| e.to_string())?
    }
}

/// Spawns `wsl.exe` on Windows, so the command wrapper keeps it off the main
/// thread; internal callers are already on the blocking pool.
pub fn wsl_home(distro: String) -> Result<String, String> {
    #[cfg(not(windows))]
    {
        let _ = distro;
        Err("WSL is only available on Windows".into())
    }
    #[cfg(windows)]
    {
        let out = run_wsl_sh(&distro, "printf %s \"$HOME\"")?;
        let home = normalize_wsl_value(out, "");
        if home.is_empty() {
            Err(format!("could not resolve WSL home for {distro}"))
        } else {
            Ok(home)
        }
    }
}

/// IPC entry points whose names are fixed by the frontend but whose blocking
/// counterparts are also called synchronously from the PTY spawn path.
pub mod ipc {
    #[tauri::command]
    pub async fn wsl_home(distro: String) -> Result<String, String> {
        crate::modules::fs::blocking(move || super::wsl_home(distro)).await
    }
}

#[cfg(windows)]
pub fn wsl_login_shell(distro: String) -> Result<String, String> {
    const SCRIPT: &str = r#"uid="$(id -u 2>/dev/null || printf '')"
entry=''
if [ -n "$uid" ] && command -v getent >/dev/null 2>&1; then
  entry="$(getent passwd "$uid" 2>/dev/null || true)"
fi
if [ -z "$entry" ] && [ -n "$uid" ] && [ -r /etc/passwd ]; then
  entry="$(awk -F: -v u="$uid" '$3 == u { print; exit }' /etc/passwd 2>/dev/null)"
fi
shell=''
if [ -n "$entry" ]; then
  shell="${entry##*:}"
fi
if [ -z "$shell" ] && [ -n "$SHELL" ]; then
  shell="$SHELL"
fi
if [ -z "$shell" ]; then
  shell=/bin/sh
fi
printf %s "$shell""#;

    let out = run_wsl_sh(&distro, SCRIPT)?;
    Ok(normalize_wsl_value(out, "/bin/sh"))
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn decode_command_output_reads_plain_utf8() {
        assert_eq!(decode_command_output(b"hello world"), "hello world");
    }

    #[test]
    fn decode_command_output_strips_utf16le_bom() {
        // BOM (FF FE) followed by "hi" as little-endian UTF-16.
        let bytes = [0xff, 0xfe, 0x68, 0x00, 0x69, 0x00];
        assert_eq!(decode_command_output(&bytes), "hi");
    }

    #[test]
    fn decode_command_output_detects_bomless_utf16le() {
        // "test" as little-endian UTF-16, no BOM; the odd-position NULs trip the
        // heuristic.
        let bytes = [0x74, 0x00, 0x65, 0x00, 0x73, 0x00, 0x74, 0x00];
        assert_eq!(decode_command_output(&bytes), "test");
    }

    #[test]
    fn decode_command_output_keeps_ascii_without_nuls_as_utf8() {
        assert_eq!(decode_command_output(b"C:\\Users"), "C:\\Users");
    }

    #[test]
    fn normalize_wsl_value_uses_last_nonempty_line() {
        assert_eq!(
            normalize_wsl_value("banner\n  /bin/zsh \n".into(), "/bin/sh"),
            "/bin/zsh"
        );
    }

    #[test]
    fn normalize_wsl_value_falls_back_when_empty() {
        assert_eq!(normalize_wsl_value(" \n".into(), "/bin/sh"), "/bin/sh");
    }
}

#[cfg(test)]
mod auth_tests {
    use super::*;
    use std::env;
    use std::fs;

    fn loc(path: &Path) -> Location {
        Location::of_host(path).expect("a Unicode path")
    }

    fn tempdir(label: &str) -> PathBuf {
        let mut p = env::temp_dir();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        p.push(format!("terax-auth-{label}-{nanos}-{}", std::process::id()));
        fs::create_dir_all(&p).expect("create tempdir");
        fs::canonicalize(&p).expect("canonicalize tempdir")
    }

    /// A name that is not valid Unicode: `repo` plus an invalid byte on Unix,
    /// plus an unpaired surrogate on Windows. Its lossy spelling is `repo\u{FFFD}`.
    fn non_unicode_name() -> std::ffi::OsString {
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            std::ffi::OsStr::from_bytes(b"repo\xff").to_os_string()
        }
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStringExt;
            std::ffi::OsString::from_wide(&[0x72, 0x65, 0x70, 0x6f, 0xD800])
        }
    }

    /// A root named `repo\u{FFFD}` must not cover a sibling whose real name only
    /// *displays* the same: grants compare names exactly, as `PathBuf` did.
    #[test]
    fn a_non_unicode_sibling_cannot_alias_an_authorized_root() {
        let parent = tempdir("alias");
        let root = parent.join("repo\u{FFFD}");
        let outside = parent.join(non_unicode_name());
        fs::create_dir(&root).unwrap();
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("secret.txt"), b"x").unwrap();
        assert_eq!(
            outside.to_string_lossy(),
            root.to_string_lossy(),
            "the fixture's two names display alike"
        );

        let reg = WorkspaceRegistry::default();
        reg.authorize(&root).unwrap();
        assert!(reg.is_authorized(&root.join("file.txt")));
        assert!(!reg.is_authorized(&outside), "a look-alike sibling is not the root");
        assert!(!reg.is_authorized(&outside.join("secret.txt")));
        assert!(
            reg.authorize(&outside).is_err(),
            "a root that cannot be spelled exactly is refused"
        );
    }

    /// A look-alike never reaches an authorization check as a string.
    #[test]
    fn a_path_that_cannot_be_spelled_exactly_is_never_covered() {
        let parent = tempdir("alias-canon");
        let outside = parent.join(non_unicode_name());
        fs::create_dir(&outside).unwrap();
        let link_free = outside.join("secret.txt");
        fs::write(&link_free, b"x").unwrap();
        let ws = backend_for(&WorkspaceEnv::Local);
        // The frontend can only send the lossy spelling, which names another
        // (missing) directory; the backend must not resolve it to this one.
        let lossy = link_free.to_string_lossy().into_owned();
        assert!(ws.canonicalize(&lossy).is_err());
        // Even inside an authorized root, a path that cannot be spelled exactly
        // is not covered (it could only be compared by its lossy spelling).
        let reg = WorkspaceRegistry::default();
        reg.authorize(&parent).unwrap();
        for host in [outside.clone(), link_free.clone()] {
            assert!(
                !reg.is_authorized(&host),
                "{host:?} cannot be represented exactly, so it is not covered"
            );
        }
    }

    #[test]
    fn host_paths_name_a_location_in_their_workspace() {
        let wsl = |distro: &str, path: &str| Location {
            env: WorkspaceEnv::Wsl {
                distro: distro.into(),
            },
            path: path.into(),
        };
        let local = |path: &str| Location {
            env: WorkspaceEnv::Local,
            path: path.into(),
        };
        assert_eq!(location_of_canon("//wsl.localhost/Ubuntu/home/u", true), wsl("Ubuntu", "/home/u"));
        assert_eq!(location_of_canon("//WSL$/Ubuntu/home/u", true), wsl("Ubuntu", "/home/u"));
        assert_eq!(location_of_canon("//wsl.localhost/Ubuntu", true), wsl("Ubuntu", "/"));
        assert_eq!(location_of_canon("C:/Users/u", true), local("C:/Users/u"));
        assert_eq!(location_of_canon("//server/share/x", true), local("//server/share/x"));
        // Elsewhere there is no WSL; the same text is a local path.
        assert_eq!(
            location_of_canon("//wsl.localhost/Ubuntu/home/u", false),
            local("//wsl.localhost/Ubuntu/home/u")
        );
        assert_eq!(
            Location::of_host(Path::new(r"\\?\C:\Users\u")).unwrap().path,
            if cfg!(windows) { "C:/Users/u" } else { r"\\?\C:\Users\u" }
        );
    }

    #[test]
    fn containment_is_by_component_and_never_crosses_workspaces() {
        let at = |env: WorkspaceEnv, path: &str| Location {
            env,
            path: path.into(),
        };
        let ubuntu = || WorkspaceEnv::Wsl {
            distro: "Ubuntu".into(),
        };
        let root = at(ubuntu(), "/home/u");
        assert!(root.contains(&at(ubuntu(), "/home/u")));
        assert!(root.contains(&at(ubuntu(), "/home/u/src/a.rs")));
        assert!(root.contains(&at(ubuntu(), "/home/u/")));
        assert!(!root.contains(&at(ubuntu(), "/home/user")));
        assert!(!root.contains(&at(ubuntu(), "/home")));
        assert!(
            root.contains(&at(WorkspaceEnv::Wsl { distro: "ubuntu".into() }, "/home/u/x")),
            "distro names are case-insensitive"
        );
        assert!(!root.contains(&at(WorkspaceEnv::Wsl { distro: "Debian".into() }, "/home/u")));
        assert!(
            !root.contains(&at(WorkspaceEnv::Local, "/home/u")),
            "a WSL root does not cover the identical local path"
        );
        assert!(!at(WorkspaceEnv::Local, "/home/u").contains(&root));
        assert!(at(WorkspaceEnv::Local, "/").contains(&at(WorkspaceEnv::Local, "/etc")));
        assert!(at(WorkspaceEnv::Local, "C:/").contains(&at(WorkspaceEnv::Local, "C:/Users")));

        let roots: HashSet<Location> = [root.clone()].into();
        assert!(covered(&at(ubuntu(), "/home/u/x"), &roots, &HashMap::new(), 0));
        assert!(!covered(&at(WorkspaceEnv::Local, "/home/u/x"), &roots, &HashMap::new(), 0));
    }

    #[test]
    fn drvfs_paths_inside_a_distro_are_the_local_drive() {
        let ubuntu = WorkspaceEnv::Wsl {
            distro: "Ubuntu".into(),
        };
        let local = |path: &str| Location {
            env: WorkspaceEnv::Local,
            path: path.into(),
        };
        assert_eq!(location_in_workspace(&ubuntu, "/mnt/c/proj", true), local("C:/proj"));
        assert_eq!(location_in_workspace(&ubuntu, "/mnt/d", true), local("D:/"));
        for not_a_drive in ["/mnt/cd/x", "/mnt/", "/mnt", "/home/u", "/mnt/1/x"] {
            assert_eq!(
                location_in_workspace(&ubuntu, not_a_drive, true).env,
                ubuntu,
                "{not_a_drive}"
            );
        }
        assert_eq!(
            location_in_workspace(&ubuntu, "/mnt/c/proj", false).env,
            ubuntu,
            "there is no drvfs off Windows"
        );
        assert_eq!(
            location_in_workspace(&WorkspaceEnv::Local, "/mnt/c/proj", true),
            local("/mnt/c/proj")
        );
    }

    /// Grants made through the host-path adapters and checks made through the
    /// backend's canonical workspace paths must name the same location, or a
    /// root would stop covering itself while callers move over.
    #[cfg(windows)]
    #[test]
    fn host_and_workspace_spellings_name_the_same_location() {
        use terax_workspace_core::paths::canonical_wsl_path;
        for host in [
            "//wsl.localhost/Ubuntu/home/u/repo",
            "//wsl$/Ubuntu/home/u",
            "//wsl.localhost/Ubuntu",
            "C:/Users/u/proj",
            "C:/",
        ] {
            let workspace = canonical_wsl_path(host, "Ubuntu").unwrap();
            let env = WorkspaceEnv::Wsl {
                distro: "Ubuntu".into(),
            };
            assert_eq!(
                Location::in_workspace(&env, &workspace),
                location_of_canon(host, true),
                "{host} -> {workspace}"
            );
        }
    }

    #[test]
    fn workspace_paths_are_covered_by_component_within_their_workspace() {
        let reg = WorkspaceRegistry::default();
        let root = tempdir("covers-root");
        reg.authorize(&root).unwrap();
        let ws = backend_for(&WorkspaceEnv::Local);
        let canonical = reg.canonicalize_in(&*ws, &root.to_string_lossy()).unwrap();
        assert!(reg.covers(&WorkspaceEnv::Local, &canonical));
        assert!(reg.covers(&WorkspaceEnv::Local, &format!("{canonical}/src/a.rs")));
        assert!(!reg.covers(&WorkspaceEnv::Local, &format!("{canonical}-sibling")));
        assert!(within(&WorkspaceEnv::Local, &format!("{canonical}/a"), &canonical));
        assert!(!within(&WorkspaceEnv::Local, &canonical, &format!("{canonical}/a")));
    }

    #[test]
    fn derived_root_is_authorized_while_its_parent_is_and_after_revoke_is_not() {
        let reg = WorkspaceRegistry::default();
        let parent = tempdir("derived-parent");
        let child = tempdir("derived-child");
        reg.authorize(&parent).unwrap();
        assert!(!reg.is_authorized(&child));

        reg.authorize_derived(&child, &parent).unwrap();
        assert!(reg.is_authorized(&child));
        assert!(reg.is_authorized(&child.join("nested/file.txt")));
        assert!(!reg.is_authorized(child.parent().unwrap()));

        assert!(reg.revoke_derived(&child));
        assert!(!reg.is_authorized(&child));
        assert!(!reg.revoke_derived(&child), "second revoke is a no-op");
    }

    #[test]
    fn locations_compare_equal_across_windows_spellings_only_on_windows() {
        let verbatim = normalize_location(r"\\?\C:\Users\Runner\Temp\wt", true);
        for spelling in [
            "C:/Users/Runner/Temp/wt",
            r"C:\Users\Runner\Temp\wt",
            r"c:\users\runner\temp\WT",
            "C:/Users/Runner/Temp/wt/",
        ] {
            assert_eq!(normalize_location(spelling, true), verbatim, "{spelling}");
        }
        assert_eq!(
            normalize_location(r"\\?\UNC\server\share\wt", true),
            normalize_location("//server/share/wt", true)
        );
        assert_ne!(
            normalize_location(r"C:\Users\a", true),
            normalize_location(r"C:\Users\b", true)
        );
        // Elsewhere a backslash is a name character and case matters.
        assert_ne!(
            normalize_location(r"/tmp/a\b", false),
            normalize_location("/tmp/a/b", false)
        );
        assert_ne!(
            normalize_location("/tmp/Wt", false),
            normalize_location("/tmp/wt", false)
        );
        assert_eq!(
            normalize_location("/tmp/wt/", false),
            normalize_location("/tmp/wt", false)
        );
    }

    #[test]
    fn a_grant_on_a_deleted_directory_is_revoked_whatever_the_spelling() {
        let reg = WorkspaceRegistry::default();
        let parent = tempdir("revoke-parent");
        reg.authorize(&parent).unwrap();
        // Deliberately not canonical (on macOS /var/folders is /private/var/...).
        let spelled = env::temp_dir().join(format!(
            "terax-revoke-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        fs::create_dir_all(&spelled).unwrap();
        let canonical = fs::canonicalize(&spelled).unwrap();
        reg.authorize_derived(&spelled, &parent).unwrap();
        assert!(reg.is_authorized(&canonical));

        fs::remove_dir_all(&spelled).unwrap();
        assert!(
            reg.revoke_derived(&spelled),
            "the grant is found although the directory is gone"
        );
        assert!(!reg.is_authorized(&canonical));
        assert!(reg.derived.lock().unwrap().is_empty());
        assert!(!reg.revoke_derived(&spelled), "nothing is left to revoke");
    }

    #[test]
    fn revoking_one_grant_leaves_the_others() {
        let reg = WorkspaceRegistry::default();
        let parent = tempdir("revoke-keep-parent");
        let a = tempdir("revoke-keep-a");
        let b = tempdir("revoke-keep-b");
        reg.authorize(&parent).unwrap();
        reg.authorize_derived(&a, &parent).unwrap();
        reg.authorize_derived(&b, &parent).unwrap();
        assert!(reg.revoke_derived(&a));
        assert!(!reg.is_authorized(&a));
        assert!(reg.is_authorized(&b));
    }

    #[test]
    fn a_child_that_is_already_authorized_keeps_its_grant() {
        let reg = WorkspaceRegistry::default();
        let main = tempdir("keep-main");
        let task = tempdir("keep-task");
        reg.authorize(&main).unwrap();
        reg.authorize_derived(&task, &main).unwrap();

        // Listing the worktrees from inside the task itself derives the task
        // from the task; that must not replace `task -> main`.
        reg.authorize_derived(&task, &task).unwrap();
        assert!(reg.is_authorized(&task), "the grant still reaches a root");
        assert_eq!(reg.derived.lock().unwrap().get(&loc(&task)), Some(&loc(&main)));
        assert!(reg.revoke_derived(&task));
        assert!(!reg.is_authorized(&task));

        // Neither does a second grant through another authorized parent.
        let other = tempdir("keep-other");
        reg.authorize(&other).unwrap();
        reg.authorize_derived(&task, &main).unwrap();
        reg.authorize_derived(&task, &other).unwrap();
        assert_eq!(reg.derived.lock().unwrap().get(&loc(&task)), Some(&loc(&main)));
    }

    #[test]
    fn a_child_inside_a_root_needs_no_grant_of_its_own() {
        let reg = WorkspaceRegistry::default();
        let main = tempdir("inside-main");
        reg.authorize(&main).unwrap();
        let nested = main.join("worktrees/task");
        fs::create_dir_all(&nested).unwrap();
        let nested = fs::canonicalize(&nested).unwrap();
        reg.authorize_derived(&nested, &main).unwrap();
        assert!(reg.is_authorized(&nested));
        assert!(reg.derived.lock().unwrap().is_empty());
    }

    #[test]
    fn concurrent_authorizations_never_leave_a_self_parented_grant() {
        let reg = std::sync::Arc::new(WorkspaceRegistry::default());
        let main = tempdir("race-main");
        reg.authorize(&main).unwrap();
        let tasks: Vec<PathBuf> = (0..8).map(|i| tempdir(&format!("race-task-{i}"))).collect();
        let threads: Vec<_> = tasks
            .iter()
            .map(|task| {
                let (reg, task, main) = (reg.clone(), task.clone(), main.clone());
                std::thread::spawn(move || {
                    for _ in 0..50 {
                        reg.authorize_derived(&task, &main).unwrap();
                        reg.authorize_derived(&task, &task).unwrap();
                    }
                })
            })
            .collect();
        for thread in threads {
            thread.join().unwrap();
        }
        for task in &tasks {
            assert!(reg.is_authorized(task), "{task:?}");
            assert_eq!(reg.derived.lock().unwrap().get(&loc(task)), Some(&loc(&main)));
        }
    }

    #[test]
    fn derived_root_requires_an_authorized_parent() {
        let reg = WorkspaceRegistry::default();
        let parent = tempdir("derived-orphan-parent");
        let child = tempdir("derived-orphan-child");
        let err = reg.authorize_derived(&child, &parent).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
        assert!(!reg.is_authorized(&child));
    }

    #[test]
    fn derived_roots_chain_but_a_dangling_chain_grants_nothing() {
        let reg = WorkspaceRegistry::default();
        let root = tempdir("derived-chain-root");
        let mid = tempdir("derived-chain-mid");
        let leaf = tempdir("derived-chain-leaf");
        reg.authorize(&root).unwrap();
        reg.authorize_derived(&mid, &root).unwrap();
        reg.authorize_derived(&leaf, &mid).unwrap();
        assert!(reg.is_authorized(&leaf));

        reg.revoke_derived(&mid);
        assert!(
            !reg.is_authorized(&leaf),
            "a leaf whose parent lost its grant is no longer covered"
        );
    }

    #[test]
    fn derived_cycles_terminate() {
        let reg = WorkspaceRegistry::default();
        let root = tempdir("derived-cycle-root");
        let a = tempdir("derived-cycle-a");
        let b = tempdir("derived-cycle-b");
        reg.authorize(&root).unwrap();
        reg.authorize_derived(&a, &root).unwrap();
        reg.authorize_derived(&b, &a).unwrap();
        reg.derived.lock().unwrap().insert(loc(&a), loc(&b));
        assert!(!reg.is_authorized(&a));
        assert!(!reg.is_authorized(&b));
    }

    #[test]
    fn authorize_spawn_cwd_accepts_none() {
        let reg = WorkspaceRegistry::default();
        assert!(authorize_spawn_cwd(&reg, None, &WorkspaceEnv::Local)
            .unwrap()
            .is_none());
    }

    #[test]
    fn authorize_spawn_cwd_accepts_empty_string() {
        let reg = WorkspaceRegistry::default();
        assert!(authorize_spawn_cwd(&reg, Some("   "), &WorkspaceEnv::Local)
            .unwrap()
            .is_none());
    }

    #[test]
    fn authorize_spawn_cwd_accepts_authorized_path() {
        let dir = tempdir("ok");
        let reg = WorkspaceRegistry::default();
        reg.authorize(&dir).expect("authorize root");
        let s = dir.to_string_lossy().into_owned();
        let resolved = authorize_spawn_cwd(&reg, Some(&s), &WorkspaceEnv::Local)
            .expect("authorized")
            .expect("returned canonical");
        assert_eq!(resolved, dir);
    }

    #[test]
    fn authorize_spawn_cwd_accepts_subdir_of_authorized_root() {
        let root = tempdir("subroot");
        let sub = root.join("inside");
        fs::create_dir_all(&sub).expect("subdir");
        let canonical_sub = fs::canonicalize(&sub).expect("canon sub");
        let reg = WorkspaceRegistry::default();
        reg.authorize(&root).expect("authorize root");
        let s = canonical_sub.to_string_lossy().into_owned();
        let resolved = authorize_spawn_cwd(&reg, Some(&s), &WorkspaceEnv::Local)
            .expect("subdir authorized")
            .expect("returned canonical");
        assert_eq!(resolved, canonical_sub);
    }

    #[test]
    fn authorize_spawn_cwd_rejects_unauthorized_path() {
        let allowed = tempdir("allowed");
        let foreign = tempdir("foreign");
        let reg = WorkspaceRegistry::default();
        reg.authorize(&allowed).expect("authorize root");
        let s = foreign.to_string_lossy().into_owned();
        let err = authorize_spawn_cwd(&reg, Some(&s), &WorkspaceEnv::Local)
            .expect_err("should reject unauthorized cwd");
        assert!(err.contains("outside"), "got: {err}");
    }

    #[test]
    fn authorize_spawn_cwd_rejects_missing_path() {
        let mut missing = env::temp_dir();
        missing.push(format!(
            "terax-missing-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let reg = WorkspaceRegistry::default();
        let s = missing.to_string_lossy().into_owned();
        let err = authorize_spawn_cwd(&reg, Some(&s), &WorkspaceEnv::Local)
            .expect_err("should reject missing path");
        assert!(err.contains("cwd not accessible"), "got: {err}");
    }

    #[test]
    fn authorize_user_spawn_cwd_registers_unauthorized_path() {
        let dir = tempdir("userspawn");
        let reg = WorkspaceRegistry::default();
        let s = dir.to_string_lossy().into_owned();
        assert!(!reg.is_authorized(&dir));
        let resolved = authorize_user_spawn_cwd(&reg, Some(&s), &WorkspaceEnv::Local)
            .expect("user spawn allowed anywhere")
            .expect("returned canonical");
        assert_eq!(resolved, dir);
        assert!(reg.is_authorized(&dir));
    }

    #[test]
    fn authorize_user_spawn_cwd_rejects_missing_path() {
        let mut missing = env::temp_dir();
        missing.push(format!("terax-user-missing-{}", std::process::id()));
        let reg = WorkspaceRegistry::default();
        let s = missing.to_string_lossy().into_owned();
        let err = authorize_user_spawn_cwd(&reg, Some(&s), &WorkspaceEnv::Local)
            .expect_err("missing path must fail");
        assert!(err.contains("cwd not accessible"), "got: {err}");
    }

    #[test]
    fn user_spawn_cwd_or_home_keeps_accessible_dir() {
        let dir = tempdir("orhome-ok");
        let reg = WorkspaceRegistry::default();
        let s = dir.to_string_lossy().into_owned();
        assert_eq!(
            user_spawn_cwd_or_home(&reg, Some(&s), &WorkspaceEnv::Local),
            Some(s)
        );
        assert!(reg.is_authorized(&dir));
    }

    #[test]
    fn user_spawn_cwd_or_home_falls_back_when_inaccessible() {
        let mut missing = env::temp_dir();
        missing.push(format!("terax-orhome-missing-{}", std::process::id()));
        let reg = WorkspaceRegistry::default();
        let s = missing.to_string_lossy().into_owned();
        assert_eq!(
            user_spawn_cwd_or_home(&reg, Some(&s), &WorkspaceEnv::Local),
            None
        );
    }

    #[test]
    fn user_spawn_cwd_or_home_passes_through_empty() {
        let reg = WorkspaceRegistry::default();
        assert_eq!(
            user_spawn_cwd_or_home(&reg, None, &WorkspaceEnv::Local),
            None
        );
        assert_eq!(
            user_spawn_cwd_or_home(&reg, Some("  "), &WorkspaceEnv::Local),
            None
        );
    }

    #[test]
    fn authorize_spawn_cwd_blocks_symlink_escape() {
        let allowed = tempdir("symroot");
        let outside = tempdir("symtarget");
        let link = allowed.join("escape");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, &link).expect("symlink");
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(&outside, &link).expect("symlink");
        let reg = WorkspaceRegistry::default();
        reg.authorize(&allowed).expect("authorize root");
        let s = link.to_string_lossy().into_owned();
        let err = authorize_spawn_cwd(&reg, Some(&s), &WorkspaceEnv::Local)
            .expect_err("symlink-escape must be rejected");
        assert!(err.contains("outside"), "got: {err}");
    }

    #[test]
    fn resolve_launch_cwd_prefers_cli_dir_over_env() {
        let cli = tempdir("cli");
        let env = tempdir("env");
        let s = cli.to_string_lossy().into_owned();
        let resolved = resolve_launch_cwd(Some(&s), Some(env.clone()));
        assert_eq!(resolved.as_deref(), Some(cli.as_path()));
    }

    #[test]
    fn resolve_launch_cwd_falls_back_to_env_when_cli_missing() {
        let env = tempdir("envonly");
        assert_eq!(resolve_launch_cwd(None, Some(env.clone())), Some(env));
    }

    #[test]
    fn resolve_launch_cwd_ignores_nonexistent_cli_dir() {
        let env = tempdir("envfb");
        let resolved = resolve_launch_cwd(Some("/no/such/terax/dir"), Some(env.clone()));
        assert_eq!(resolved, Some(env));
    }
}

#[cfg(all(test, target_os = "linux"))]
mod appimage_tests {
    use super::*;
    use std::collections::HashMap;

    fn reader(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let map: HashMap<String, OsString> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), OsString::from(v)))
            .collect();
        move |k: &str| map.get(k).cloned()
    }

    fn find<'a>(
        out: &'a [(&'static str, Option<OsString>)],
        key: &str,
    ) -> Option<&'a Option<OsString>> {
        out.iter().find(|(k, _)| *k == key).map(|(_, v)| v)
    }

    #[test]
    fn strips_appdir_from_path_lists_and_unsets_when_empty() {
        let appdir = Path::new("/tmp/.mount_Terax_X");
        let env = reader(&[
            ("LD_LIBRARY_PATH", "/tmp/.mount_Terax_X/usr/lib:/usr/lib"),
            ("PATH", "/tmp/.mount_Terax_X/usr/bin:/usr/bin:/bin"),
            ("GST_PLUGIN_SYSTEM_PATH", "/tmp/.mount_Terax_X/usr/lib/gstreamer-1.0"),
            ("APPDIR", "/tmp/.mount_Terax_X"),
        ]);
        let out = compute_appimage_env_overrides(appdir, env);

        assert_eq!(find(&out, "LD_LIBRARY_PATH"), Some(&Some(OsString::from("/usr/lib"))));
        assert_eq!(find(&out, "PATH"), Some(&Some(OsString::from("/usr/bin:/bin"))));
        // Only an APPDIR entry, so the var is removed entirely.
        assert_eq!(find(&out, "GST_PLUGIN_SYSTEM_PATH"), Some(&None));
        assert_eq!(find(&out, "APPDIR"), Some(&None));
    }

    #[test]
    fn leaves_untouched_vars_alone() {
        let appdir = Path::new("/tmp/.mount_Terax_X");
        let env = reader(&[
            ("LD_LIBRARY_PATH", "/usr/lib:/usr/local/lib"),
            ("LD_PRELOAD", "/home/u/my.so"),
        ]);
        let out = compute_appimage_env_overrides(appdir, env);

        // No APPDIR component => no override emitted for these.
        assert!(find(&out, "LD_LIBRARY_PATH").is_none());
        assert!(find(&out, "LD_PRELOAD").is_none());
    }

    #[test]
    fn unsets_value_vars_only_when_pointing_into_appdir() {
        let appdir = Path::new("/tmp/.mount_Terax_X");
        let into = reader(&[("LD_PRELOAD", "/tmp/.mount_Terax_X/usr/lib/x.so")]);
        assert_eq!(find(&compute_appimage_env_overrides(appdir, into), "LD_PRELOAD"), Some(&None));

        let outside = reader(&[("FONTCONFIG_FILE", "/etc/fonts/fonts.conf")]);
        assert!(find(&compute_appimage_env_overrides(appdir, outside), "FONTCONFIG_FILE").is_none());
    }
}
