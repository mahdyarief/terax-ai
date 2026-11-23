//! Test helpers: a connector that serves the real agent in-process over
//! pipes (with a switch that cuts the stream as a dropped network would),
//! and the app-wide registry built on it.

use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;

use terax_remote_protocol::client::{Options, RemoteWorkspace};
use terax_remote_protocol::transport::Log;
use terax_workspace_core::env::{SshHost, WorkspaceEnv};
use terax_workspace_core::host::HostWorkspace;
use terax_workspace_core::workspace::Workspace;

use super::machine::{Failure, Stage};
use super::registry::{self, Connector, Link, Registry};

type Gate = Arc<Mutex<Option<io::PipeWriter>>>;

#[derive(Default)]
pub struct InProcess {
    /// Scripted failures, one per attempt; an empty script connects.
    failures: Mutex<VecDeque<Option<(String, Failure)>>>,
    /// Whether each attempt was interactive.
    calls: Mutex<Vec<bool>>,
    /// Each host's live stream.
    gates: Mutex<std::collections::HashMap<String, Gate>>,
}

impl InProcess {
    pub fn script(failures: Vec<Option<(String, Failure)>>) -> Arc<Self> {
        Arc::new(Self {
            failures: Mutex::new(failures.into()),
            ..Self::default()
        })
    }

    /// Cuts the live stream: the client sees it end.
    pub fn cut(&self, host: &SshHost) {
        let gate = self.gates.lock().unwrap().remove(host.as_str()).expect("a live link");
        gate.lock().unwrap().take();
    }

    pub fn calls(&self) -> Vec<bool> {
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
                    home: None,
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
        self.gates.lock().unwrap().insert(host.as_str().to_string(), gate);
        Ok(Link {
            workspace,
            process: None,
        })
    }
}

/// The app-wide registry (`registry::global`, what `backend_for` asks),
/// installed once per test process on an in-process connector. Tests share
/// it, so each uses a host name of its own.
pub fn global_registry() -> &'static Registry {
    global_connector();
    registry::global().expect("installed")
}

/// The app-wide registry's connector, to cut a host's stream.
pub fn global_connector() -> &'static Arc<InProcess> {
    static CONNECTOR: OnceLock<Arc<InProcess>> = OnceLock::new();
    CONNECTOR.get_or_init(|| {
        let connector = InProcess::script(Vec::new());
        registry::install(Registry::new(connector.clone(), |_| {}));
        connector
    })
}

/// Connects `host` on the app-wide registry and waits until it is ready.
pub fn connected(host: &str) -> SshHost {
    let host = SshHost::try_from(host).unwrap();
    let registry = global_registry();
    registry.connect(&host);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while registry.state(&host) != super::machine::State::Ready {
        assert!(
            std::time::Instant::now() < deadline,
            "{host} never connected"
        );
        thread::sleep(std::time::Duration::from_millis(10));
    }
    host
}
