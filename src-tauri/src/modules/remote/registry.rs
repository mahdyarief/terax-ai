//! The app's SSH connections, one per host, shared by every space on it.
//!
//! Each host has a driver thread that owns its `Machine`, starts attempts,
//! holds the live link, beats the heartbeat and notices a drop. Callers see
//! the state and, while it is `Ready`, the `RemoteWorkspace`; anything else
//! gives `NotConnected`, never a fallback to this machine.

use std::collections::HashMap;
use std::io;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;
use terax_remote_protocol::client::RemoteWorkspace;
use terax_remote_protocol::transport::{Log, Process};
use terax_workspace_core::env::SshHost;

use super::machine::{Action, Event, Failure, Machine, Stage, State, HEARTBEAT_EVERY};

/// How long a heartbeat waits for its answer.
const HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(10);
/// How often a live connection is checked for an ended stream or process.
const LIVENESS_POLL: Duration = Duration::from_secs(1);
const LOG_LINES: usize = 500;

/// A live connection: the workspace, and the process carrying it (none in
/// tests, where the agent runs in-process).
pub struct Link {
    pub workspace: RemoteWorkspace,
    pub process: Option<Process>,
}

/// Makes the connection for an attempt; the real one is `SshConnector`.
pub trait Connector: Send + Sync + 'static {
    /// `interactive`: the user started it and may be asked for passwords.
    fn connect(
        &self,
        host: &SshHost,
        interactive: bool,
        log: &Arc<Log>,
        on_stage: &dyn Fn(Stage),
    ) -> Result<Link, (String, Failure)>;
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Snapshot {
    pub host: String,
    #[serde(flatten)]
    pub state: State,
}

type Notify = Arc<dyn Fn(Snapshot) + Send + Sync>;

struct Shared {
    state: Mutex<State>,
    workspace: Mutex<Option<Arc<RemoteWorkspace>>>,
    /// The roots to keep on the agent, with a generation that changes on
    /// every update, so a connection made meanwhile catches up.
    roots: Mutex<(u64, Vec<String>)>,
    log: Arc<Log>,
}

struct Connection {
    tx: Sender<Msg>,
    shared: Arc<Shared>,
}

enum Msg {
    Event(Event),
    Linked {
        attempt: u64,
        link: Box<Link>,
        roots: u64,
    },
    Heartbeat(io::Result<()>),
}

pub struct Registry {
    connector: Arc<dyn Connector>,
    notify: Notify,
    connections: Mutex<HashMap<String, Connection>>,
}

impl Registry {
    pub fn new(
        connector: Arc<dyn Connector>,
        notify: impl Fn(Snapshot) + Send + Sync + 'static,
    ) -> Self {
        Self {
            connector,
            notify: Arc::new(notify),
            connections: Mutex::new(HashMap::new()),
        }
    }

    fn send(&self, host: &SshHost, event: Event) {
        let mut connections = self.connections.lock().unwrap_or_else(|p| p.into_inner());
        let connection = connections
            .entry(host.as_str().to_string())
            .or_insert_with(|| {
                let shared = Arc::new(Shared {
                    state: Mutex::new(State::Idle),
                    workspace: Mutex::new(None),
                    roots: Mutex::new((0, Vec::new())),
                    log: Arc::new(Log::new(LOG_LINES)),
                });
                let (tx, rx) = mpsc::channel();
                let driver = Driver {
                    host: host.clone(),
                    connector: Arc::clone(&self.connector),
                    notify: Arc::clone(&self.notify),
                    shared: Arc::clone(&shared),
                    tx: tx.clone(),
                    machine: Machine::default(),
                    process: None,
                    next_heartbeat: Instant::now(),
                    beating: false,
                };
                thread::Builder::new()
                    .name(format!("terax-remote-{host}"))
                    .spawn(move || driver.run(rx))
                    .expect("spawn the connection driver");
                Connection { tx, shared }
            });
        let _ = connection.tx.send(Msg::Event(event));
    }

    /// Connects, or reconnects now; the outcome arrives as state changes.
    pub fn connect(&self, host: &SshHost) {
        self.send(host, Event::Connect);
    }

    pub fn disconnect(&self, host: &SshHost) {
        if self.shared(host).is_some() {
            self.send(host, Event::Disconnect);
        }
    }

    fn shared(&self, host: &SshHost) -> Option<Arc<Shared>> {
        let connections = self.connections.lock().unwrap_or_else(|p| p.into_inner());
        connections
            .get(host.as_str())
            .map(|c| Arc::clone(&c.shared))
    }

    /// The live workspace, while the host is `Ready`.
    pub fn workspace(&self, host: &SshHost) -> Option<Arc<RemoteWorkspace>> {
        let shared = self.shared(host)?;
        let workspace = shared
            .workspace
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        workspace
    }

    pub fn state(&self, host: &SshHost) -> State {
        self.shared(host)
            .map(|shared| {
                shared
                    .state
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .clone()
            })
            .unwrap_or(State::Idle)
    }

    pub fn states(&self) -> Vec<Snapshot> {
        let connections = self.connections.lock().unwrap_or_else(|p| p.into_inner());
        let mut all: Vec<Snapshot> = connections
            .iter()
            .map(|(host, c)| Snapshot {
                host: host.clone(),
                state: c
                    .shared
                    .state
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .clone(),
            })
            .collect();
        all.sort_by(|a, b| a.host.cmp(&b.host));
        all
    }

    pub fn log(&self, host: &SshHost) -> Vec<String> {
        self.shared(host)
            .map(|shared| shared.log.lines())
            .unwrap_or_default()
    }

    /// Sets the roots the agent allows, now if connected and again after
    /// every reconnect.
    pub fn set_roots(&self, host: &SshHost, roots: Vec<String>) -> io::Result<()> {
        let Some(shared) = self.shared(host) else {
            return Err(not_connected(host));
        };
        {
            let mut kept = shared.roots.lock().unwrap_or_else(|p| p.into_inner());
            kept.0 += 1;
            kept.1 = roots.clone();
        }
        match self.workspace(host) {
            Some(workspace) => workspace.set_roots(roots),
            None => Ok(()),
        }
    }
}

pub fn not_connected(host: &SshHost) -> io::Error {
    io::Error::new(
        io::ErrorKind::NotConnected,
        format!("not connected to {host}"),
    )
}

struct Driver {
    host: SshHost,
    connector: Arc<dyn Connector>,
    notify: Notify,
    shared: Arc<Shared>,
    tx: Sender<Msg>,
    machine: Machine,
    /// The process of the live connection; dropping it ends the session.
    process: Option<Process>,
    next_heartbeat: Instant,
    beating: bool,
}

impl Driver {
    fn run(mut self, rx: Receiver<Msg>) {
        loop {
            let now = Instant::now();
            let mut wake = now + Duration::from_secs(3600);
            if let Some(at) = self.machine.deadline() {
                wake = wake.min(at);
            }
            if self.machine.state() == &State::Ready {
                wake = wake.min(now + LIVENESS_POLL).min(self.next_heartbeat);
            }
            let msg = match rx.recv_timeout(wake.saturating_duration_since(now)) {
                Ok(msg) => Some(msg),
                Err(RecvTimeoutError::Timeout) => None,
                // The registry is gone (the app is quitting).
                Err(RecvTimeoutError::Disconnected) => return,
            };
            match msg {
                Some(Msg::Event(event)) => self.apply(event),
                Some(Msg::Linked {
                    attempt,
                    link,
                    roots,
                }) => self.linked(attempt, *link, roots),
                Some(Msg::Heartbeat(result)) => {
                    self.beating = false;
                    match result {
                        Ok(()) => self.apply(Event::HeartbeatOk),
                        Err(error) if error.kind() == io::ErrorKind::TimedOut => {
                            self.apply(Event::HeartbeatMissed)
                        }
                        Err(error) => self.apply(Event::Dropped {
                            reason: error.to_string(),
                        }),
                    }
                }
                None => {
                    self.apply(Event::Tick);
                    self.check_link();
                }
            }
        }
    }

    fn apply(&mut self, event: Event) {
        let before = self.machine.state().clone();
        let actions = self.machine.apply(event, Instant::now());
        for action in actions {
            match action {
                Action::Start {
                    attempt,
                    interactive,
                } => self.start(attempt, interactive),
                Action::Close => self.close(),
            }
        }
        let after = self.machine.state().clone();
        if after != before {
            self.publish(after);
        }
    }

    fn publish(&self, state: State) {
        self.shared.log.push(format!("state: {}", describe(&state)));
        *self.shared.state.lock().unwrap_or_else(|p| p.into_inner()) = state.clone();
        (self.notify)(Snapshot {
            host: self.host.as_str().to_string(),
            state,
        });
    }

    fn start(&mut self, attempt: u64, interactive: bool) {
        let (host, connector, shared, tx) = (
            self.host.clone(),
            Arc::clone(&self.connector),
            Arc::clone(&self.shared),
            self.tx.clone(),
        );
        thread::spawn(move || {
            let stage_tx = tx.clone();
            let on_stage = move |stage| {
                let _ = stage_tx.send(Msg::Event(Event::Stage { attempt, stage }));
            };
            let result = connector.connect(&host, interactive, &shared.log, &on_stage);
            let msg = match result.and_then(|link| {
                // The roots go on before anyone can use the connection.
                let (generation, roots) = shared
                    .roots
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .clone();
                link.workspace.set_roots(roots).map_err(|error| {
                    (
                        format!("could not set the roots: {error}"),
                        Failure::Transient,
                    )
                })?;
                Ok((link, generation))
            }) {
                Ok((link, roots)) => Msg::Linked {
                    attempt,
                    link: Box::new(link),
                    roots,
                },
                Err((reason, failure)) => {
                    shared.log.push(format!("attempt failed: {reason}"));
                    Msg::Event(Event::AttemptFailed {
                        attempt,
                        reason,
                        failure,
                    })
                }
            };
            let _ = tx.send(msg);
        });
    }

    fn linked(&mut self, attempt: u64, link: Link, roots: u64) {
        let Link { workspace, process } = link;
        let workspace = Arc::new(workspace);
        let (generation, current) = self
            .shared
            .roots
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        if generation != roots {
            if let Err(error) = workspace.set_roots(current) {
                self.shared
                    .log
                    .push(format!("could not update the roots: {error}"));
            }
        }
        // In place before `Ready` is announced, so whoever reacts to it can
        // use the connection; taken back if the attempt no longer counts.
        let previous = self
            .shared
            .workspace
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .replace(workspace);
        self.apply(Event::Connected { attempt });
        if self.machine.state() == &State::Ready && previous.is_none() {
            self.process = process;
            self.next_heartbeat = Instant::now() + HEARTBEAT_EVERY;
        } else {
            *self
                .shared
                .workspace
                .lock()
                .unwrap_or_else(|p| p.into_inner()) = previous;
        }
    }

    fn close(&mut self) {
        *self
            .shared
            .workspace
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = None;
        self.process = None;
    }

    /// While `Ready`: a drop shows as an ended stream or process; otherwise
    /// a heartbeat is due every `HEARTBEAT_EVERY`.
    fn check_link(&mut self) {
        if self.machine.state() != &State::Ready {
            return;
        }
        let workspace = self
            .shared
            .workspace
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        let Some(workspace) = workspace else { return };
        if workspace.is_closed() {
            let reason = drop_reason(&self.shared.log.lines());
            self.apply(Event::Dropped { reason });
            return;
        }
        let now = Instant::now();
        if now >= self.next_heartbeat && !self.beating {
            self.beating = true;
            self.next_heartbeat = now + HEARTBEAT_EVERY;
            let tx = self.tx.clone();
            thread::spawn(move || {
                let result = workspace.ping(HEARTBEAT_TIMEOUT).map(|_| ());
                let _ = tx.send(Msg::Heartbeat(result));
            });
        }
    }
}

/// Why the live session ended: ssh's last word since it connected, if it
/// said anything. Lines from before (an earlier session's warnings) are not
/// this session's reason.
pub fn drop_reason(lines: &[String]) -> String {
    let connected = format!("state: {}", describe(&State::Ready));
    lines
        .iter()
        .rev()
        .take_while(|line| **line != connected)
        .find_map(|line| line.strip_prefix("stderr: ").map(str::to_string))
        .unwrap_or_else(|| "the connection ended".into())
}

fn describe(state: &State) -> String {
    match state {
        State::Idle => "disconnected".into(),
        State::Connecting => "connecting".into(),
        State::Installing { bytes } => format!("installing the agent ({bytes} bytes)"),
        State::Handshaking => "starting the agent".into(),
        State::Ready => "connected".into(),
        State::Reconnecting { reason, .. } => format!("reconnecting: {reason}"),
        State::Failed { reason } => format!("failed: {reason}"),
    }
}

static GLOBAL: OnceLock<Registry> = OnceLock::new();

/// Installs the app's registry, once, at startup.
pub fn install(registry: Registry) {
    let _ = GLOBAL.set(registry);
}

pub fn global() -> Option<&'static Registry> {
    GLOBAL.get()
}
