//! Source control's live state on a server. A subscription lists the
//! repository's folders (its root, the tracked folders, the refs folders)
//! and git's own files (`HEAD`, `index`, `packed-refs`, each branch's ref),
//! and adds them to the host's remote watch (`remote/watch.rs`), so they
//! reach the agent with everything else that is watched there and come back
//! after a reconnect. A change pushed for one of them becomes a
//! `terax:git-changed` for the repository, with the kinds it stands for.
//!
//! Git's files are watched as files, by their stamp: git rewrites them
//! through a `<name>.lock` renamed into place, which leaves the folder's
//! listing as it was, so a folder watch alone never sees a commit.

use std::collections::HashMap;
use std::sync::Mutex;

use terax_workspace_core::env::SshHost;

/// Every subscribed repository on a server, by the key the frontend holds.
#[derive(Default)]
pub struct RemoteGitWatches(Mutex<HashMap<String, Repo>>);

struct Repo {
    host: SshHost,
    /// The repository's canonical root, as the server spells it.
    root: String,
    paths: Watched,
    subscribers: usize,
}

/// What a repository has watched: folders by their listing, git's files by
/// their stamp.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Watched {
    pub dirs: Vec<String>,
    pub files: Vec<String>,
}

/// The key a subscription is known by: unique per host, as two servers can
/// hold a repository at the same path.
pub fn key(host: &SshHost, root: &str) -> String {
    format!("ssh:{}:{}", host.as_str(), root.trim_end_matches('/'))
}

/// What a change to `path` (a watched folder or file) means for the
/// repository at `root`.
fn kinds(root: &str, path: &str) -> &'static [&'static str] {
    let root = root.trim_end_matches('/');
    let git = format!("{root}/.git");
    let path = path.trim_end_matches('/');
    if path == format!("{git}/HEAD") {
        &["head"]
    } else if path == format!("{git}/index") {
        &["index"]
    } else if path == git || path.starts_with(&format!("{git}/")) {
        &["refs"]
    } else {
        &["worktree"]
    }
}

impl RemoteGitWatches {
    /// Adds a subscriber; returns the key, and what to watch when this is
    /// the repository's first.
    pub fn subscribe(
        &self,
        host: &SshHost,
        root: &str,
        list: impl FnOnce() -> Watched,
    ) -> (String, Option<Watched>) {
        let key = key(host, root);
        let mut repos = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(repo) = repos.get_mut(&key) {
            repo.subscribers += 1;
            return (key, None);
        }
        let paths = list();
        repos.insert(
            key.clone(),
            Repo {
                host: host.clone(),
                root: root.trim_end_matches('/').to_string(),
                paths: paths.clone(),
                subscribers: 1,
            },
        );
        (key, Some(paths))
    }

    /// Releases a subscriber by its key; returns the host and what to stop
    /// watching when it was the last. An unknown key is not a server's.
    pub fn unsubscribe(&self, key: &str) -> Option<(SshHost, Watched)> {
        let mut repos = self.0.lock().unwrap_or_else(|e| e.into_inner());
        let repo = repos.get_mut(key)?;
        repo.subscribers -= 1;
        if repo.subscribers > 0 {
            return None;
        }
        let repo = repos.remove(key)?;
        Some((repo.host, repo.paths))
    }

    pub fn holds(&self, key: &str) -> bool {
        let repos = self.0.lock().unwrap_or_else(|e| e.into_inner());
        repos.contains_key(key)
    }

    /// The repositories `paths` (changed on `host`) belong to, each with the
    /// kinds of change, in a stable order.
    pub fn changed(&self, host: &SshHost, paths: &[String]) -> Vec<(String, Vec<&'static str>)> {
        let repos = self.0.lock().unwrap_or_else(|e| e.into_inner());
        let mut out: Vec<(String, Vec<&'static str>)> = Vec::new();
        for (key, repo) in repos.iter() {
            if repo.host != *host {
                continue;
            }
            let mut found: Vec<&'static str> = Vec::new();
            for path in paths {
                let path = path.trim_end_matches('/');
                let watched = repo.paths.dirs.iter().chain(&repo.paths.files);
                if watched.map(|w| w.trim_end_matches('/')).any(|w| w == path) {
                    for kind in kinds(&repo.root, path) {
                        if !found.contains(kind) {
                            found.push(kind);
                        }
                    }
                }
            }
            if !found.is_empty() {
                found.sort_unstable();
                out.push((key.clone(), found));
            }
        }
        out.sort();
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host(name: &str) -> SshHost {
        SshHost::try_from(name.to_string()).unwrap()
    }

    fn dirs() -> Watched {
        Watched {
            dirs: ["/srv/r", "/srv/r/src", "/srv/r/.git/refs/heads"]
                .map(String::from)
                .to_vec(),
            files: [
                "/srv/r/.git/HEAD",
                "/srv/r/.git/index",
                "/srv/r/.git/refs/heads/main",
            ]
            .map(String::from)
            .to_vec(),
        }
    }

    #[test]
    fn a_repository_is_watched_once_and_released_by_its_last_subscriber() {
        let watches = RemoteGitWatches::default();
        let (key, first) = watches.subscribe(&host("a"), "/srv/r/", dirs);
        assert_eq!(key, "ssh:a:/srv/r");
        assert_eq!(first, Some(dirs()));
        let (again, second) = watches.subscribe(&host("a"), "/srv/r", || panic!("listed once"));
        assert_eq!((again.as_str(), second), (key.as_str(), None));
        assert!(watches.unsubscribe(&key).is_none(), "one subscriber left");
        let (released_host, released) = watches.unsubscribe(&key).unwrap();
        assert_eq!(released_host, host("a"));
        assert_eq!(released, dirs());
        assert!(!watches.holds(&key));
        assert!(watches.unsubscribe(&key).is_none());
        // The same path on another server is another repository.
        assert_ne!(key, super::key(&host("b"), "/srv/r"));
    }

    #[test]
    fn a_change_is_classified_by_the_directory_it_came_from() {
        let watches = RemoteGitWatches::default();
        let (key, _) = watches.subscribe(&host("a"), "/srv/r", dirs);
        let changed = |paths: &[&str]| {
            watches.changed(
                &host("a"),
                &paths.iter().map(|p| p.to_string()).collect::<Vec<_>>(),
            )
        };
        assert_eq!(changed(&["/srv/r/src"]), [(key.clone(), vec!["worktree"])]);
        assert_eq!(
            changed(&["/srv/r/.git/HEAD", "/srv/r/.git/index"]),
            [(key.clone(), vec!["head", "index"])]
        );
        assert_eq!(
            changed(&["/srv/r/.git/refs/heads/", "/srv/r/.git/refs/heads/main"]),
            [(key.clone(), vec!["refs"])]
        );
        // Not one of its directories, or another host: nothing.
        assert!(changed(&["/srv/other"]).is_empty());
        assert!(watches
            .changed(&host("b"), &["/srv/r/src".to_string()])
            .is_empty());
    }
}
