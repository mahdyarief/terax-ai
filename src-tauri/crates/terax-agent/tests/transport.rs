//! The transport end to end, with `sh` in place of `ssh`: the real bootstrap
//! command, the real agent uploaded into a temporary `HOME`, and the
//! protocol over the same pipes. Unix only.
//!
//! The agent is this crate's binary; `TERAX_AGENT_BIN` points elsewhere
//! (a cross-built test runs on a machine where cargo's path does not exist).
#![cfg(unix)]

use std::ffi::OsString;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};
use terax_remote_protocol::bootstrap::INSTALL_DIR;
use terax_remote_protocol::client::Options;
use terax_remote_protocol::transport::{
    connect, AgentFile, ConnectError, ConnectOptions, Connected, Log, Stage, Transport,
};
use terax_workspace_core::env::{SshHost, WorkspaceEnv};
use terax_workspace_core::workspace::Workspace;

const VERSION: &str = "0.9.4-dy";

/// Hashed once: the debug agent is large and an unoptimized hash is slow.
fn agent() -> AgentFile {
    static AGENT: OnceLock<AgentFile> = OnceLock::new();
    AGENT
        .get_or_init(|| {
            let path = std::env::var_os("TERAX_AGENT_BIN")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_terax-agent")));
            let bytes = std::fs::read(&path).unwrap();
            let sha256 = Sha256::digest(&bytes)
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect();
            AgentFile { path, sha256 }
        })
        .clone()
}

fn env() -> WorkspaceEnv {
    WorkspaceEnv::Ssh {
        host: SshHost::try_from("test-host").unwrap(),
    }
}

/// `sh -c <prefix>`, which runs the bootstrap command (its `$1`) with
/// `eval` after whatever the prefix does first.
fn shell(home: &Path, prefix: &str) -> Transport {
    Transport {
        program: "sh".into(),
        args: vec![
            "-c".into(),
            format!("{prefix} eval \"$1\"").into(),
            "sh".into(),
        ],
        env: vec![("HOME".into(), home.as_os_str().to_owned())],
    }
}

fn options(timeout: Duration) -> ConnectOptions {
    ConnectOptions {
        client: Options {
            version: VERSION.into(),
            ..Options::default()
        },
        bootstrap_timeout: timeout,
    }
}

struct Run {
    result: Result<Connected, ConnectError>,
    stages: Vec<Stage>,
    log: Arc<Log>,
}

fn run(transport: &Transport, bundled: Option<AgentFile>, timeout: Duration) -> Run {
    let log = Arc::new(Log::new(500));
    let stages = Mutex::new(Vec::new());
    let result = connect(
        transport,
        env(),
        &|_platform: &str| bundled.clone(),
        &log,
        &|stage| stages.lock().unwrap().push(stage),
        options(timeout),
    );
    Run {
        result,
        stages: stages.into_inner().unwrap(),
        log,
    }
}

fn installed_agent(home: &Path) -> PathBuf {
    home.join(INSTALL_DIR).join(VERSION).join("terax-agent")
}

#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "the agent runs on Linux servers and refuses macOS"
)]
fn a_first_connect_installs_and_a_second_reuses() {
    let home = tempfile::tempdir().unwrap();
    let agent = agent();
    let size = std::fs::metadata(&agent.path).unwrap().len();

    let first = run(
        &shell(home.path(), ""),
        Some(agent.clone()),
        Duration::from_secs(60),
    );
    let connected = first
        .result
        .unwrap_or_else(|e| panic!("{e}: {:?}", first.log.lines()));
    assert!(connected.installed);
    assert_eq!(connected.report.os, "Linux");
    assert_eq!(
        first.stages,
        [
            Stage::Connecting,
            Stage::Installing(size),
            Stage::Handshaking
        ]
    );
    assert!(connected.workspace.ping(Duration::from_secs(5)).is_ok());
    // No roots yet: the agent refuses every path.
    assert!(connected
        .workspace
        .metadata(home.path().to_str().unwrap())
        .is_err());
    assert_eq!(
        std::fs::read(installed_agent(home.path())).unwrap(),
        std::fs::read(&agent.path).unwrap()
    );
    drop(connected);

    let second = run(
        &shell(home.path(), ""),
        Some(agent),
        Duration::from_secs(60),
    );
    let connected = second.result.unwrap();
    assert!(!connected.installed);
    assert_eq!(second.stages, [Stage::Connecting, Stage::Handshaking]);
    assert!(connected.workspace.ping(Duration::from_secs(5)).is_ok());
}

#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "the agent runs on Linux servers and refuses macOS"
)]
fn shell_noise_is_skipped_and_stderr_reaches_the_log() {
    let home = tempfile::tempdir().unwrap();
    let noisy = shell(home.path(), "echo 'Welcome!'; echo 'from stderr' >&2;");
    let run = run(&noisy, Some(agent()), Duration::from_secs(60));
    let connected = run.result.unwrap();
    assert!(connected.workspace.ping(Duration::from_secs(5)).is_ok());
    drop(connected);
    let lines = run.log.lines();
    assert!(lines.contains(&"server: Welcome!".to_string()), "{lines:?}");
    // stderr is read on its own thread; give it a moment.
    let deadline = Instant::now() + Duration::from_secs(5);
    while !run.log.lines().contains(&"stderr: from stderr".to_string()) {
        assert!(Instant::now() < deadline, "{:?}", run.log.lines());
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "the agent runs on Linux servers and refuses macOS"
)]
fn a_session_that_dies_mid_install_fails_cleanly() {
    let home = tempfile::tempdir().unwrap();
    let shim = tempfile::tempdir().unwrap();
    // A `head` that reads a little of the upload, then kills the bootstrap,
    // as a dropped network would.
    let head = shim.path().join("head");
    std::fs::write(
        &head,
        "#!/bin/sh\n[ \"$2\" = 0 ] && exit 0\ndd bs=1 count=100 of=/dev/null 2>/dev/null\necho 'Connection reset by peer' >&2\nkill -9 $PPID\n",
    )
    .unwrap();
    std::fs::set_permissions(&head, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut transport = shell(home.path(), "");
    let path = format!(
        "{}:{}",
        shim.path().display(),
        std::env::var("PATH").unwrap()
    );
    transport.env.push(("PATH".into(), OsString::from(path)));

    let run = run(&transport, Some(agent()), Duration::from_secs(60));
    match run.result {
        // The last line is whatever the server's shells said about the
        // killed bootstrap ("Killed" from sh); what came before is kept.
        Err(ConnectError::Ended { stderr }) => {
            assert!(stderr.is_some());
            let lines = run.log.lines();
            assert!(
                lines.contains(&"stderr: Connection reset by peer".to_string()),
                "{lines:?}"
            );
        }
        Err(other) => panic!("{other:?}"),
        Ok(_) => panic!("connected"),
    }
    assert!(!installed_agent(home.path()).exists());
}

#[test]
fn a_refused_login_reports_sshs_message() {
    let home = tempfile::tempdir().unwrap();
    let refused = shell(
        home.path(),
        "echo 'me@box: Permission denied (publickey).' >&2; exit 255;",
    );
    match run(&refused, Some(agent()), Duration::from_secs(60)).result {
        Err(ConnectError::Ended { stderr }) => {
            assert_eq!(
                stderr.as_deref(),
                Some("me@box: Permission denied (publickey).")
            )
        }
        other => panic!("{:?}", other.err()),
    }
}

#[test]
fn last_words_after_the_output_closes_are_kept() {
    let home = tempfile::tempdir().unwrap();
    let late = shell(
        home.path(),
        "exec >&-; sleep 0.5; echo 'Host key verification failed.' >&2; exit 255;",
    );
    match run(&late, Some(agent()), Duration::from_secs(60)).result {
        Err(ConnectError::Ended { stderr }) => {
            assert_eq!(stderr.as_deref(), Some("Host key verification failed."))
        }
        other => panic!("{:?}", other.err()),
    }
}

#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "the agent runs on Linux servers and refuses macOS"
)]
fn a_server_without_an_agent_for_it_is_refused_and_left_alone() {
    let home = tempfile::tempdir().unwrap();
    match run(&shell(home.path(), ""), None, Duration::from_secs(60)).result {
        Err(ConnectError::Unsupported(why)) => assert!(why.contains("carries no agent"), "{why}"),
        other => panic!("{:?}", other.err()),
    }
    assert!(!installed_agent(home.path()).exists());
}

#[test]
fn output_that_is_not_the_bootstrap_is_an_error() {
    let home = tempfile::tempdir().unwrap();
    let wrong = shell(
        home.path(),
        "echo 'TERAX-BOOTSTRAP 1'; echo 'colour=blue'; echo END; exit 0;",
    );
    assert!(matches!(
        run(&wrong, Some(agent()), Duration::from_secs(60)).result,
        Err(ConnectError::Bootstrap(_))
    ));
}

#[test]
fn a_stalled_bootstrap_times_out() {
    let home = tempfile::tempdir().unwrap();
    let stalled = shell(home.path(), "exec sleep 30;");
    let agent = agent();
    let started = Instant::now();
    assert!(matches!(
        run(&stalled, Some(agent), Duration::from_millis(300)).result,
        Err(ConnectError::TimedOut)
    ));
    assert!(started.elapsed() < Duration::from_secs(10));
}

#[test]
fn a_missing_transport_is_a_spawn_error() {
    let missing = Transport {
        program: "/nonexistent/ssh".into(),
        args: Vec::new(),
        env: Vec::new(),
    };
    assert!(matches!(
        run(&missing, Some(agent()), Duration::from_secs(5)).result,
        Err(ConnectError::Spawn(_))
    ));
}
