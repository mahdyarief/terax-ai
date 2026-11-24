//! The AI's shell tools in a server's space, through the real agent served
//! in-process.

use std::time::Duration;

use terax_workspace_core::env::WorkspaceEnv;

use super::testing::connected;
use crate::modules::shell::session::ShellSession;
use crate::modules::shell::{authorize_server_cwd, run_command_in};
use crate::modules::workspace::WorkspaceRegistry;

struct Remote {
    _dir: tempfile::TempDir,
    base: String,
    root: String,
    env: WorkspaceEnv,
    registry: WorkspaceRegistry,
}

fn remote(name: &str) -> Remote {
    let env = WorkspaceEnv::Ssh {
        host: connected(name),
    };
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().to_string_lossy().replace('\\', "/");
    std::fs::create_dir_all(format!("{base}/root/src")).unwrap();
    std::fs::write(format!("{base}/root/src/lib.rs"), "fn a() {}\n").unwrap();
    let registry = WorkspaceRegistry::default();
    let root = registry
        .authorize_in(&env, &format!("{base}/root"))
        .unwrap();
    Remote {
        _dir: dir,
        base,
        root,
        env,
        registry,
    }
}

#[test]
fn the_ai_runs_a_command_on_the_server_in_its_folder() {
    let r = remote("shell-run");
    let out = run_command_in(
        &r.registry,
        "ls src; echo oops >&2; exit 3".into(),
        Some(r.root.clone()),
        r.env.clone(),
        Duration::from_secs(30),
    )
    .unwrap();
    assert_eq!(out.stdout.trim(), "lib.rs");
    assert_eq!(out.stderr.trim(), "oops");
    assert_eq!(out.exit_code, Some(3));
    assert!(!out.timed_out);

    let slow = run_command_in(
        &r.registry,
        "sleep 5".into(),
        Some(r.root.clone()),
        r.env.clone(),
        Duration::from_secs(1),
    )
    .unwrap();
    assert!(slow.timed_out);
}

#[test]
fn a_server_command_starts_only_in_an_authorized_folder() {
    let r = remote("shell-cwd");
    let run = |cwd: Option<String>| {
        run_command_in(
            &r.registry,
            "echo hi".into(),
            cwd,
            r.env.clone(),
            Duration::from_secs(30),
        )
        .err()
        .unwrap_or_default()
    };
    let outside = run(Some(r.base.clone()));
    assert!(outside.contains("outside"), "{outside}");
    assert!(run(Some(format!("{}/src/lib.rs", r.root))).contains("not a directory"));
    // A server has no default folder the user approved.
    assert!(run(None).contains("needs the folder"));
    assert_eq!(
        authorize_server_cwd(&r.registry, Some(&format!("{}/src/../src", r.root)), &r.env).unwrap(),
        format!("{}/src", r.root)
    );
}

#[test]
fn a_shell_session_on_the_server_keeps_its_folder() {
    let r = remote("shell-session");
    let session = ShellSession::new(r.root.clone(), r.env.clone());
    let first = session
        .run("cd src".into(), None, None, Duration::from_secs(30))
        .unwrap();
    assert_eq!(first.exit_code, Some(0));
    // The next command starts where `cd` left it: the folder `pwd` names,
    // checked on the server. MSYS's `sh`, which runs these tests on
    // Windows, names it its own way (`/c/...`), so only a Unix machine
    // can follow it; the test on a real server does too.
    #[cfg(unix)]
    {
        assert_eq!(first.cwd_after, format!("{}/src", r.root));
        let second = session
            .run("ls".into(), None, None, Duration::from_secs(30))
            .unwrap();
        assert_eq!(second.stdout.trim(), "lib.rs");
    }
    #[cfg(windows)]
    assert_eq!(first.cwd_after, r.root);
}
