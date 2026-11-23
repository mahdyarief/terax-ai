//! The full stack the app will use, minus SSH: `RemoteWorkspace` talking to
//! the built `terax-agent` binary through a child process's stdio.

use std::io;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use terax_remote_protocol::client::{Options, RemoteWorkspace};
use terax_workspace_core::confined::{Confine, ConfinedError, ConfinedRead};
use terax_workspace_core::contract;
use terax_workspace_core::env::WorkspaceEnv;
use terax_workspace_core::exec::capture::Limits;
use terax_workspace_core::exec::ExecSpec;
use terax_workspace_core::host::HostWorkspace;
use terax_workspace_core::workspace::Workspace;

struct Session {
    child: Child,
    ws: Arc<RemoteWorkspace>,
    _dir: tempfile::TempDir,
    root: String,
    base: String,
}

/// An agent confined to `<tmp>/root`, with `<tmp>/outside` beside it.
fn start() -> Session {
    let dir = tempfile::tempdir().unwrap();
    let base = HostWorkspace::new(WorkspaceEnv::Local)
        .canonicalize(&dir.path().to_string_lossy())
        .unwrap();
    let root = format!("{base}/root");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(format!("{base}/outside")).unwrap();
    std::fs::write(format!("{base}/outside/secret.txt"), "secret").unwrap();

    let mut child = Command::new(env!("CARGO_BIN_EXE_terax-agent"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("start terax-agent");
    let ws = RemoteWorkspace::connect(
        child.stdout.take().unwrap(),
        child.stdin.take().unwrap(),
        WorkspaceEnv::Local,
        vec![root.clone()],
        Options {
            version: "test".into(),
            ..Options::default()
        },
    )
    .expect("connect to the agent");
    Session {
        child,
        ws: Arc::new(ws),
        _dir: dir,
        root,
        base,
    }
}

/// The child's exit status, or a panic if it has not exited by `within`.
fn exits_within(child: &mut Child, within: Duration) -> std::process::ExitStatus {
    let until = Instant::now() + within;
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return status;
        }
        assert!(Instant::now() < until, "the agent is still running");
        thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn the_agent_meets_the_workspace_contract_through_the_client() {
    let mut session = start();
    assert_eq!(session.ws.session().peer.program, "terax-agent");
    contract::run(session.ws.as_ref(), &session.root);
    drop(session.ws);
    assert!(exits_within(&mut session.child, Duration::from_secs(10)).success());
}

#[test]
fn the_roots_hold_through_the_whole_stack() {
    let mut session = start();
    let secret = format!("{}/outside/secret.txt", session.base);
    let error = session.ws.metadata(&secret).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    assert!(error.to_string().contains("outside"), "{error}");
    assert_eq!(
        session
            .ws
            .write_atomic(&format!("{}/outside/new.txt", session.base), b"x")
            .unwrap_err()
            .kind(),
        io::ErrorKind::PermissionDenied
    );
    // A confined read asking for a root the session does not have.
    let request = ConfinedRead {
        roots: vec![Confine {
            path: format!("{}/outside", session.base),
            subtree: true,
        }],
        secret_policy: false,
        offset: 0,
        limit: 64,
    };
    assert!(matches!(
        session.ws.read_confined(&secret, &request),
        Err(ConfinedError::Outside)
    ));
    assert_eq!(std::fs::read(&secret).unwrap(), b"secret");
    session.child.kill().unwrap();
    let _ = session.child.wait();
}

fn slow_command(cwd: &str) -> ExecSpec {
    #[cfg(windows)]
    let (program, args) = ("ping", vec!["-n", "6", "127.0.0.1"]);
    #[cfg(unix)]
    let (program, args) = ("sleep", vec!["5"]);
    ExecSpec {
        program: program.into(),
        args: args.into_iter().map(Into::into).collect(),
        cwd: Some(cwd.into()),
        env: Vec::new(),
        env_remove: Vec::new(),
        stdin: None,
        timeout: Duration::from_secs(30),
        limits: Limits {
            stdout: 4096,
            stderr: 4096,
            stop_on_stdout_cap: false,
        },
    }
}

#[test]
fn killing_the_agent_mid_call_fails_the_call_promptly() {
    let mut session = start();
    let call = {
        let (ws, root) = (session.ws.clone(), session.root.clone());
        thread::spawn(move || {
            let started = Instant::now();
            (ws.exec(slow_command(&root)), started)
        })
    };
    thread::sleep(Duration::from_millis(300));
    session.child.kill().unwrap();
    let killed = Instant::now();
    let (result, _) = call.join().unwrap();
    assert_eq!(result.unwrap_err().kind(), io::ErrorKind::ConnectionAborted);
    assert!(
        killed.elapsed() < Duration::from_secs(3),
        "failed after {:?}",
        killed.elapsed()
    );
    assert_eq!(
        session.ws.metadata(&session.root).unwrap_err().kind(),
        io::ErrorKind::ConnectionAborted
    );
    let _ = session.child.wait();
}

#[test]
fn the_agent_exits_when_the_client_goes_away() {
    let mut session = start();
    session.ws.ping(Duration::from_secs(5)).unwrap();
    drop(session.ws);
    assert!(exits_within(&mut session.child, Duration::from_secs(10)).success());
}

#[test]
fn the_binary_reports_its_version() {
    let out = Command::new(env!("CARGO_BIN_EXE_terax-agent"))
        .arg("--version")
        .output()
        .unwrap();
    assert!(out.status.success());
    let text = String::from_utf8(out.stdout).unwrap();
    assert_eq!(
        text.trim(),
        format!("terax-agent {}", env!("CARGO_PKG_VERSION"))
    );
    let bad = Command::new(env!("CARGO_BIN_EXE_terax-agent"))
        .arg("--nope")
        .output()
        .unwrap();
    assert_eq!(bad.status.code(), Some(2));
}

/// The app reinstalls an agent whose version differs from its own, so a
/// release that bumps the app must bump the agent with it.
#[test]
fn the_agent_carries_the_apps_version() {
    let manifest = include_str!("../../../Cargo.toml");
    let app = manifest
        .split("[package]")
        .nth(1)
        .and_then(|package| package.lines().find_map(|line| line.strip_prefix("version = ")))
        .map(|value| value.trim().trim_matches('"'))
        .expect("the app's [package] version");
    assert_eq!(env!("CARGO_PKG_VERSION"), app, "bump crates/terax-agent/Cargo.toml with the app");
}
