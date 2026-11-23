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
