//! A manual harness, not run by default: the workspace contract against an
//! agent on a real server, with `ssh` as the transport, before the app has a
//! connection of its own.
//!
//! ```text
//! TERAX_AGENT_SSH_HOST=myserver \
//! TERAX_AGENT_SSH_PATH=/home/me/.cache/terax/terax-agent \
//! cargo test -p terax-agent --test over_ssh -- --ignored --nocapture
//! ```
//!
//! The agent must already be at that path. A scratch directory is made with
//! `mktemp -d` on the server and removed afterwards.

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use terax_remote_protocol::client::{Options, RemoteWorkspace};
use terax_workspace_core::contract;
use terax_workspace_core::env::WorkspaceEnv;
use terax_workspace_core::exec::capture::Limits;
use terax_workspace_core::exec::ExecSpec;
use terax_workspace_core::workspace::Workspace;

fn ssh(host: &str) -> Command {
    let mut cmd = Command::new("ssh");
    cmd.args(["-T", "-o", "BatchMode=yes", "-o", "ConnectTimeout=15", host]);
    cmd
}

fn remote(host: &str, script: &str) -> String {
    let out = ssh(host).arg(script).output().expect("run ssh");
    assert!(
        out.status.success(),
        "`{script}` failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

#[test]
#[ignore = "needs TERAX_AGENT_SSH_HOST and an agent installed on it"]
fn the_agent_meets_the_contract_over_ssh() {
    let (Ok(host), Ok(agent)) = (
        std::env::var("TERAX_AGENT_SSH_HOST"),
        std::env::var("TERAX_AGENT_SSH_PATH"),
    ) else {
        panic!("set TERAX_AGENT_SSH_HOST and TERAX_AGENT_SSH_PATH");
    };
    println!("server: {}", remote(&host, "uname -sm"));
    println!("agent:  {}", remote(&host, &format!("{agent} --version")));
    let scratch = remote(&host, "mktemp -d");

    let started = Instant::now();
    let mut child = ssh(&host)
        .arg(&agent)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("start ssh");
    let ws = RemoteWorkspace::connect(
        child.stdout.take().unwrap(),
        child.stdin.take().unwrap(),
        WorkspaceEnv::Local,
        vec![scratch.clone()],
        Options {
            version: "over-ssh".into(),
            ..Options::default()
        },
    )
    .expect("connect through ssh");
    println!(
        "connected in {:?}: {:?}",
        started.elapsed(),
        ws.session().peer
    );
    let rtt: Vec<Duration> = (0..5)
        .map(|_| ws.ping(Duration::from_secs(10)).unwrap())
        .collect();
    println!("ping: {rtt:?}");

    let started = Instant::now();
    contract::run(&ws, &scratch);
    println!("contract passed in {:?}", started.elapsed());

    // git on the server, started in the session's root.
    let git = ws
        .exec(ExecSpec {
            program: "git".into(),
            args: vec!["init".into(), "-q".into()],
            cwd: None,
            env: Vec::new(),
            env_remove: Vec::new(),
            stdin: None,
            timeout: Duration::from_secs(30),
            limits: Limits {
                stdout: 4096,
                stderr: 4096,
                stop_on_stdout_cap: false,
            },
        })
        .unwrap();
    assert_eq!(
        git.exit_code,
        Some(0),
        "{}",
        String::from_utf8_lossy(&git.stderr)
    );
    assert!(
        ws.metadata(&format!("{scratch}/.git/HEAD")).is_ok(),
        "git ran in the root"
    );
    let outside = ws.metadata("/etc/passwd").unwrap_err();
    assert_eq!(outside.kind(), std::io::ErrorKind::PermissionDenied);

    drop(ws);
    let status = child.wait().unwrap();
    assert!(status.success(), "ssh and the agent end cleanly: {status}");
    remote(&host, &format!("rm -rf -- '{scratch}'"));
}

/// The app's own transport against a real server: `ssh` from `PATH`, the
/// bootstrap, an upload of the release agent, then a reconnect that reuses
/// it. Installs under a throwaway version, removed before and after.
///
/// ```text
/// TERAX_AGENT_SSH_HOST=myserver \
/// cargo test -p terax-agent --test over_ssh transport -- --ignored --nocapture
/// ```
///
/// The agent is `src-tauri/binaries/terax-agent-x86_64-linux-musl` (from
/// `pnpm build:agent`), or `TERAX_AGENT_LINUX_BIN`.
#[test]
#[ignore = "needs TERAX_AGENT_SSH_HOST and a built Linux agent"]
fn the_transport_installs_and_reuses_the_agent_over_ssh() {
    use sha2::{Digest, Sha256};
    use terax_remote_protocol::transport::{
        connect, openssh_version, AgentFile, ConnectOptions, Log, Stage, Transport,
    };
    use terax_workspace_core::env::SshHost;

    const TEST_VERSION: &str = "0.0.0-transport-test";
    let host = std::env::var("TERAX_AGENT_SSH_HOST").expect("set TERAX_AGENT_SSH_HOST");
    let path = std::env::var_os("TERAX_AGENT_LINUX_BIN")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../binaries/terax-agent-x86_64-linux-musl")
        });
    let bytes = std::fs::read(&path).expect("build the agent with `pnpm build:agent`");
    let agent = AgentFile {
        path,
        sha256: Sha256::digest(&bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
    };
    let cleanup = format!("rm -rf ~/.terax-server/{TEST_VERSION}");
    remote(&host, &cleanup);

    println!("ssh: {:?}", openssh_version(&"ssh".into()).unwrap());
    let ssh_host = SshHost::try_from(host.as_str()).unwrap();
    let transport = Transport::ssh("ssh", &ssh_host);
    let env = WorkspaceEnv::Ssh { host: ssh_host };
    let options = || ConnectOptions {
        client: Options {
            version: TEST_VERSION.into(),
            ..Options::default()
        },
        bootstrap_timeout: Duration::from_secs(120),
    };

    for expect_install in [true, false] {
        let log = std::sync::Arc::new(Log::new(500));
        let stages = std::sync::Mutex::new(Vec::new());
        let started = Instant::now();
        let connected = connect(
            &transport,
            env.clone(),
            &|platform| (platform == "x86_64-linux-musl").then(|| agent.clone()),
            &log,
            &|stage| stages.lock().unwrap().push(stage),
            options(),
        )
        .unwrap_or_else(|e| panic!("{e}\n{:#?}", log.lines()));
        let took = started.elapsed();
        let ping = connected.workspace.ping(Duration::from_secs(10)).unwrap();
        println!(
            "connect (install: {}): {took:?}, ping {ping:?}, stages {:?}",
            connected.installed,
            stages.lock().unwrap()
        );
        assert_eq!(connected.installed, expect_install);
        assert_eq!(
            stages
                .lock()
                .unwrap()
                .contains(&Stage::Installing(bytes.len() as u64)),
            expect_install
        );
        for line in log.lines() {
            println!("  log: {line}");
        }
    }
    let installed = remote(
        &host,
        &format!("sha256sum ~/.terax-server/{TEST_VERSION}/terax-agent | cut -d' ' -f1; stat -c %a ~/.terax-server/{TEST_VERSION}/terax-agent"),
    );
    assert_eq!(installed, format!("{}\n700", agent.sha256));
    remote(&host, &cleanup);
}

/// A terminal on a real server, through the app's transport: the login
/// shell with the app's integration, keystrokes, a resize, a flood and an
/// exit code. Installs under a throwaway version, removed before and after.
///
/// ```text
/// TERAX_AGENT_SSH_HOST=myserver \
/// cargo test -p terax-agent --test over_ssh terminal -- --ignored --nocapture
/// ```
#[test]
#[ignore = "needs TERAX_AGENT_SSH_HOST and a built Linux agent"]
fn a_terminal_runs_on_the_server_over_ssh() {
    use sha2::{Digest, Sha256};
    use std::sync::mpsc::Receiver;
    use terax_remote_protocol::client::PtyEvent;
    use terax_remote_protocol::message::{PtyId, PtySpec};
    use terax_remote_protocol::transport::{connect, AgentFile, ConnectOptions, Log, Transport};
    use terax_workspace_core::env::SshHost;

    const TEST_VERSION: &str = "0.0.0-terminal-test";
    let host = std::env::var("TERAX_AGENT_SSH_HOST").expect("set TERAX_AGENT_SSH_HOST");
    let path = std::env::var_os("TERAX_AGENT_LINUX_BIN")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../binaries/terax-agent-x86_64-linux-musl")
        });
    let bytes = std::fs::read(&path).expect("build the agent with `pnpm build:agent`");
    let agent = AgentFile {
        path,
        sha256: Sha256::digest(&bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
    };
    let cleanup = format!("rm -rf ~/.terax-server/{TEST_VERSION}");
    remote(&host, &cleanup);
    let scratch = remote(&host, "mktemp -d");

    let ssh_host = SshHost::try_from(host.as_str()).unwrap();
    let log = std::sync::Arc::new(Log::new(500));
    let connected = connect(
        &Transport::ssh("ssh", &ssh_host),
        WorkspaceEnv::Ssh { host: ssh_host },
        &|platform| (platform == "x86_64-linux-musl").then(|| agent.clone()),
        &log,
        &|_| {},
        ConnectOptions {
            client: Options {
                version: TEST_VERSION.into(),
                ..Options::default()
            },
            bootstrap_timeout: Duration::from_secs(120),
        },
    )
    .unwrap_or_else(|e| panic!("{e}\n{:#?}", log.lines()));
    let ws = &connected.workspace;
    ws.set_roots(vec![scratch.clone()]).unwrap();

    let read_until = |id: PtyId, events: &Receiver<PtyEvent>, done: &dyn Fn(&str) -> bool| {
        let until = Instant::now() + Duration::from_secs(60);
        let mut out = Vec::new();
        loop {
            match events.recv_timeout(until.saturating_duration_since(Instant::now())) {
                Ok(PtyEvent::Data(bytes)) => {
                    ws.pty_credit(id, bytes.len() as u64).unwrap();
                    out.extend_from_slice(&bytes);
                    if done(&String::from_utf8_lossy(&out)) {
                        return (String::from_utf8_lossy(&out).into_owned(), None);
                    }
                }
                Ok(PtyEvent::Exit { code, reason }) => {
                    return (
                        String::from_utf8_lossy(&out).into_owned(),
                        Some((code, reason)),
                    )
                }
                Err(error) => panic!("{error}; so far: {}", String::from_utf8_lossy(&out)),
            }
        }
    };

    let started = Instant::now();
    let (id, program, events) = ws
        .pty_open(PtySpec {
            cwd: Some(scratch.clone()),
            cols: 80,
            rows: 24,
            command: None,
            env: Vec::new(),
            blocks: true,
        })
        .unwrap();
    println!("shell: {program}, opened in {:?}", started.elapsed());

    ws.pty_write(id, b"pwd\r").unwrap();
    let (out, exit) = read_until(id, &events, &|out| out.matches(&scratch).count() >= 2);
    assert!(exit.is_none(), "{out:?}");
    let integrated = ["bash", "zsh", "fish"].iter().any(|s| program.ends_with(s));
    if integrated {
        // Prompt marks and the working directory, from the app's scripts.
        assert!(out.contains("\x1b]133;"), "no prompt marks: {out:?}");
        assert!(out.contains("\x1b]7;"), "no OSC 7: {out:?}");
    }

    ws.pty_resize(id, 133, 41).unwrap();
    ws.pty_write(id, b"stty size\r").unwrap();
    let (out, _) = read_until(id, &events, &|out| out.contains("41 133"));
    assert!(out.contains("41 133"));

    let started = Instant::now();
    ws.pty_write(id, b"seq 1 400000; echo flood-$((6*7))\r")
        .unwrap();
    let probe = std::thread::scope(|scope| {
        let probe = scope.spawn(|| {
            std::thread::sleep(Duration::from_millis(300));
            let started = Instant::now();
            ws.metadata(&scratch).unwrap();
            started.elapsed()
        });
        let (out, _) = read_until(id, &events, &|out| out.contains("flood-42"));
        assert!(out.contains("400000"));
        probe.join().unwrap()
    });
    println!(
        "flood of ~2.7 MB in {:?}; a stat beside it took {probe:?}",
        started.elapsed()
    );
    assert!(probe < Duration::from_secs(3), "{probe:?}");

    ws.pty_write(id, b"exit 4\r").unwrap();
    let (_, exit) = read_until(id, &events, &|_| false);
    assert_eq!(exit, Some((Some(4), None)));

    drop(connected);
    remote(&host, &format!("rm -rf -- '{scratch}'; {cleanup}"));
}
