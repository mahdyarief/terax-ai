//! Copying files from this machine into a workspace directory: a paste from
//! the OS clipboard or a drop from the desktop. For a remote workspace the
//! same operation is an upload.

use std::ffi::{OsStr, OsString};
use std::io;
use std::path::Path;

use crate::host::rename_no_replace;
use crate::workspace::Workspace;

const MAX_COPY_SUFFIX: usize = 10_000;
const MAX_COPY_DEPTH: usize = 128;

/// Copies `src` (a path on this machine) into the directory `dest` (a
/// canonical host path), named after `src`, and returns the name it got.
/// The item is assembled in a staging directory inside `dest` and published
/// with a no-replace rename, so a reader never sees a partial copy and an
/// existing entry is never replaced.
pub fn import_item(src: &Path, dest: &Path, rename_on_conflict: bool) -> io::Result<OsString> {
    if !src.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "copy source must be absolute",
        ));
    }
    let name = src.file_name().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "copy source has no filename")
    })?;
    let metadata = std::fs::symlink_metadata(src)?;
    if !rename_on_conflict && std::fs::symlink_metadata(dest.join(name)).is_ok() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "destination already exists",
        ));
    }
    if metadata.is_dir() && dest.starts_with(std::fs::canonicalize(src)?) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "cannot copy a folder into itself",
        ));
    }

    // Publish only complete items; the same-filesystem rename is no-replace.
    let staging = tempfile::Builder::new()
        .prefix(".terax-copy-")
        .tempdir_in(dest)?;
    let staged = staging.path().join("item");
    copy_recursive(src, &staged, 0)?;
    publish_copy(&staged, dest, name, metadata.is_dir(), rename_on_conflict)
}

fn publish_copy(
    staged: &Path,
    dest: &Path,
    name: &OsStr,
    is_dir: bool,
    rename_on_conflict: bool,
) -> io::Result<OsString> {
    for n in 0..=MAX_COPY_SUFFIX {
        let name = if n == 0 {
            name.to_os_string()
        } else {
            copy_name(name, n, is_dir)
        };
        let target = dest.join(&name);
        match rename_no_replace(staged, &target) {
            Ok(()) => return Ok(name),
            Err(error) => {
                // Windows can report AccessDenied for an existing directory.
                if std::fs::symlink_metadata(&target).is_err() {
                    return Err(error);
                }
                if !rename_on_conflict {
                    return Err(io::Error::new(
                        io::ErrorKind::AlreadyExists,
                        format!("already exists: {}", target.display()),
                    ));
                }
            }
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "no free copy name",
    ))
}

fn copy_recursive(src: &Path, dst: &Path, depth: usize) -> io::Result<()> {
    if depth > MAX_COPY_DEPTH {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "folder nesting exceeds the copy limit",
        ));
    }
    let metadata = std::fs::symlink_metadata(src)?;
    let kind = metadata.file_type();
    if kind.is_symlink() {
        copy_symlink(src, dst)
    } else if kind.is_dir() {
        std::fs::create_dir(dst)?;
        for entry in std::fs::read_dir(src)? {
            let entry = entry?;
            copy_recursive(&entry.path(), &dst.join(entry.file_name()), depth + 1)?;
        }
        Ok(())
    } else if kind.is_file() {
        let mut source = std::fs::File::open(src)?;
        let mut target = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(dst)?;
        io::copy(&mut source, &mut target)?;
        target.set_permissions(metadata.permissions())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "only files, folders and symbolic links can be copied",
        ))
    }
}

#[cfg(unix)]
fn copy_symlink(src: &Path, dst: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(std::fs::read_link(src)?, dst)
}

#[cfg(windows)]
fn copy_symlink(src: &Path, dst: &Path) -> io::Result<()> {
    use std::os::windows::fs::{symlink_dir, symlink_file, FileTypeExt};
    let target = std::fs::read_link(src)?;
    if std::fs::symlink_metadata(src)?.file_type().is_symlink_dir() {
        symlink_dir(target, dst)
    } else {
        symlink_file(target, dst)
    }
}

#[cfg(not(any(unix, windows)))]
fn copy_symlink(_src: &Path, _dst: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "copying symbolic links is unsupported",
    ))
}

pub(crate) fn copy_name(name: &OsStr, n: usize, is_dir: bool) -> OsString {
    let path = Path::new(name);
    let (stem, ext) = match (is_dir, path.file_stem(), path.extension()) {
        (false, Some(stem), Some(ext)) => (stem, Some(ext)),
        _ => (name, None),
    };
    let mut out = stem.to_os_string();
    out.push(if n == 1 {
        " copy".to_string()
    } else {
        format!(" copy {n}")
    });
    if let Some(ext) = ext {
        out.push(".");
        out.push(ext);
    }
    out
}

/// Uploads `src` (a path on this machine) into `dest_dir` on `ws`, the same
/// contract as `import_item` built from workspace calls, for a workspace
/// whose files are elsewhere (a server). The item is assembled in a staging
/// directory inside `dest_dir` and published with `rename`, which never
/// replaces, so a partial upload is never visible and is removed on
/// failure. Files over `max_file` bytes and symbolic links are refused:
/// a file crosses in one write.
pub fn upload_item(
    ws: &dyn Workspace,
    src: &Path,
    dest_dir: &str,
    rename_on_conflict: bool,
    max_file: u64,
) -> io::Result<String> {
    if !src.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "copy source must be absolute",
        ));
    }
    let name = utf8_name(src.file_name().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "copy source has no filename")
    })?)?;
    let metadata = std::fs::symlink_metadata(src)?;
    let join = |name: &str| {
        if dest_dir.ends_with('/') {
            format!("{dest_dir}{name}")
        } else {
            format!("{dest_dir}/{name}")
        }
    };
    if !rename_on_conflict && ws.symlink_metadata(&join(name)).is_ok() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "destination already exists",
        ));
    }
    let staging = join(&format!(".terax-copy-{}", unique_suffix()));
    ws.create_dir_all(&staging)?;
    let staged = format!("{staging}/item");
    let outcome = upload_recursive(ws, src, &staged, 0, max_file).and_then(|()| {
        for n in 0..=MAX_COPY_SUFFIX {
            let candidate = if n == 0 {
                name.to_string()
            } else {
                utf8_name(&copy_name(OsStr::new(name), n, metadata.is_dir()))?.to_string()
            };
            match ws.rename(&staged, &join(&candidate)) {
                Ok(()) => return Ok(candidate),
                Err(error) => {
                    if ws.symlink_metadata(&join(&candidate)).is_err() {
                        return Err(error);
                    }
                    if !rename_on_conflict {
                        return Err(io::Error::new(
                            io::ErrorKind::AlreadyExists,
                            format!("already exists: {}", join(&candidate)),
                        ));
                    }
                }
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "no free copy name",
        ))
    });
    // Whatever happened, the staging directory goes (empty after a publish).
    let _ = ws.remove_dir_all(&staging);
    outcome
}

fn utf8_name(name: &OsStr) -> io::Result<&str> {
    name.to_str().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "a file name that is not valid Unicode cannot be copied to a server",
        )
    })
}

fn unique_suffix() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    format!(
        "{}-{nanos:x}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    )
}

fn upload_recursive(
    ws: &dyn Workspace,
    src: &Path,
    dst: &str,
    depth: usize,
    max_file: u64,
) -> io::Result<()> {
    if depth > MAX_COPY_DEPTH {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "folder nesting exceeds the copy limit",
        ));
    }
    let metadata = std::fs::symlink_metadata(src)?;
    let kind = metadata.file_type();
    if kind.is_symlink() {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            format!(
                "symbolic links cannot be copied to a server yet: {}",
                src.display()
            ),
        ))
    } else if kind.is_dir() {
        ws.create_dir_all(dst)?;
        for entry in std::fs::read_dir(src)? {
            let entry = entry?;
            let name = entry.file_name();
            let child = format!("{dst}/{}", utf8_name(&name)?);
            upload_recursive(ws, &entry.path(), &child, depth + 1, max_file)?;
        }
        Ok(())
    } else if kind.is_file() {
        if metadata.len() > max_file {
            return Err(io::Error::new(
                io::ErrorKind::FileTooLarge,
                format!(
                    "{} is over the {} MiB a file can be copied to a server",
                    src.display(),
                    max_file / (1024 * 1024)
                ),
            ));
        }
        let bytes = std::fs::read(src)?;
        ws.write_atomic(dst, &bytes).map(|_| ())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "only files, folders and symbolic links can be copied",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_files_dotfiles_and_dotted_directories() {
        assert_eq!(copy_name(OsStr::new("a.txt"), 1, false), "a copy.txt");
        assert_eq!(copy_name(OsStr::new("a.txt"), 2, false), "a copy 2.txt");
        assert_eq!(copy_name(OsStr::new(".env"), 1, false), ".env copy");
        assert_eq!(
            copy_name(OsStr::new("Makefile"), 3, false),
            "Makefile copy 3"
        );
        assert_eq!(copy_name(OsStr::new("v1.2"), 1, true), "v1.2 copy");
    }
}

#[cfg(test)]
mod upload_tests {
    use super::*;
    use crate::env::WorkspaceEnv;
    use crate::host::HostWorkspace;

    struct Fixture {
        _dir: tempfile::TempDir,
        src: std::path::PathBuf,
        dest: String,
        ws: HostWorkspace,
    }

    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        std::fs::create_dir_all(src.join("pkg/inner")).unwrap();
        std::fs::write(src.join("notes.txt"), "hello").unwrap();
        std::fs::write(src.join("pkg/a.rs"), "fn a() {}").unwrap();
        std::fs::write(src.join("pkg/inner/b.rs"), "fn b() {}").unwrap();
        std::fs::create_dir_all(dir.path().join("dest")).unwrap();
        let ws = HostWorkspace::new(WorkspaceEnv::Local);
        let dest = ws
            .canonicalize(&dir.path().join("dest").to_string_lossy())
            .unwrap();
        Fixture {
            _dir: dir,
            src,
            dest,
            ws,
        }
    }

    fn names(ws: &HostWorkspace, dir: &str) -> Vec<String> {
        let mut names: Vec<_> = ws
            .read_dir(dir)
            .unwrap()
            .into_iter()
            .map(|e| e.name)
            .collect();
        names.sort();
        names
    }

    #[test]
    fn uploads_a_file_and_a_tree_and_leaves_no_staging() {
        let f = fixture();
        let got = upload_item(&f.ws, &f.src.join("notes.txt"), &f.dest, false, 1 << 20).unwrap();
        assert_eq!(got, "notes.txt");
        let got = upload_item(&f.ws, &f.src.join("pkg"), &f.dest, false, 1 << 20).unwrap();
        assert_eq!(got, "pkg");
        assert_eq!(names(&f.ws, &f.dest), ["notes.txt", "pkg"]);
        assert_eq!(names(&f.ws, &format!("{}/pkg/inner", f.dest)), ["b.rs"]);
        let read =
            f.ws.read(
                &format!("{}/pkg/inner/b.rs", f.dest),
                crate::workspace::ReadRequest {
                    offset: 0,
                    limit: 100,
                },
            )
            .unwrap();
        assert_eq!(read.bytes, b"fn b() {}");
    }

    #[test]
    fn a_conflict_fails_or_gets_a_copy_name() {
        let f = fixture();
        let notes = f.src.join("notes.txt");
        upload_item(&f.ws, &notes, &f.dest, false, 1 << 20).unwrap();
        let error = upload_item(&f.ws, &notes, &f.dest, false, 1 << 20).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(
            upload_item(&f.ws, &notes, &f.dest, true, 1 << 20).unwrap(),
            "notes copy.txt"
        );
        assert_eq!(
            upload_item(&f.ws, &notes, &f.dest, true, 1 << 20).unwrap(),
            "notes copy 2.txt"
        );
        assert_eq!(
            names(&f.ws, &f.dest),
            ["notes copy 2.txt", "notes copy.txt", "notes.txt"]
        );
    }

    #[test]
    fn a_failed_upload_leaves_nothing_behind() {
        let f = fixture();
        // Each file is 9 bytes, over this limit: the first one fails the tree.
        let error = upload_item(&f.ws, &f.src.join("pkg"), &f.dest, false, 8).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::FileTooLarge);
        assert!(names(&f.ws, &f.dest).is_empty(), "no item, no staging");
        let relative = upload_item(&f.ws, Path::new("notes.txt"), &f.dest, false, 8).unwrap_err();
        assert_eq!(relative.kind(), io::ErrorKind::InvalidInput);
    }
}
