//! End to end against a real server, not run by default: the app's registry
//! and `SshConnector` with the user's `ssh`, the agent bundled from
//! `target/`, and `terax-cli` as askpass answering a stand-in endpoint.
//!
//! ```text
//! cargo build -p terax-cli
//! pnpm build:agent        # or the x86_64 musl agent in target/
//! TERAX_AGENT_SSH_HOST=myserver \
//! cargo test -p terax --lib remote::e2e_tests -- --ignored --nocapture
//! ```
//!
//! Every login succeeds (the user's own key; the new host key is accepted),
//! so the server sees no failed attempt: fail2ban banned this machine once.
//! The test installs under `~/.terax-server/0.0.0-e2e-<pid>-*` only and
//! removes those directories at the end.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use terax_control_protocol::askpass::{classify, AskpassKind, AskpassParams, CANCELLED};
use terax_control_protocol::{ControlRequest, ControlResponse};
use terax_workspace_core::env::SshHost;
use terax_workspace_core::workspace::{EntryKind, Workspace};

use super::connector::SshConnector;
use super::machine::State;
use super::registry::Registry;
use crate::modules::control::ControlState;
use crate::modules::remote_agent::{dev_root, AgentBundle};

type Asked = Arc<Mutex<Vec<(AskpassKind, String)>>>;

/// Answers ssh's questions: a new host key is accepted, anything else is
/// cancelled (this server takes the user's key; no password is expected).
fn endpoint() -> (String, Asked) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap().to_string();
    let asked: Asked = Arc::default();
    let record = Arc::clone(&asked);
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut line = String::new();
            if BufReader::new(&mut stream).read_line(&mut line).is_err() {
                continue;
            }
            let Ok(request) = serde_json::from_str::<ControlRequest>(&line) else {
                continue;
            };
            let params: AskpassParams = serde_json::from_value(request.params).unwrap();
            let kind = classify(&params.prompt, params.hint.as_deref());
            record.lock().unwrap().push((kind, params.prompt.clone()));
            let response = match kind {
                AskpassKind::YesNo { host_key: true } => {
                    ControlResponse::success(request.id, serde_json::json!({ "answer": "yes" }))
                }
                _ => ControlResponse::failure(request.id, CANCELLED, "not expected"),
            };
            let mut bytes = serde_json::to_vec(&response).unwrap();
            bytes.push(b'\n');
            let _ = stream.write_all(&bytes);
        }
    });
    (address, asked)
}

fn cli() -> PathBuf {
    let name = if cfg!(windows) {
        "terax-cli.exe"
    } else {
        "terax-cli"
    };
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("debug")
        .join(name);
    assert!(path.exists(), "run `cargo build -p terax-cli` first");
    path
}

fn server(host: &str, script: &str) -> String {
    let output = Command::new("ssh")
        .args(["-o", "BatchMode=yes", host, script])
        .output()
        .unwrap();
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Removes this run's agent directories from the server.
struct Cleanup {
    host: String,
    prefix: String,
}

impl Drop for Cleanup {
    fn drop(&mut self) {
        server(
            &self.host,
            &format!(
                "rm -rf ~/.terax-server/{}-a ~/.terax-server/{}-b",
                self.prefix, self.prefix
            ),
        );
    }
}

fn registry(version: &str, control: &ControlState, known_hosts: &Path) -> Registry {
    let connector = SshConnector {
        ssh: "ssh".into(),
        version: version.into(),
        agents: AgentBundle::new(None, dev_root().map(PathBuf::from)),
        control: control.clone(),
        ssh_options: vec![
            format!("UserKnownHostsFile={}", known_hosts.display()),
            "StrictHostKeyChecking=ask".into(),
        ],
    };
    Registry::new(Arc::new(connector), |_| {})
}

fn wait(registry: &Registry, host: &SshHost, what: &str, done: impl Fn(&State) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        let state = registry.state(host);
        if done(&state) {
            return;
        }
        assert!(
            !matches!(state, State::Failed { .. }) && Instant::now() < deadline,
            "waiting for {what}; now {state:?}\n{}",
            registry.log(host).join("\n")
        );
        thread::sleep(Duration::from_millis(50));
    }
}

fn installs(registry: &Registry, host: &SshHost) -> usize {
    registry
        .log(host)
        .iter()
        .filter(|line| line.as_str() == "the agent was installed")
        .count()
}

#[test]
#[ignore = "needs TERAX_AGENT_SSH_HOST"]
fn connect_install_reconnect_and_upgrade_on_a_real_server() {
    let name = std::env::var("TERAX_AGENT_SSH_HOST").expect("set TERAX_AGENT_SSH_HOST");
    let host = SshHost::try_from(name.as_str()).unwrap();
    let prefix = format!("0.0.0-e2e-{}", std::process::id());
    let _cleanup = Cleanup {
        host: name.clone(),
        prefix: prefix.clone(),
    };
    let (address, asked) = endpoint();
    let control = ControlState::for_askpass_tests(&address, cli());
    let dir = tempfile::tempdir().unwrap();
    let known_hosts = dir.path().join("known_hosts");

    // A first connect: the new host key is asked through terax-cli, the
    // agent is installed.
    let a = registry(&format!("{prefix}-a"), &control, &known_hosts);
    let started = Instant::now();
    a.connect(&host);
    wait(&a, &host, "ready", |s| s == &State::Ready);
    println!("first connect with install: {:?}", started.elapsed());
    {
        let asked = asked.lock().unwrap();
        assert!(
            matches!(
                asked.first(),
                Some((AskpassKind::YesNo { host_key: true }, _))
            ),
            "the new host key is asked: {asked:?}"
        );
        assert_eq!(asked.len(), 1, "{asked:?}");
    }
    assert!(known_hosts.exists(), "the accepted key was recorded");
    assert_eq!(installs(&a, &host), 1);
    let workspace = a.workspace(&host).unwrap();
    assert!(workspace.metadata("/tmp").is_err(), "no roots yet");
    // The folder picker's calls work before any root.
    let home = workspace.home().unwrap();
    assert!(home.starts_with('/'), "{home}");
    let browse = workspace.browse_dirs(&home).unwrap();
    assert_eq!(browse.path, home);
    assert!(
        browse.dirs.iter().any(|d| d == ".terax-server"),
        "{:?}",
        browse.dirs
    );
    println!("home {home}: {} directories", browse.dirs.len());
    a.set_roots(&host, vec!["/tmp".into()]).unwrap();
    assert_eq!(workspace.metadata("/tmp").unwrap().kind, EntryKind::Dir);

    // Disconnect and connect again: the installed agent is reused, and the
    // now known host key is not asked again.
    a.disconnect(&host);
    wait(&a, &host, "idle", |s| s == &State::Idle);
    let started = Instant::now();
    a.connect(&host);
    wait(&a, &host, "ready again", |s| s == &State::Ready);
    println!("reconnect reusing the agent: {:?}", started.elapsed());
    assert_eq!(installs(&a, &host), 1, "reused");
    assert_eq!(asked.lock().unwrap().len(), 1, "no new question");

    // The session dies under it (the agent is killed on the server): the
    // registry notices and reconnects on its own, without prompting, and
    // puts the roots back.
    server(
        &name,
        &format!("pkill -f '[.]terax-server/{prefix}-a/terax-agent'"),
    );
    wait(&a, &host, "reconnecting", |s| {
        matches!(s, State::Reconnecting { .. })
    });
    let started = Instant::now();
    wait(&a, &host, "ready after the drop", |s| s == &State::Ready);
    println!("automatic reconnect after a drop: {:?}", started.elapsed());
    let reconnecting = a
        .log(&host)
        .into_iter()
        .find(|line| line.starts_with("state: reconnecting"))
        .unwrap();
    assert!(
        !reconnecting.contains("Permanently added"),
        "an earlier session's warning is not the reason: {reconnecting}"
    );
    let workspace = a.workspace(&host).unwrap();
    assert_eq!(
        workspace.metadata("/tmp").unwrap().kind,
        EntryKind::Dir,
        "roots put back"
    );
    assert_eq!(installs(&a, &host), 1);
    assert_eq!(asked.lock().unwrap().len(), 1);

    // Another version installs beside the first, which stays.
    let b = registry(&format!("{prefix}-b"), &control, &known_hosts);
    b.connect(&host);
    wait(&b, &host, "ready on b", |s| s == &State::Ready);
    assert_eq!(installs(&b, &host), 1);
    let listed = server(&name, "ls ~/.terax-server");
    assert!(listed.contains(&format!("{prefix}-a")), "{listed}");
    assert!(listed.contains(&format!("{prefix}-b")), "{listed}");
    assert!(
        b.log(&host)
            .iter()
            .any(|line| line.starts_with("ssh: OpenSSH")),
        "the ssh version is logged"
    );

    a.disconnect(&host);
    b.disconnect(&host);
    wait(&a, &host, "a idle", |s| s == &State::Idle);
    wait(&b, &host, "b idle", |s| s == &State::Idle);
    println!("log of a:\n{}", a.log(&host).join("\n"));
}
