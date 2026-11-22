//! Copying files from this machine into a workspace directory: a paste from
//! the OS clipboard or a drop from the desktop. For a remote workspace the
//! same operation is an upload.

use std::ffi::{OsStr, OsString};
use std::io;
use std::path::Path;

use crate::host::rename_no_replace;

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
