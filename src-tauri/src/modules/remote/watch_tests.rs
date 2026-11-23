//! Watching a server's folder through the real agent served in-process,
//! across a dropped connection.

use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use terax_workspace_core::env::{SshHost, WorkspaceEnv};

use super::machine::State;
use super::testing::{connected, global_connector, global_registry};
use super::watch::{push, resume, RemoteWatches, WatchSet};
use crate::modules::workspace::WorkspaceRegistry;

fn wait_for(host: &SshHost, what: &str, done: impl Fn(&State) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while !done(&global_registry().state(host)) {
        assert!(Instant::now() < deadline, "waiting for {what}");
        thread::sleep(Duration::from_millis(10));
    }
}

fn next(rx: &mpsc::Receiver<(String, Vec<String>)>) -> Vec<String> {
    let (_, paths) = rx
        .recv_timeout(Duration::from_secs(10))
        .expect("a change is delivered");
    paths
}

#[test]
fn a_watched_server_folder_reports_changes_and_is_watched_again_after_a_reconnect() {
    let host = connected("watch-a");
    let env = WorkspaceEnv::Ssh { host: host.clone() };
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().to_string_lossy().replace('\\', "/");
    std::fs::create_dir_all(format!("{base}/repo/src")).unwrap();
    let root = WorkspaceRegistry::default()
        .authorize_in(&env, &format!("{base}/repo"))
        .unwrap();
    let src = format!("{root}/src");

    let (tx, rx) = mpsc::channel();
    let tx = std::sync::Mutex::new(tx);
    let deliver: super::watch::Deliver = Arc::new(move |host: &SshHost, paths| {
        let _ = tx.lock().unwrap().send((host.as_str().to_string(), paths));
    });
    let ws = global_registry().workspace(&host).unwrap();
    resume(&ws, &host, WatchSet::default(), deliver.clone()).unwrap();
    assert!(
        rx.try_recv().is_err(),
        "nothing watched, nothing to catch up"
    );

    let watches = RemoteWatches::default();
    let set = watches.add(&host, std::slice::from_ref(&src), &[]).unwrap();
    push(Some(ws), set).unwrap();
    std::fs::write(format!("{src}/a.rs"), "").unwrap();
    assert_eq!(next(&rx), std::slice::from_ref(&src));

    global_connector().cut(&host);
    wait_for(&host, "reconnecting", |s| {
        matches!(s, State::Reconnecting { .. })
    });
    wait_for(&host, "ready", |s| s == &State::Ready);
    // What the app does on `ready`.
    let ws = global_registry().workspace(&host).unwrap();
    resume(&ws, &host, watches.set(&host), deliver).unwrap();
    assert_eq!(
        next(&rx),
        std::slice::from_ref(&src),
        "one catch-up delivery"
    );
    std::fs::write(format!("{src}/b.rs"), "").unwrap();
    assert_eq!(
        next(&rx),
        std::slice::from_ref(&src),
        "watched on the new connection"
    );

    // Released: the agent stops watching.
    let set = watches
        .remove(&host, std::slice::from_ref(&src), &[])
        .unwrap();
    push(global_registry().workspace(&host), set).unwrap();
    std::fs::write(format!("{src}/c.rs"), "").unwrap();
    assert!(rx.recv_timeout(Duration::from_millis(1500)).is_err());
}
