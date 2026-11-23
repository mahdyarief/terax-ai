//! The agents this app carries for Linux servers, and their SHA-256.
//!
//! An installer bundles `binaries/terax-agent-<platform>` (built by
//! `scripts/build-agent.mjs`) as the resource `agents/terax-agent-<platform>`.
//! A debug build also looks in this source tree, so a developer can connect
//! after `pnpm build:agent` or a plain `cargo zigbuild`. The hash is computed
//! from the file the app will upload, so there is none to keep in sync.

use std::collections::HashMap;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

use sha2::{Digest, Sha256};

/// The platforms an agent is built for, as `bootstrap::plan` names them.
pub const AGENT_PLATFORMS: [&str; 2] = ["x86_64-linux-musl", "aarch64-linux-musl"];

/// Where the installer puts the agents, under the resource directory.
pub const RESOURCE_SUBDIR: &str = "agents";

pub fn file_name(platform: &str) -> String {
    format!("terax-agent-{platform}")
}

/// Rust's name for a platform's target (`x86_64-linux-musl` is
/// `x86_64-unknown-linux-musl`), where cargo puts its build output.
pub fn cargo_target(platform: &str) -> String {
    platform.replacen("-linux-", "-unknown-linux-", 1)
}

/// Where to look, in order: the installed resource, then (in a debug build,
/// `dev_root` being `src-tauri`) the prepared copy and cargo's own output.
pub fn candidates(
    platform: &str,
    resource_dir: Option<&Path>,
    dev_root: Option<&Path>,
) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(dir) = resource_dir {
        paths.push(dir.join(RESOURCE_SUBDIR).join(file_name(platform)));
    }
    if let Some(dev) = dev_root {
        paths.push(dev.join("binaries").join(file_name(platform)));
        paths.push(
            dev.join("target")
                .join(cargo_target(platform))
                .join("release")
                .join("terax-agent"),
        );
    }
    paths
}

pub fn find(
    platform: &str,
    resource_dir: Option<&Path>,
    dev_root: Option<&Path>,
) -> Option<PathBuf> {
    candidates(platform, resource_dir, dev_root)
        .into_iter()
        .find(|path| std::fs::metadata(path).is_ok_and(|meta| meta.is_file() && meta.len() > 0))
}

/// The source tree's `src-tauri`, in a debug build only: an installed app
/// must never pick up whatever happens to sit at its build path.
pub fn dev_root() -> Option<&'static Path> {
    cfg!(debug_assertions).then(|| Path::new(env!("CARGO_MANIFEST_DIR")))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BundledAgent {
    pub path: PathBuf,
    pub len: u64,
    /// Lowercase hex, as `sha256sum` prints it.
    pub sha256: String,
}

pub fn sha256_file(path: &Path) -> io::Result<(u64, String)> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    let mut len = 0u64;
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        len += read as u64;
    }
    let hex = hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    Ok((len, hex))
}

/// Finds and hashes agents on demand, rehashing a file only when its size
/// or modification time changes (a developer may rebuild it).
pub struct AgentBundle {
    resource_dir: Option<PathBuf>,
    dev_root: Option<PathBuf>,
    hashed: Mutex<HashMap<PathBuf, Hashed>>,
}

/// A hash, with the size and modification time of the file it was taken of.
struct Hashed {
    len: u64,
    modified: Option<SystemTime>,
    agent: BundledAgent,
}

impl AgentBundle {
    pub fn new(resource_dir: Option<PathBuf>, dev_root: Option<PathBuf>) -> Self {
        Self {
            resource_dir,
            dev_root,
            hashed: Mutex::new(HashMap::new()),
        }
    }

    /// The agent for `platform`, or `None` when this build carries none.
    pub fn get(&self, platform: &str) -> io::Result<Option<BundledAgent>> {
        let Some(path) = find(
            platform,
            self.resource_dir.as_deref(),
            self.dev_root.as_deref(),
        ) else {
            return Ok(None);
        };
        let meta = std::fs::metadata(&path)?;
        let (len, modified) = (meta.len(), meta.modified().ok());
        let mut hashed = self
            .hashed
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(known) = hashed.get(&path) {
            if (known.len, known.modified) == (len, modified) {
                return Ok(Some(known.agent.clone()));
            }
        }
        let (read, sha256) = sha256_file(&path)?;
        let agent = BundledAgent {
            path: path.clone(),
            len: read,
            sha256,
        };
        hashed.insert(
            path,
            Hashed {
                len,
                modified,
                agent: agent.clone(),
            },
        );
        Ok(Some(agent))
    }

    /// For `bootstrap::plan`: the hash of the agent for `platform`, if any.
    pub fn sha256(&self, platform: &str) -> Option<String> {
        self.get(platform).ok().flatten().map(|agent| agent.sha256)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const X86: &str = "x86_64-linux-musl";

    fn put(path: &Path, bytes: &[u8]) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }

    #[test]
    fn the_installed_resource_comes_first_then_the_dev_copies() {
        let resources = tempfile::tempdir().unwrap();
        let dev = tempfile::tempdir().unwrap();
        let resource = resources.path().join("agents").join(file_name(X86));
        let prepared = dev.path().join("binaries").join(file_name(X86));
        let built = dev
            .path()
            .join("target")
            .join("x86_64-unknown-linux-musl")
            .join("release")
            .join("terax-agent");
        let find = || find(X86, Some(resources.path()), Some(dev.path()));

        assert_eq!(find(), None);
        put(&built, b"built");
        assert_eq!(find(), Some(built.clone()));
        put(&prepared, b"prepared");
        assert_eq!(find(), Some(prepared.clone()));
        put(&resource, b"resource");
        assert_eq!(find(), Some(resource));
        // Without a dev root (a release build), only the resource counts.
        assert_eq!(super::find(X86, None, Some(dev.path())), Some(prepared));
        assert_eq!(super::find(X86, Some(dev.path()), None), None);
    }

    #[test]
    fn an_empty_file_or_a_directory_is_not_an_agent() {
        let resources = tempfile::tempdir().unwrap();
        let path = resources.path().join("agents").join(file_name(X86));
        put(&path, b"");
        assert_eq!(find(X86, Some(resources.path()), None), None);
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir_all(&path).unwrap();
        assert_eq!(find(X86, Some(resources.path()), None), None);
    }

    #[test]
    fn the_hash_is_sha256sums() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("f");
        std::fs::write(&path, b"abc").unwrap();
        assert_eq!(
            sha256_file(&path).unwrap(),
            (
                3,
                "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad".into()
            )
        );
        // Larger than one read buffer.
        let big = vec![b'a'; 1_000_000];
        std::fs::write(&path, &big).unwrap();
        assert_eq!(
            sha256_file(&path).unwrap().1,
            "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
        );
    }

    #[test]
    fn a_rebuilt_agent_is_hashed_again() {
        let resources = tempfile::tempdir().unwrap();
        let path = resources.path().join("agents").join(file_name(X86));
        put(&path, b"abc");
        let bundle = AgentBundle::new(Some(resources.path().to_path_buf()), None);
        let first = bundle.get(X86).unwrap().unwrap();
        assert_eq!((first.len, first.path.clone()), (3, path.clone()));
        assert_eq!(bundle.sha256(X86), Some(first.sha256.clone()));
        put(&path, b"abcd");
        assert_ne!(bundle.sha256(X86), Some(first.sha256));
        assert_eq!(bundle.sha256("aarch64-linux-musl"), None);
    }

    #[test]
    fn the_triples_are_the_bootstraps() {
        use terax_remote_protocol::bootstrap::{plan, Plan, Report};
        for (arch, platform) in [
            ("x86_64", AGENT_PLATFORMS[0]),
            ("aarch64", AGENT_PLATFORMS[1]),
        ] {
            let report = Report {
                os: "Linux".into(),
                arch: arch.into(),
                hasher: Some("sha256sum".into()),
                sha256: None,
            };
            assert_eq!(
                plan(&report, |_| Some("x".into())),
                Plan::Install { platform }
            );
        }
    }
}
