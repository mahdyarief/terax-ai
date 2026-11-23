//! The walk behind the file finder (`fs_search`, `fs_list_files`) and the
//! packed list it produces. The cache that keeps one walk per root lives in
//! the app; only the walk runs next to the data.

use std::path::Path;

use ignore::WalkBuilder;
use serde::{Deserialize, Serialize};

use crate::paths::to_canon;

/// Hard cap on entries a walk may visit. Protects against roots like $HOME
/// where there is no .gitignore and the tree is effectively unbounded.
pub const MAX_SCANNED: usize = 50_000;

/// Directory names pruned unconditionally; they dominate scan time when present
/// and are rarely useful in a file finder.
pub const PRUNE_DIRS: &[&str] = &[
    "node_modules",
    ".git",
    "target",
    "dist",
    "build",
    ".next",
    ".turbo",
    ".cache",
    ".venv",
    "__pycache__",
];

/// Upper bound on the packed path text of one index (about 50k typical paths).
pub const MAX_PACKED_BYTES: usize = 16 * 1024 * 1024;
const DIR_BIT: u32 = 1 << 31;

/// Paths of one walk in a single string plus end offsets: about 12 bytes of
/// overhead per entry instead of a heap allocation each.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "PackedParts")]
pub struct Packed {
    buf: String,
    ends: Vec<u32>,
}

/// `Packed` as it arrives from elsewhere (a remote agent), before the checks
/// that keep `get` from slicing out of bounds or inside a character.
#[derive(Deserialize)]
struct PackedParts {
    buf: String,
    ends: Vec<u32>,
}

impl TryFrom<PackedParts> for Packed {
    type Error = String;

    fn try_from(parts: PackedParts) -> Result<Self, String> {
        if parts.buf.len() > MAX_PACKED_BYTES || parts.ends.len() > MAX_SCANNED {
            return Err("file index exceeds its budget".into());
        }
        let mut start = 0;
        for raw in &parts.ends {
            let end = (raw & !DIR_BIT) as usize;
            if end < start || end > parts.buf.len() || !parts.buf.is_char_boundary(end) {
                return Err("file index offsets are inconsistent".into());
            }
            start = end;
        }
        Ok(Self {
            buf: parts.buf,
            ends: parts.ends,
        })
    }
}

impl Packed {
    /// False when the buffer budget is exhausted; the entry is not stored.
    pub fn push(&mut self, rel: &str, is_dir: bool) -> bool {
        if self.buf.len() + rel.len() > MAX_PACKED_BYTES || self.ends.len() >= MAX_SCANNED {
            return false;
        }
        self.buf.push_str(rel);
        let end = self.buf.len() as u32;
        self.ends.push(if is_dir { end | DIR_BIT } else { end });
        true
    }

    pub fn len(&self) -> usize {
        self.ends.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ends.is_empty()
    }

    pub fn get(&self, index: usize) -> (&str, bool) {
        let start = if index == 0 {
            0
        } else {
            (self.ends[index - 1] & !DIR_BIT) as usize
        };
        let raw = self.ends[index];
        (
            &self.buf[start..(raw & !DIR_BIT) as usize],
            raw & DIR_BIT != 0,
        )
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, bool)> {
        (0..self.len()).map(|i| self.get(i))
    }

    /// Heap held by the buffers; for memory-budget tests.
    #[doc(hidden)]
    pub fn heap_bytes(&self) -> usize {
        self.buf.capacity() + self.ends.capacity() * std::mem::size_of::<u32>()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileIndex {
    pub entries: Packed,
    /// The walk stopped at a budget; results are a prefix of the tree.
    pub truncated: bool,
}

/// Walks `root` (a host path) for a file finder: `.gitignore` honoured,
/// heavy directories pruned, at most `MAX_SCANNED` entries. `None` when
/// cancelled.
pub fn walk(
    root: &Path,
    show_hidden: bool,
    max_depth: Option<usize>,
    cancelled: &dyn Fn() -> bool,
) -> Option<FileIndex> {
    if cancelled() {
        return None;
    }
    let walker = WalkBuilder::new(root)
        .hidden(!show_hidden)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .ignore(true)
        .parents(true)
        .follow_links(false)
        .max_depth(max_depth)
        .filter_entry(|dent| {
            // Prune known-heavy dirs even when no .gitignore is present.
            if dent.depth() == 0 {
                return true;
            }
            match dent.file_name().to_str() {
                Some(name) => !PRUNE_DIRS.contains(&name),
                None => true,
            }
        })
        .build();

    let mut entries = Packed::default();
    let mut scanned = 0usize;
    let mut truncated = false;
    for dent in walker {
        if cancelled() {
            return None;
        }
        let Ok(dent) = dent else { continue };
        scanned += 1;
        if scanned > MAX_SCANNED {
            truncated = true;
            break;
        }
        let Ok(relative) = dent.path().strip_prefix(root) else {
            continue;
        };
        let rel = to_canon(relative);
        if rel.is_empty() {
            continue;
        }
        let is_dir = dent.file_type().is_some_and(|t| t.is_dir());
        if !entries.push(&rel, is_dir) {
            truncated = true;
            break;
        }
    }
    Some(FileIndex { entries, truncated })
}
