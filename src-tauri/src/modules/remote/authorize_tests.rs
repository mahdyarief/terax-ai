//! Authorizing a folder on a server: resolved there, covered per env, and
//! the host's agent confined to it, before and after a reconnect.

use std::io;
use std::thread;
use std::time::{Duration, Instant};

use terax_workspace_core::env::{SshHost, WorkspaceEnv};
use terax_workspace_core::workspace::EntryKind;

use super::machine::State;
use super::testing::{connected, global_connector, global_registry};
use crate::modules::workspace::{backend_for, WorkspaceRegistry};

struct Tree {
    _dir: tempfile::TempDir,
    base: String,
}

/// `base/repo/src/main.rs` and `base/other/secret.txt`.
fn tree() -> Tree {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().to_string_lossy().replace('\\', "/");
    std::fs::create_dir_all(format!("{base}/repo/src")).unwrap();
    std::fs::create_dir_all(format!("{base}/other")).unwrap();
    std::fs::write(format!("{base}/repo/src/main.rs"), "fn main() {}\n").unwrap();
    std::fs::write(format!("{base}/other/secret.txt"), "no").unwrap();
    Tree { _dir: dir, base }
}

fn ssh(host: &SshHost) -> WorkspaceEnv {
    WorkspaceEnv::Ssh { host: host.clone() }
}

fn wait_for(host: &SshHost, what: &str, done: impl Fn(&State) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while !done(&global_registry().state(host)) {
        assert!(Instant::now() < deadline, "waiting for {what}");
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn a_remote_folder_is_resolved_on_the_server_and_confines_the_agent() {
    let host = connected("auth-a");
    let env = ssh(&host);
    let tree = tree();
    let registry = WorkspaceRegistry::default();
    let backend = backend_for(&env);
    assert!(
        backend.read_dir(&format!("{}/repo", tree.base)).is_err(),
        "nothing yet"
    );

    let root = registry
        .authorize_in(&env, &format!("{}/repo/src/..", tree.base))
        .unwrap();
    // Canonical, as the server spells it.
    let canonical_base = backend_for(&env)
        .canonicalize(&root)
        .unwrap()
        .trim_end_matches("/repo")
        .to_string();
    assert!(root.ends_with("/repo"), "{root}");

    // The app's own check, per env.
    assert!(registry.covers(&env, &format!("{root}/src/main.rs")));
    assert!(!registry.covers(&env, &format!("{canonical_base}/other")));
    assert!(!registry.covers(&WorkspaceEnv::Local, &root), "another env");
    let elsewhere = ssh(&SshHost::try_from("auth-other-host").unwrap());
    assert!(!registry.covers(&elsewhere, &root), "another host");

    // And the agent's, behind it.
    let backend = backend_for(&env);
    assert_eq!(
        backend
            .metadata(&format!("{root}/src/main.rs"))
            .unwrap()
            .kind,
        EntryKind::File
    );
    assert!(backend
        .read_dir(&format!("{canonical_base}/other"))
        .is_err());
}

#[test]
fn a_second_root_adds_to_the_first() {
    let host = connected("auth-b");
    let env = ssh(&host);
    let tree = tree();
    let registry = WorkspaceRegistry::default();
    let repo = registry
        .authorize_in(&env, &format!("{}/repo", tree.base))
        .unwrap();
    let other = registry
        .authorize_in(&env, &format!("{}/other", tree.base))
        .unwrap();
    let backend = backend_for(&env);
    assert!(backend.read_dir(&repo).is_ok());
    assert!(backend.read_dir(&other).is_ok(), "both roots on the agent");
}

#[test]
fn the_roots_come_back_after_a_reconnect() {
    let host = connected("auth-c");
    let env = ssh(&host);
    let tree = tree();
    let registry = WorkspaceRegistry::default();
    let root = registry
        .authorize_in(&env, &format!("{}/repo", tree.base))
        .unwrap();

    global_connector().cut(&host);
    wait_for(&host, "reconnecting", |s| {
        matches!(s, State::Reconnecting { .. })
    });
    wait_for(&host, "ready", |s| s == &State::Ready);
    assert!(
        backend_for(&env).read_dir(&root).is_ok(),
        "the agent has it again"
    );
}

#[test]
fn only_a_directory_can_become_a_root_and_only_while_connected() {
    let host = connected("auth-d");
    let env = ssh(&host);
    let tree = tree();
    let registry = WorkspaceRegistry::default();
    let file = registry
        .authorize_in(&env, &format!("{}/repo/src/main.rs", tree.base))
        .unwrap_err();
    assert_eq!(file.kind(), io::ErrorKind::NotADirectory);
    assert!(!registry.covers(&env, &format!("{}/repo/src/main.rs", tree.base)));

    let never = ssh(&SshHost::try_from("auth-never-connected").unwrap());
    let error = registry
        .authorize_in(&never, &format!("{}/repo", tree.base))
        .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::NotConnected);
}
