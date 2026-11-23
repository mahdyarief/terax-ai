//! The registry with the real agent served in-process over pipes, and a
//! switch that cuts the stream as a dropped network would.

use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use terax_remote_protocol::client::{Options, RemoteWorkspace};
use terax_remote_protocol::transport::Log;
use terax_workspace_core::env::{SshHost, WorkspaceEnv};
use terax_workspace_core::host::HostWorkspace;
use terax_workspace_core::workspace::{EntryKind, Workspace};

use super::machine::{Failure, Stage, State};
use super::registry::{Connector, Link, Registry, Snapshot};

type Gate = Arc<Mutex<Option<io::PipeWriter>>>;

#[derive(Default)]
struct InProcess {
    /// Scripted failures, one per attempt; an empty script connects.
    failures: Mutex<VecDeque<Option<(String, Failure)>>>,
    /// Whether each attempt was interactive.
    calls: Mutex<Vec<bool>>,
    gate: Mutex<Option<Gate>>,
}

impl InProcess {
    fn script(failures: Vec<Option<(String, Failure)>>) -> Arc<Self> {
        Arc::new(Self {
            failures: Mutex::new(failures.into()),
            ..Self::default()
        })
    }

    /// Cuts the live stream: the client sees it end.
    fn cut(&self) {
        let gate = self.gate.lock().unwrap().take().expect("a live link");
        gate.lock().unwrap().take();
    }

    fn calls(&self) -> Vec<bool> {
        self.calls.lock().unwrap().clone()
    }
}

impl Connector for InProcess {
    fn connect(
        &self,
        host: &SshHost,
        interactive: bool,
        _log: &Arc<Log>,
        on_stage: &dyn Fn(Stage),
    ) -> Result<Link, (String, Failure)> {
        self.calls.lock().unwrap().push(interactive);
        on_stage(Stage::Handshaking);
        if let Some(Some(failure)) = self.failures.lock().unwrap().pop_front() {
            return Err(failure);
        }
        let (client_read, relay_write) = io::pipe().unwrap();
        let (agent_read, client_write) = io::pipe().unwrap();
        let (mut relay_read, agent_write) = io::pipe().unwrap();
        let gate: Gate = Arc::new(Mutex::new(Some(relay_write)));
        let relay_gate = Arc::clone(&gate);
        thread::spawn(move || {
            let mut buffer = [0u8; 64 * 1024];
            while let Ok(n) = relay_read.read(&mut buffer) {
                if n == 0 {
                    break;
                }
                let mut gate = relay_gate.lock().unwrap();
                let written = gate
                    .as_mut()
                    .is_some_and(|writer| writer.write_all(&buffer[..n]).is_ok());
                if !written {
                    break;
                }
            }
            relay_gate.lock().unwrap().take();
        });
        thread::spawn(move || {
            let ws: Arc<dyn Workspace> = Arc::new(HostWorkspace::new(WorkspaceEnv::Local));
            let _ = terax_agent::serve::serve(
                agent_read,
                agent_write,
                ws,
                terax_agent::serve::Options {
                    version: "test".into(),
                    workers: 2,
                },
            );
        });
        let workspace = RemoteWorkspace::connect(
            client_read,
            client_write,
            WorkspaceEnv::Ssh { host: host.clone() },
            Vec::new(),
            Options::default(),
        )
        .map_err(|error| (error.to_string(), Failure::Transient))?;
        *self.gate.lock().unwrap() = Some(gate);
        Ok(Link {
            workspace,
            process: None,
        })
    }
}

struct Fixture {
    registry: Registry,
    connector: Arc<InProcess>,
    seen: Arc<Mutex<Vec<Snapshot>>>,
    host: SshHost,
}

fn fixture(host: &str, connector: Arc<InProcess>) -> Fixture {
    let seen: Arc<Mutex<Vec<Snapshot>>> = Arc::default();
    let sink = Arc::clone(&seen);
    let registry = Registry::new(connector.clone(), move |snapshot| {
        sink.lock().unwrap().push(snapshot)
    });
    Fixture {
        registry,
        connector,
        seen,
        host: SshHost::try_from(host).unwrap(),
    }
}

impl Fixture {
    fn wait(&self, what: &str, done: impl Fn(&State) -> bool) -> State {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            let state = self.registry.state(&self.host);
            if done(&state) {
                return state;
            }
            assert!(
                Instant::now() < deadline,
                "waiting for {what}; now {state:?}\n{:#?}",
                self.registry.log(&self.host)
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn ready(&self) {
        self.wait("ready", |s| s == &State::Ready);
    }

    fn states_seen(&self) -> Vec<String> {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .map(|s| {
                serde_json::to_value(&s.state).unwrap()["state"]
                    .as_str()
                    .unwrap()
                    .to_string()
            })
            .collect()
    }
}

fn dir() -> (tempfile::TempDir, String) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().to_string_lossy().replace('\\', "/");
    (dir, path)
}

#[test]
fn a_connection_comes_up_with_no_roots_and_takes_them() {
    let f = fixture("in-process-a", InProcess::script(vec![]));
    let (_dir, path) = dir();
    f.registry.connect(&f.host);
    f.ready();
    assert_eq!(f.states_seen(), ["connecting", "handshaking", "ready"]);
    let workspace = f.registry.workspace(&f.host).unwrap();
    assert!(workspace.metadata(&path).is_err(), "no roots yet");
    f.registry.set_roots(&f.host, vec![path.clone()]).unwrap();
    assert!(workspace.metadata(&path).unwrap().kind == EntryKind::Dir);
    assert_eq!(f.connector.calls(), [true]);
}

#[test]
fn after_a_disconnect_the_backend_is_not_connected() {
    let f = fixture("in-process-b", InProcess::script(vec![]));
    let (_dir, path) = dir();
    f.registry.connect(&f.host);
    f.ready();
    f.registry.disconnect(&f.host);
    f.wait("idle", |s| s == &State::Idle);
    assert!(f.registry.workspace(&f.host).is_none());
    // Roots set while down are kept for the next connection.
    f.registry.set_roots(&f.host, vec![path.clone()]).unwrap();

    // `backend_for` answers from the app-wide registry.
    let global = fixture("in-process-global", InProcess::script(vec![]));
    let host = global.host.clone();
    super::registry::install(global.registry);
    let registry = super::registry::global().unwrap();
    let env = WorkspaceEnv::Ssh { host: host.clone() };
    registry.connect(&host);
    let deadline = Instant::now() + Duration::from_secs(15);
    while registry.state(&host) != State::Ready {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(10));
    }
    registry.set_roots(&host, vec![path.clone()]).unwrap();
    let backend = crate::modules::workspace::backend_for(&env);
    assert!(backend.metadata(&path).unwrap().kind == EntryKind::Dir);
    registry.disconnect(&host);
    while registry.state(&host) != State::Idle {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(10));
    }
    let error = crate::modules::workspace::backend_for(&env)
        .metadata(&path)
        .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::NotConnected);
}

#[test]
fn a_drop_reconnects_on_its_own_without_prompting_and_keeps_the_roots() {
    let f = fixture("in-process-c", InProcess::script(vec![]));
    let (_dir, path) = dir();
    f.registry.connect(&f.host);
    f.ready();
    f.registry.set_roots(&f.host, vec![path.clone()]).unwrap();

    f.connector.cut();
    f.wait("reconnecting", |s| matches!(s, State::Reconnecting { .. }));
    assert!(
        f.registry.workspace(&f.host).is_none(),
        "no stale workspace while down"
    );
    f.ready();
    assert_eq!(
        f.connector.calls(),
        [true, false],
        "the reconnect does not prompt"
    );
    let workspace = f.registry.workspace(&f.host).unwrap();
    assert!(
        workspace.metadata(&path).unwrap().kind == EntryKind::Dir,
        "roots put back"
    );
}

#[test]
fn a_first_connect_that_fails_shows_why() {
    let refused = Some((
        "me@box: Permission denied (publickey).".to_string(),
        Failure::Permanent,
    ));
    let f = fixture("in-process-d", InProcess::script(vec![refused]));
    f.registry.connect(&f.host);
    let state = f.wait("failed", |s| matches!(s, State::Failed { .. }));
    assert_eq!(
        state,
        State::Failed {
            reason: "me@box: Permission denied (publickey).".into()
        }
    );
    assert!(f
        .registry
        .log(&f.host)
        .iter()
        .any(|line| line.contains("attempt failed: me@box: Permission denied")));
    assert!(f.registry.workspace(&f.host).is_none());
}

#[test]
fn a_refused_reconnect_stops_instead_of_retrying() {
    let refused = Some((
        "Permission denied (publickey).".to_string(),
        Failure::Permanent,
    ));
    let f = fixture("in-process-e", InProcess::script(vec![None, refused]));
    f.registry.connect(&f.host);
    f.ready();
    f.connector.cut();
    f.wait("failed", |s| matches!(s, State::Failed { .. }));
    thread::sleep(Duration::from_secs(3));
    assert_eq!(
        f.connector.calls(),
        [true, false],
        "no attempt after the refusal"
    );
}

#[test]
fn a_drop_is_explained_only_by_what_ssh_said_since_connecting() {
    use super::registry::drop_reason;
    let lines = |l: &[&str]| l.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert_eq!(
        drop_reason(&lines(&[
            "stderr: Warning: Permanently added 'h' (ED25519) to the list of known hosts.",
            "state: connected",
        ])),
        "the connection ended"
    );
    assert_eq!(
        drop_reason(&lines(&[
            "stderr: old news",
            "state: connected",
            "stderr: Connection reset by 203.0.113.7 port 22",
        ])),
        "Connection reset by 203.0.113.7 port 22"
    );
    assert_eq!(drop_reason(&[]), "the connection ended");
}
