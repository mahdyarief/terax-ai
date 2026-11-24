//! The AI's shell tools in a server's space, through the real agent served
//! in-process.

use std::time::Duration;

use terax_workspace_core::env::WorkspaceEnv;

use super::testing::connected;
use crate::modules::shell::session::ShellSession;
use crate::modules::shell::{authorize_server_cwd, run_command_in, spawn_background};
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

/// A pause in `sh` itself: MSYS's `sh`, which runs the agent's commands in
/// these tests on Windows, finds no `sleep` on its `PATH` there.
fn wait(secs: u32) -> String {
    if cfg!(windows) {
        format!("read -t {secs} x || true")
    } else {
        format!("sleep {secs}")
    }
}

/// Polls a background process's log until it has exited.
fn finished(proc: &crate::modules::shell::background::BackgroundProc) -> (String, Option<i32>) {
    let until = std::time::Instant::now() + Duration::from_secs(30);
    let mut text = String::new();
    let mut offset = 0;
    loop {
        let logs = proc.read_logs(offset);
        text.push_str(&logs.bytes);
        offset = logs.next_offset;
        if logs.exited {
            return (text, logs.exit_code);
        }
        assert!(std::time::Instant::now() < until, "still running: {text:?}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn a_background_process_on_the_server_streams_its_output() {
    let r = remote("shell-bg");
    let proc = spawn_background(
        &r.registry,
        format!(
            "echo started; {}; for f in src/*; do echo \"$f\"; done; exit 4",
            wait(1)
        ),
        Some(r.root.clone()),
        r.env.clone(),
    )
    .unwrap();
    // Output arrives while the process still runs.
    let until = std::time::Instant::now() + Duration::from_secs(30);
    while !proc.read_logs(0).bytes.contains("started") {
        assert!(std::time::Instant::now() < until);
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(!proc.read_logs(0).exited);
    let (text, code) = finished(&proc);
    assert!(text.contains("src/lib.rs"), "{text:?}");
    assert!(!text.contains("\r\n"), "{text:?}");
    assert_eq!(code, Some(4));

    let outside = spawn_background(
        &r.registry,
        "true".into(),
        Some(r.base.clone()),
        r.env.clone(),
    );
    assert!(outside.err().unwrap_or_default().contains("outside"));
}

#[test]
fn a_background_process_on_the_server_can_be_stopped() {
    let r = remote("shell-bg-kill");
    let proc = spawn_background(
        &r.registry,
        format!("echo up; {}", wait(60)),
        Some(r.root.clone()),
        r.env.clone(),
    )
    .unwrap();
    let until = std::time::Instant::now() + Duration::from_secs(30);
    while !proc.read_logs(0).bytes.contains("up") {
        assert!(std::time::Instant::now() < until);
        std::thread::sleep(Duration::from_millis(20));
    }
    let started = std::time::Instant::now();
    proc.kill();
    finished(&proc);
    assert!(started.elapsed() < Duration::from_secs(10));
}
