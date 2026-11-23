//! Watching a server's folders. The explorer and the editor tabs register
//! paths as they do on this machine; here they are counted per host and the
//! whole set is sent to the agent (`SetWatch`), which watches on the server
//! and pushes `Changed`. A connection that comes back gets the set again and
//! one catch-up delivery of every watched path, since anything may have
//! changed while it was down.

use std::collections::{BTreeMap, HashMap};
use std::io;
use std::sync::{Arc, Mutex};

use terax_remote_protocol::client::RemoteWorkspace;
use terax_workspace_core::env::SshHost;

/// Gets the paths that changed on `host`.
pub type Deliver = Arc<dyn Fn(&SshHost, Vec<String>) + Send + Sync>;

/// Every host's watched directories and files, reference counted.
#[derive(Default)]
pub struct RemoteWatches(Mutex<HashMap<String, Counts>>);

#[derive(Default)]
struct Counts {
    dirs: BTreeMap<String, usize>,
    files: BTreeMap<String, usize>,
}

/// One host's watched paths: what `SetWatch` sends.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WatchSet {
    pub dirs: Vec<String>,
    pub files: Vec<String>,
}

impl WatchSet {
    pub fn is_empty(&self) -> bool {
        self.dirs.is_empty() && self.files.is_empty()
    }

    fn all(&self) -> Vec<String> {
        self.dirs.iter().chain(&self.files).cloned().collect()
    }
}

fn key(path: &str) -> String {
    match path.trim_end_matches('/') {
        "" => "/".to_string(),
        trimmed => trimmed.to_string(),
    }
}

impl RemoteWatches {
    /// Adds a registration; returns the host's set when it changed.
    pub fn add(&self, host: &SshHost, dirs: &[String], files: &[String]) -> Option<WatchSet> {
        let mut hosts = self.0.lock().unwrap_or_else(|e| e.into_inner());
        let counts = hosts.entry(host.as_str().to_string()).or_default();
        let mut moved = false;
        for (paths, map) in [(dirs, &mut counts.dirs), (files, &mut counts.files)] {
            for path in paths {
                let count = map.entry(key(path)).or_insert(0);
                moved |= *count == 0;
                *count += 1;
            }
        }
        moved.then(|| set_of(counts))
    }

    /// Releases a registration; returns the host's set when it changed.
    pub fn remove(&self, host: &SshHost, dirs: &[String], files: &[String]) -> Option<WatchSet> {
        let mut hosts = self.0.lock().unwrap_or_else(|e| e.into_inner());
        let counts = hosts.get_mut(host.as_str())?;
        let mut moved = false;
        for (paths, map) in [(dirs, &mut counts.dirs), (files, &mut counts.files)] {
            for path in paths {
                let path = key(path);
                if let Some(count) = map.get_mut(&path) {
                    *count -= 1;
                    if *count == 0 {
                        map.remove(&path);
                        moved = true;
                    }
                }
            }
        }
        moved.then(|| set_of(counts))
    }

    pub fn set(&self, host: &SshHost) -> WatchSet {
        let hosts = self.0.lock().unwrap_or_else(|e| e.into_inner());
        hosts.get(host.as_str()).map(set_of).unwrap_or_default()
    }
}

fn set_of(counts: &Counts) -> WatchSet {
    WatchSet {
        dirs: counts.dirs.keys().cloned().collect(),
        files: counts.files.keys().cloned().collect(),
    }
}

/// Sends `set` to a live connection. Without one there is nothing to do:
/// `resume` sends it when the host is ready.
pub fn push(ws: Option<Arc<RemoteWorkspace>>, set: WatchSet) -> io::Result<()> {
    match ws {
        Some(ws) => ws.set_watch(set.dirs, set.files),
        None => Ok(()),
    }
}

/// For a connection that just became ready: routes its pushes to `deliver`,
/// sends the set, and delivers every watched path once.
pub fn resume(
    ws: &RemoteWorkspace,
    host: &SshHost,
    set: WatchSet,
    deliver: Deliver,
) -> io::Result<()> {
    let pushed = {
        let (host, deliver) = (host.clone(), deliver.clone());
        Arc::new(move |paths: Vec<String>| deliver(&host, paths))
    };
    ws.on_changed(pushed);
    if set.is_empty() {
        return Ok(());
    }
    let all = set.all();
    ws.set_watch(set.dirs, set.files)?;
    deliver(host, all);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host() -> SshHost {
        SshHost::try_from("box".to_string()).unwrap()
    }

    fn strings(paths: &[&str]) -> Vec<String> {
        paths.iter().map(|p| p.to_string()).collect()
    }

    #[test]
    fn registrations_are_counted_and_the_set_is_sent_only_when_it_changes() {
        let watches = RemoteWatches::default();
        let h = host();
        let set = watches.add(
            &h,
            &strings(&["/srv/a/", "/srv/b"]),
            &strings(&["/srv/a/x.rs"]),
        );
        assert_eq!(
            set,
            Some(WatchSet {
                dirs: strings(&["/srv/a", "/srv/b"]),
                files: strings(&["/srv/a/x.rs"]),
            })
        );
        // The same folder from a second view: nothing new to send.
        assert_eq!(watches.add(&h, &strings(&["/srv/a"]), &[]), None);
        assert_eq!(watches.remove(&h, &strings(&["/srv/a"]), &[]), None);
        let set = watches
            .remove(&h, &strings(&["/srv/a"]), &strings(&["/srv/a/x.rs"]))
            .unwrap();
        assert_eq!(set.dirs, strings(&["/srv/b"]));
        assert!(set.files.is_empty());
        // Released twice, or never added: ignored.
        assert_eq!(
            watches.remove(&h, &strings(&["/srv/a", "/nope"]), &[]),
            None
        );
        assert_eq!(watches.set(&h).dirs, strings(&["/srv/b"]));
        let other = SshHost::try_from("other".to_string()).unwrap();
        assert!(watches.set(&other).is_empty());
    }
}
