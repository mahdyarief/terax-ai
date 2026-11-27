//! Which terminal tabs exist, as each window publishes them.
//!
//! Windows own their tabs: the registry only mirrors what they report, keyed
//! by window label, and a remote viewer reads the merged snapshot. Nothing
//! here touches a PTY or a window.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use tokio::sync::watch;

/// The workspace a tab's terminal runs in, as the frontend spells it.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteWorkspace {
    /// `local`, `wsl:<distro>` or `ssh:<host>`.
    pub key: String,
    /// The name a person recognizes ("WSL: Ubuntu", "SSH: box").
    pub label: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemotePane {
    pub leaf_id: u32,
    /// Absent until the pane has a live PTY.
    pub pty_id: Option<u32>,
    pub active: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteAgent {
    /// The agent's name when known ("claude", "codex", ...).
    pub kind: Option<String>,
    /// `working`, `attention`, `finished` or `idle`.
    pub state: String,
}

/// One tab, terminal or not. A window never sends itself as the owner; the
/// label comes from the calling webview.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteTab {
    pub id: u32,
    pub title: String,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub space_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<RemoteWorkspace>,
    /// `terminal` or `blocks`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub panes: Vec<RemotePane>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<RemoteAgent>,
}

impl RemoteTab {
    fn is_terminal(&self) -> bool {
        self.kind == "terminal"
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteTabEntry {
    /// The window that owns and hosts the tab.
    pub window: String,
    #[serde(flatten)]
    pub tab: RemoteTab,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TabSnapshot {
    pub version: u64,
    pub tabs: Vec<RemoteTabEntry>,
}

struct Inner {
    /// Label -> tabs, in the order the window reported them.
    windows: HashMap<String, Vec<RemoteTab>>,
    version: u64,
}

/// The merged view every remote viewer sees.
pub struct TabRegistry {
    inner: Mutex<Inner>,
    updates: watch::Sender<Arc<TabSnapshot>>,
    /// Keeps the channel open with no viewers: a send with no receiver would
    /// fail and the snapshot would go stale.
    _updates_keepalive: watch::Receiver<Arc<TabSnapshot>>,
    /// Bumped whenever the set of live PTYs changes, so streams can validate
    /// an id without racing a publish.
    pty_updates: watch::Sender<u64>,
    _pty_keepalive: watch::Receiver<u64>,
}

impl Default for TabRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl TabRegistry {
    pub fn new() -> Self {
        let (updates, updates_keepalive) = watch::channel(Arc::new(TabSnapshot::default()));
        let (pty_updates, pty_keepalive) = watch::channel(0u64);
        Self {
            inner: Mutex::new(Inner {
                windows: HashMap::new(),
                version: 0,
            }),
            updates,
            _updates_keepalive: updates_keepalive,
            pty_updates,
            _pty_keepalive: pty_keepalive,
        }
    }

    pub fn subscribe(&self) -> watch::Receiver<Arc<TabSnapshot>> {
        self.updates.subscribe()
    }

    pub fn snapshot(&self) -> Arc<TabSnapshot> {
        self.updates.borrow().clone()
    }

    /// Replaces one window's tabs. An unchanged report is a no-op, so a
    /// frontend republishing on every render never wakes a viewer.
    pub fn publish(&self, window: &str, tabs: Vec<RemoteTab>) -> Arc<TabSnapshot> {
        let mut inner = self.inner.lock().unwrap();
        if inner.windows.get(window) == Some(&tabs) {
            return self.updates.borrow().clone();
        }
        inner.windows.insert(window.to_string(), tabs);
        let snapshot = Self::snapshot_of(&mut inner);
        let _ = self.updates.send(snapshot.clone());
        snapshot
    }

    /// The window is gone (closed, or its page navigated away): its tabs are
    /// no longer live.
    pub fn remove_window(&self, window: &str) -> Arc<TabSnapshot> {
        let mut inner = self.inner.lock().unwrap();
        if inner.windows.remove(window).is_none() {
            return self.updates.borrow().clone();
        }
        let snapshot = Self::snapshot_of(&mut inner);
        let _ = self.updates.send(snapshot.clone());
        snapshot
    }

    /// Every PTY a viewer may open a stream for. Ids outside this set are
    /// not listed to anybody.
    pub fn pty_ids(&self) -> HashSet<u32> {
        self.inner
            .lock()
            .unwrap()
            .windows
            .values()
            .flat_map(|tabs| tabs.iter())
            .filter(|tab| tab.is_terminal())
            .flat_map(|tab| tab.panes.iter())
            .filter_map(|pane| pane.pty_id)
            .collect()
    }

    /// The window whose space hosts a tab action.
    pub fn window_for_space(&self, space_id: &str) -> Option<String> {
        let inner = self.inner.lock().unwrap();
        inner
            .windows
            .iter()
            .find(|(_, tabs)| {
                tabs.iter()
                    .any(|tab| tab.space_id.as_deref() == Some(space_id))
            })
            .map(|(window, _)| window.clone())
    }

    /// The window and tab for a close request.
    pub fn tab(&self, tab_id: u32) -> Option<(String, RemoteTab)> {
        let inner = self.inner.lock().unwrap();
        inner.windows.iter().find_map(|(window, tabs)| {
            tabs.iter()
                .find(|tab| tab.id == tab_id)
                .map(|tab| (window.clone(), tab.clone()))
        })
    }

    /// The pty was published or dropped with its tab.
    pub fn note_pty_change(&self) {
        self.pty_updates.send_modify(|version| *version += 1);
    }

    pub fn subscribe_pty_ids(&self) -> watch::Receiver<u64> {
        self.pty_updates.subscribe()
    }

    fn snapshot_of(inner: &mut Inner) -> Arc<TabSnapshot> {
        inner.version += 1;
        let mut tabs: Vec<RemoteTabEntry> = inner
            .windows
            .iter()
            .flat_map(|(window, tabs)| {
                tabs.iter().map(move |tab| RemoteTabEntry {
                    window: window.clone(),
                    tab: tab.clone(),
                })
            })
            .collect();
        tabs.sort_by(|a, b| a.window.cmp(&b.window).then(a.tab.id.cmp(&b.tab.id)));
        Arc::new(TabSnapshot {
            version: inner.version,
            tabs,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tab(id: u32, space: &str, pty: Option<u32>) -> RemoteTab {
        RemoteTab {
            id,
            title: format!("shell {id}"),
            kind: "terminal".into(),
            space_id: Some(space.into()),
            workspace: Some(RemoteWorkspace {
                key: "local".into(),
                label: "Local".into(),
            }),
            mode: Some("terminal".into()),
            panes: vec![RemotePane {
                leaf_id: id * 10,
                pty_id: pty,
                active: true,
            }],
            agent: None,
        }
    }

    #[test]
    fn two_windows_merge_into_one_snapshot() {
        let registry = TabRegistry::new();
        registry.publish("main", vec![tab(1, "a", Some(11))]);
        let merged = registry.publish("space-b", vec![tab(2, "b", Some(12))]);
        assert_eq!(merged.tabs.len(), 2);
        assert_eq!(merged.version, 2);
        assert_eq!(
            merged
                .tabs
                .iter()
                .map(|entry| entry.window.as_str())
                .collect::<Vec<_>>(),
            ["main", "space-b"]
        );
        assert_eq!(registry.pty_ids(), HashSet::from([11, 12]));
        assert_eq!(registry.window_for_space("b").as_deref(), Some("space-b"));
        assert_eq!(
            registry.tab(2).map(|(window, _)| window).as_deref(),
            Some("space-b")
        );
    }

    #[test]
    fn a_closed_window_takes_its_tabs_and_ptys_with_it() {
        let registry = TabRegistry::new();
        registry.publish("main", vec![tab(1, "a", Some(11))]);
        registry.publish("space-b", vec![tab(2, "b", Some(12))]);
        let after = registry.remove_window("space-b");
        assert_eq!(after.tabs.len(), 1);
        assert_eq!(after.version, 3);
        assert_eq!(registry.pty_ids(), HashSet::from([11]));
        assert!(registry.window_for_space("b").is_none());
        // Removing it again changes nothing.
        assert_eq!(registry.remove_window("space-b").version, 3);
    }

    #[test]
    fn republishing_identical_tabs_does_not_wake_viewers() {
        let registry = TabRegistry::new();
        let mut updates = registry.subscribe();
        let mut published = false;
        let tabs = vec![tab(1, "a", Some(11))];
        let first = registry.publish("main", tabs.clone());
        assert!(updates.has_changed().unwrap());
        updates.borrow_and_update();
        let again = registry.publish("main", tabs);
        published |= again.version != first.version;
        assert!(!published, "an unchanged report bumps nothing");
        assert!(!updates.has_changed().unwrap());
        // A real change does.
        registry.publish("main", vec![tab(1, "a", Some(13))]);
        assert!(updates.has_changed().unwrap());
    }
}
