//! An image on the clipboard pasted into a terminal: saved as a PNG where the
//! terminal's programs can read it, and its path is what gets pasted.
//!
//! - this machine: `%TEMP%\terax-image` (`$TMPDIR/terax-image`);
//! - WSL: the same file, by its `/mnt/<drive>/...` path;
//! - a server: uploaded to `~/.cache/terax-image` there.
//!
//! Each save removes the folder's pastes older than a day. Only image pastes
//! land here; text is typed into the terminal as before.

use std::io;
use std::time::{SystemTime, UNIX_EPOCH};

use tauri::AppHandle;
use tauri_plugin_clipboard_manager::ClipboardExt;
use terax_workspace_core::workspace::{EntryKind, Workspace};

use crate::modules::workspace::{backend_for, WorkspaceEnv};

/// The folder's name, on this machine and on a server.
pub const FOLDER: &str = "terax-image";
/// A larger PNG is refused (a server's frame is 32 MiB).
pub const MAX_PNG_BYTES: usize = 20 * 1024 * 1024;
/// Pastes older than this are removed by the next one.
pub const KEEP_MS: u64 = 24 * 60 * 60 * 1000;
/// Our own files only: nothing else in the folder is ever removed.
const PREFIX: &str = "image-";

/// The clipboard's image as a PNG, or `None` when it holds no image.
fn clipboard_png(app: &AppHandle) -> Result<Option<Vec<u8>>, String> {
    let Ok(image) = app.clipboard().read_image() else {
        return Ok(None);
    };
    let png = encode_png(image.rgba(), image.width(), image.height())?;
    if png.len() > MAX_PNG_BYTES {
        return Err(format!(
            "The image is {} MB; images up to {} MB can be pasted.",
            png.len().div_ceil(1024 * 1024),
            MAX_PNG_BYTES / (1024 * 1024)
        ));
    }
    Ok(Some(png))
}

pub(crate) fn encode_png(rgba: &[u8], width: u32, height: u32) -> Result<Vec<u8>, String> {
    let expected = width as usize * height as usize * 4;
    if width == 0 || height == 0 || rgba.len() != expected {
        return Err("The clipboard's image could not be read.".into());
    }
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::Fast);
    let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
    writer.write_image_data(rgba).map_err(|e| e.to_string())?;
    writer.finish().map_err(|e| e.to_string())?;
    Ok(out)
}

/// `image-20261124-153012-042.png`, in UTC.
pub(crate) fn file_name(now_ms: u64) -> String {
    let secs = now_ms / 1000;
    let (year, month, day) = civil_from_days((secs / 86_400) as i64);
    let rest = secs % 86_400;
    format!(
        "{PREFIX}{year:04}{month:02}{day:02}-{:02}{:02}{:02}-{:03}.png",
        rest / 3600,
        rest / 60 % 60,
        rest % 60,
        now_ms % 1000
    )
}

/// Days since 1970-01-01 to a calendar date (Howard Hinnant's algorithm).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// Removes the folder's own pastes older than `KEEP_MS`; best effort.
pub(crate) fn prune(ws: &dyn Workspace, dir: &str, sep: char, now_ms: u64) {
    let Ok(entries) = ws.read_dir(dir) else {
        return;
    };
    for entry in entries {
        if entry.kind != EntryKind::File
            || !entry.name.starts_with(PREFIX)
            || !entry.name.ends_with(".png")
        {
            continue;
        }
        let path = format!("{dir}{sep}{}", entry.name);
        let old = ws
            .metadata(&path)
            .is_ok_and(|meta| meta.mtime_ms > 0 && now_ms.saturating_sub(meta.mtime_ms) > KEEP_MS);
        if old {
            let _ = ws.remove_file(&path);
        }
    }
}

/// Saves `png` in `dir` (created if missing) and returns its path.
pub(crate) fn save(
    ws: &dyn Workspace,
    dir: &str,
    sep: char,
    png: &[u8],
    now_ms: u64,
) -> io::Result<String> {
    ws.create_dir_all(dir)?;
    prune(ws, dir, sep, now_ms);
    let mut stamp = now_ms;
    loop {
        let path = format!("{dir}{sep}{}", file_name(stamp));
        // Two pastes in the same millisecond get the next free name.
        match ws.metadata(&path) {
            Ok(_) => stamp += 1,
            Err(_) => {
                ws.write_atomic(&path, png)?;
                return Ok(path);
            }
        }
    }
}

/// `C:\Users\me\x.png` as WSL mounts it: `/mnt/c/Users/me/x.png`.
pub(crate) fn wsl_mount_path(windows: &str) -> Option<String> {
    let mut chars = windows.chars();
    let drive = chars.next().filter(char::is_ascii_alphabetic)?;
    if chars.next() != Some(':') {
        return None;
    }
    let rest = chars.as_str().replace('\\', "/");
    let rest = rest.trim_start_matches('/');
    Some(format!("/mnt/{}/{rest}", drive.to_ascii_lowercase()))
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn save_for(workspace: &WorkspaceEnv, png: &[u8]) -> Result<String, String> {
    let now = now_ms();
    match workspace {
        WorkspaceEnv::Ssh { host } => {
            let remote = crate::modules::remote::registry::global()
                .and_then(|registry| registry.workspace(host))
                .ok_or_else(|| format!("{host} is not connected"))?;
            let home = remote.home().map_err(|e| e.to_string())?;
            let dir = format!("{}/.cache/{FOLDER}", home.trim_end_matches('/'));
            save(&*backend_for(workspace), &dir, '/', png, now).map_err(|e| e.to_string())
        }
        _ => {
            let dir = std::env::temp_dir().join(FOLDER);
            let dir = dir.to_string_lossy().into_owned();
            let sep = std::path::MAIN_SEPARATOR;
            let path = save(&*backend_for(&WorkspaceEnv::Local), &dir, sep, png, now)
                .map_err(|e| e.to_string())?;
            if matches!(workspace, WorkspaceEnv::Wsl { .. }) {
                return wsl_mount_path(&path).ok_or_else(|| format!("{path} has no WSL path"));
            }
            Ok(path)
        }
    }
}

/// The path to paste for the clipboard's image, saved where the terminal's
/// programs can read it; `None` when the clipboard holds no image.
#[tauri::command]
pub async fn pty_paste_image(
    app: AppHandle,
    workspace: Option<WorkspaceEnv>,
) -> Result<Option<String>, String> {
    let workspace = WorkspaceEnv::from_option(workspace);
    crate::modules::fs::blocking(move || {
        let Some(png) = clipboard_png(&app)? else {
            return Ok(None);
        };
        save_for(&workspace, &png).map(Some)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_sort_by_time_and_say_when() {
        // 2026-11-24 15:30:12.042 UTC
        assert_eq!(
            file_name(1_795_534_212_042),
            "image-20261124-153012-042.png"
        );
        assert_eq!(file_name(0), "image-19700101-000000-000.png");
        assert_eq!(file_name(951_782_400_000), "image-20000229-000000-000.png");
    }

    #[test]
    fn an_image_becomes_a_png_and_a_broken_one_is_refused() {
        let png = encode_png(&[255, 0, 0, 255, 0, 255, 0, 128], 2, 1).unwrap();
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        assert!(encode_png(&[0; 7], 2, 1).is_err());
        assert!(encode_png(&[], 0, 0).is_err());
    }

    #[test]
    fn wsl_reads_a_windows_file_through_its_mount() {
        assert_eq!(
            wsl_mount_path(r"C:\Users\me\AppData\Local\Temp\terax-image\a.png").as_deref(),
            Some("/mnt/c/Users/me/AppData/Local/Temp/terax-image/a.png")
        );
        assert_eq!(wsl_mount_path("D:/x.png").as_deref(), Some("/mnt/d/x.png"));
        assert_eq!(wsl_mount_path("/tmp/x.png"), None);
    }

    #[test]
    fn saving_keeps_recent_pastes_and_removes_only_old_ones_of_ours() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join(FOLDER);
        let folder_str = folder.to_string_lossy().into_owned();
        let ws = backend_for(&WorkspaceEnv::Local);
        let sep = std::path::MAIN_SEPARATOR;
        let first = save(&*ws, &folder_str, sep, b"one", now_ms()).unwrap();
        let second = save(&*ws, &folder_str, sep, b"two", now_ms()).unwrap();
        assert_ne!(first, second);
        assert_eq!(std::fs::read(&first).unwrap(), b"one");
        std::fs::write(folder.join("notes.txt"), b"mine").unwrap();
        // A day and a bit later, the next paste removes both old ones.
        let later = now_ms() + KEEP_MS + 60_000;
        let third = save(&*ws, &folder_str, sep, b"three", later).unwrap();
        assert!(!std::path::Path::new(&first).exists());
        assert!(!std::path::Path::new(&second).exists());
        assert!(std::path::Path::new(&third).exists());
        assert!(folder.join("notes.txt").exists());
    }
}
