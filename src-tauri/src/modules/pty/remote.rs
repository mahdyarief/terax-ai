//! A terminal on a server: the agent's PTY (`terax-agent/src/pty.rs`) behind
//! portable-pty's traits, so the session pipeline (`session::run`: output
//! queue, replay, hub, handoff) is the one a local terminal has.
//!
//! The reader credits the agent for each chunk as it hands it on, so the
//! app's own backpressure (a reader blocked on a full output queue) reaches
//! the server. There is no local process: no pid, no job, no foreground
//! check. When the terminal ends for any reason but its own exit or a
//! close, the reader writes the reason into the terminal before it ends, so
//! a dropped connection says so where the user is looking.

use std::fmt;
use std::io::{self, Read, Write};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Condvar, Mutex};

use portable_pty::{Child, ChildKiller, ExitStatus, MasterPty, PtySize};
use terax_remote_protocol::client::{PtyEvent, RemoteWorkspace};
use terax_remote_protocol::message::{PtyId, PtySpec};
use terax_workspace_core::env::SshHost;

use super::session::{self, Host, Parts, Session};

/// What the agent says when the app closed the terminal itself.
const CLOSED: &str = "the terminal was closed";

/// The exit code of a server's terminal whose connection ended (no process
/// exits with it): the frontend keeps the pane and offers a new terminal
/// instead of closing it (`PTY_EXIT_CONNECTION_ENDED` in `pty-bridge.ts`).
pub const CONNECTION_ENDED_EXIT: i32 = -1001;

/// Opens a terminal on `server` in `cwd` (a server path the app has
/// authorized; `None` is the space's folder) and runs it like a local one.
pub(super) fn spawn(
    host: Host,
    size: PtySize,
    server: &SshHost,
    cwd: Option<String>,
    blocks: bool,
) -> Result<Arc<Session>, String> {
    let ws = crate::modules::remote::registry::global()
        .and_then(|registry| registry.workspace(server))
        .ok_or_else(|| crate::modules::remote::registry::not_connected(server).to_string())?;
    spawn_on(host, size, ws, server, cwd, blocks)
}

pub(super) fn spawn_on(
    host: Host,
    size: PtySize,
    ws: Arc<RemoteWorkspace>,
    server: &SshHost,
    cwd: Option<String>,
    blocks: bool,
) -> Result<Arc<Session>, String> {
    let (id, program, events) = ws
        .pty_open(PtySpec {
            cwd,
            cols: size.cols,
            rows: size.rows,
            command: None,
            env: Vec::new(),
            blocks,
        })
        .map_err(|e| e.to_string())?;
    let exit = Arc::new(Exit::default());
    let terminal = Terminal { ws: ws.clone(), id };
    session::run(
        host,
        size,
        Parts {
            master: Box::new(Master {
                terminal: terminal.clone(),
                size: Mutex::new(size),
            }),
            child: Box::new(RemoteChild {
                terminal: terminal.clone(),
                exit: exit.clone(),
            }),
            reader: Box::new(Reader {
                terminal: terminal.clone(),
                events,
                pending: Vec::new(),
                at: 0,
                exit,
                server: server.as_str().to_string(),
            }),
            writer: Box::new(Writer(terminal)),
            shell_program: program,
            native: None,
        },
    )
}

/// One terminal on one connection.
#[derive(Clone)]
struct Terminal {
    ws: Arc<RemoteWorkspace>,
    id: PtyId,
}

impl fmt::Debug for Terminal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "remote terminal {}", self.id)
    }
}

/// The exit code, once the reader has seen the terminal end.
#[derive(Default)]
struct Exit {
    code: Mutex<Option<u32>>,
    set: Condvar,
}

impl Exit {
    fn set(&self, code: u32) {
        self.code.lock().unwrap().get_or_insert(code);
        self.set.notify_all();
    }

    fn get(&self) -> Option<u32> {
        *self.code.lock().unwrap()
    }

    fn wait(&self) -> u32 {
        let mut code = self.code.lock().unwrap();
        loop {
            if let Some(code) = *code {
                return code;
            }
            code = self.set.wait(code).unwrap();
        }
    }
}

struct Reader {
    terminal: Terminal,
    events: Receiver<PtyEvent>,
    pending: Vec<u8>,
    at: usize,
    exit: Arc<Exit>,
    server: String,
}

impl Read for Reader {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        loop {
            if self.at < self.pending.len() {
                let n = buffer.len().min(self.pending.len() - self.at);
                buffer[..n].copy_from_slice(&self.pending[self.at..self.at + n]);
                self.at += n;
                return Ok(n);
            }
            if self.exit.get().is_some() {
                return Ok(0);
            }
            match self.events.recv() {
                Ok(PtyEvent::Data(bytes)) => {
                    let _ = self
                        .terminal
                        .ws
                        .pty_credit(self.terminal.id, bytes.len() as u64);
                    self.pending = bytes;
                    self.at = 0;
                }
                Ok(PtyEvent::Exit { code, reason }) => {
                    let (pending, code) = match reason.as_deref() {
                        None | Some(CLOSED) => (Vec::new(), code.unwrap_or(1)),
                        Some(_) => (
                            ended(&self.server).into_bytes(),
                            CONNECTION_ENDED_EXIT as u32,
                        ),
                    };
                    self.pending = pending;
                    self.at = 0;
                    self.exit.set(code);
                }
                // The client always sends an exit before it lets go.
                Err(_) => {
                    self.exit.set(CONNECTION_ENDED_EXIT as u32);
                    return Ok(0);
                }
            }
        }
    }
}

/// The line a terminal ends with when its server's connection did.
fn ended(server: &str) -> String {
    format!("\r\n\x1b[2m[the connection to {server} ended]\x1b[0m\r\n")
}

struct Writer(Terminal);

impl Write for Writer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.ws.pty_write(self.0.id, bytes)?;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct Master {
    terminal: Terminal,
    size: Mutex<PtySize>,
}

impl MasterPty for Master {
    fn resize(&self, size: PtySize) -> Result<(), anyhow::Error> {
        self.terminal
            .ws
            .pty_resize(self.terminal.id, size.cols, size.rows)?;
        *self.size.lock().unwrap() = size;
        Ok(())
    }

    fn get_size(&self) -> Result<PtySize, anyhow::Error> {
        Ok(*self.size.lock().unwrap())
    }

    fn try_clone_reader(&self) -> Result<Box<dyn Read + Send>, anyhow::Error> {
        Err(anyhow::anyhow!("a server's terminal has one reader"))
    }

    fn take_writer(&self) -> Result<Box<dyn Write + Send>, anyhow::Error> {
        Err(anyhow::anyhow!("a server's terminal has one writer"))
    }

    #[cfg(unix)]
    fn process_group_leader(&self) -> Option<libc::pid_t> {
        None
    }

    #[cfg(unix)]
    fn as_raw_fd(&self) -> Option<std::os::unix::io::RawFd> {
        None
    }

    #[cfg(unix)]
    fn tty_name(&self) -> Option<std::path::PathBuf> {
        None
    }
}

#[derive(Debug)]
struct Killer(Terminal);

impl ChildKiller for Killer {
    fn kill(&mut self) -> io::Result<()> {
        self.0.ws.pty_close(self.0.id)
    }

    fn clone_killer(&self) -> Box<dyn ChildKiller + Send + Sync> {
        Box::new(Killer(self.0.clone()))
    }
}

struct RemoteChild {
    terminal: Terminal,
    exit: Arc<Exit>,
}

impl fmt::Debug for RemoteChild {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.terminal.fmt(f)
    }
}

impl ChildKiller for RemoteChild {
    fn kill(&mut self) -> io::Result<()> {
        self.terminal.ws.pty_close(self.terminal.id)
    }

    fn clone_killer(&self) -> Box<dyn ChildKiller + Send + Sync> {
        Box::new(Killer(self.terminal.clone()))
    }
}

impl Child for RemoteChild {
    fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        Ok(self.exit.get().map(ExitStatus::with_exit_code))
    }

    fn wait(&mut self) -> io::Result<ExitStatus> {
        Ok(ExitStatus::with_exit_code(self.exit.wait()))
    }

    fn process_id(&self) -> Option<u32> {
        None
    }

    #[cfg(windows)]
    fn as_raw_handle(&self) -> Option<std::os::windows::io::RawHandle> {
        None
    }
}

impl Drop for RemoteChild {
    /// A session that never started its waiter must not leave the terminal
    /// running on the server.
    fn drop(&mut self) {
        if self.exit.get().is_none() {
            let _ = self.terminal.ws.pty_close(self.terminal.id);
        }
    }
}

/// A server's terminal through the app's session, driven as the frontend
/// drives it (poll, acknowledge, write, resize). For the tests below and
/// the end-to-end test on a real server (`remote/e2e_tests.rs`).
#[cfg(test)]
pub(crate) mod probe {
    use std::time::{Duration, Instant};

    use super::*;
    use crate::modules::pty::hub::Hub;
    use crate::modules::pty::session::testing::{host, Frontend};
    use crate::modules::pty::wire::Ack;

    pub(crate) struct Probe {
        _hub: Hub,
        pub(crate) session: Arc<Session>,
        frontend: Frontend,
        id: u32,
        acks: Vec<Ack>,
    }

    impl Probe {
        /// Opens a terminal on `server` (connected in the app-wide
        /// registry) in `cwd`, at 80x24.
        pub(crate) fn open(server: &SshHost, cwd: Option<String>, id: u32) -> Self {
            let ws = crate::modules::remote::registry::global()
                .and_then(|registry| registry.workspace(server))
                .expect("the server is connected");
            Self::open_on(ws, server, cwd, id)
        }

        pub(crate) fn open_on(
            ws: Arc<RemoteWorkspace>,
            server: &SshHost,
            cwd: Option<String>,
            id: u32,
        ) -> Self {
            let hub = Hub::new();
            let frontend = Frontend::open(&hub, "main");
            let (host, _reaped) = host(id, &hub);
            let size = PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            };
            let session = spawn_on(host, size, ws, server, cwd, false).unwrap();
            Self {
                _hub: hub,
                session,
                frontend,
                id,
                acks: vec![Ack { id, bytes: 0 }],
            }
        }

        pub(crate) fn write(&self, text: &str) {
            let bytes = text.as_bytes().to_vec();
            tauri::async_runtime::block_on(self.session.write(bytes)).unwrap();
        }

        pub(crate) fn resize(&self, cols: u16, rows: u16) {
            self.session
                .resize(PtySize {
                    rows,
                    cols,
                    pixel_width: 0,
                    pixel_height: 0,
                })
                .unwrap();
        }

        /// Everything the terminal has shown.
        pub(crate) fn output(&self) -> String {
            String::from_utf8_lossy(&self.frontend.received).into_owned()
        }

        /// Polls, acknowledging what arrived, until `done` sees the output.
        pub(crate) fn read_until(&mut self, done: impl Fn(&str) -> bool) -> String {
            let until = Instant::now() + Duration::from_secs(60);
            loop {
                let out = self.output();
                if done(&out) {
                    return out;
                }
                assert!(Instant::now() < until, "timed out; so far: {out:?}");
                assert!(self.frontend.exit.is_none(), "exited; so far: {out:?}");
                self.poll();
            }
        }

        /// Polls until the terminal exits; its exit code.
        pub(crate) fn wait_exit(&mut self) -> i32 {
            let until = Instant::now() + Duration::from_secs(60);
            while self.frontend.exit.is_none() {
                assert!(
                    Instant::now() < until,
                    "no exit; so far: {:?}",
                    self.output()
                );
                self.poll();
            }
            self.frontend.exit.unwrap()
        }

        fn poll(&mut self) {
            self.frontend
                .poll_once(self.id, std::mem::take(&mut self.acks));
            let end = self.frontend.received.len() as u64;
            if end > self.frontend.acked {
                self.frontend.acked = end;
                self.acks.push(Ack {
                    id: self.id,
                    bytes: end,
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::probe::Probe;
    use super::*;
    use crate::modules::remote::testing::{connected, global_connector};
    use crate::modules::workspace::{WorkspaceEnv, WorkspaceRegistry};
    use terax_workspace_core::workspace::Workspace;

    #[cfg(windows)]
    const MARKER: &str = "echo marker-%TERAX_TERMINAL%\r";
    #[cfg(unix)]
    const MARKER: &str = "echo marker-$TERAX_TERMINAL\r";
    #[cfg(windows)]
    const SIZE: (&str, &[&str]) = ("mode con\r", &["133", "41"]);
    #[cfg(unix)]
    const SIZE: (&str, &[&str]) = ("stty size\r", &["41 133"]);

    struct Server {
        name: SshHost,
        ws: Arc<RemoteWorkspace>,
        root: String,
        _dir: tempfile::TempDir,
    }

    fn server(name: &str) -> Server {
        let name = connected(name);
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().to_string_lossy().replace('\\', "/");
        let env = WorkspaceEnv::Ssh { host: name.clone() };
        let root = WorkspaceRegistry::default()
            .authorize_in(&env, &base)
            .unwrap();
        let ws = crate::modules::remote::registry::global()
            .and_then(|registry| registry.workspace(&name))
            .unwrap();
        Server {
            name,
            ws,
            root,
            _dir: dir,
        }
    }

    fn open(server: &Server, id: u32) -> Probe {
        Probe::open_on(
            server.ws.clone(),
            &server.name,
            Some(server.root.clone()),
            id,
        )
    }

    #[test]
    #[cfg_attr(
        target_os = "macos",
        ignore = "the agent runs on Linux servers and refuses macOS"
    )]
    fn a_server_terminal_runs_through_the_apps_session() {
        let server = server("pty-session");
        let mut terminal = open(&server, 1);
        assert_eq!(terminal.session.shell_pid, 0, "no process on this machine");
        let leaf = server.root.rsplit('/').next().unwrap().to_string();
        terminal.write(MARKER);
        terminal.read_until(|out| out.contains("marker-1") && out.contains(&leaf));

        terminal.resize(133, 41);
        terminal.write(SIZE.0);
        terminal.read_until(|out| SIZE.1.iter().all(|s| out.contains(s)));

        terminal.write("exit 6\r");
        assert_eq!(terminal.wait_exit(), 6);
    }

    #[test]
    fn a_dropped_connection_ends_the_terminal_and_says_so() {
        let server = server("pty-drop");
        let mut terminal = open(&server, 2);
        terminal.write(MARKER);
        terminal.read_until(|out| out.contains("marker-1"));

        global_connector().cut(&server.name);
        assert_eq!(terminal.wait_exit(), CONNECTION_ENDED_EXIT);
        let out = terminal.output();
        assert!(
            out.contains("[the connection to pty-drop ended]"),
            "{out:?}"
        );
    }

    #[test]
    fn closing_the_session_ends_the_terminal_on_the_server() {
        let server = server("pty-close");
        let mut terminal = open(&server, 3);
        terminal.write(MARKER);
        terminal.read_until(|out| out.contains("marker-1"));
        terminal.session.killer.lock().unwrap().kill().unwrap();
        assert_ne!(terminal.wait_exit(), CONNECTION_ENDED_EXIT);
        let out = terminal.output();
        assert!(
            !out.contains("connection"),
            "a close is not a drop: {out:?}"
        );
        // The connection is untouched.
        assert!(server.ws.metadata(&server.root).is_ok());
    }
}
