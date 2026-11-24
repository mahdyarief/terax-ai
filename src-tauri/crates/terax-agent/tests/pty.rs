//! Terminals through the real client and the real agent, joined by pipes as
//! an SSH session joins them. On this machine the agent's terminal is the
//! platform's (ConPTY on Windows); the commands are chosen per platform.

use std::io;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use terax_agent::serve::{serve, Options};
use terax_remote_protocol::client::{self, PtyEvent, RemoteWorkspace};
use terax_remote_protocol::message::{PtyId, PtySpec, PTY_WINDOW};
use terax_workspace_core::env::WorkspaceEnv;
use terax_workspace_core::host::HostWorkspace;
use terax_workspace_core::workspace::Workspace;

struct Connected {
    remote: Option<RemoteWorkspace>,
    root: String,
    agent: Option<JoinHandle<io::Result<()>>>,
    _dir: tempfile::TempDir,
}

fn connect() -> Connected {
    let dir = tempfile::tempdir().unwrap();
    let host = HostWorkspace::new(WorkspaceEnv::Local);
    let root = host.canonicalize(&dir.path().to_string_lossy()).unwrap();
    let (agent_in, to_agent) = io::pipe().unwrap();
    let (from_agent, agent_out) = io::pipe().unwrap();
    let agent = thread::spawn(move || {
        let ws = Arc::new(HostWorkspace::new(WorkspaceEnv::Local));
        serve(agent_in, agent_out, ws, Options::default())
    });
    let remote = RemoteWorkspace::connect(
        from_agent,
        to_agent,
        WorkspaceEnv::Local,
        vec![root.clone()],
        client::Options::default(),
    )
    .unwrap();
    Connected {
        remote: Some(remote),
        root,
        agent: Some(agent),
        _dir: dir,
    }
}

impl Connected {
    fn remote(&self) -> &RemoteWorkspace {
        self.remote.as_ref().unwrap()
    }

    fn open(&self, command: Option<Vec<String>>) -> (PtyId, mpsc::Receiver<PtyEvent>) {
        let spec = PtySpec {
            cwd: Some(self.root.clone()),
            cols: 80,
            rows: 24,
            command,
            env: vec![("TERAX_TEST_VAR".into(), "from-the-app".into())],
            blocks: false,
        };
        let (id, _program, events) = self.remote().pty_open(spec).unwrap();
        (id, events)
    }
}

/// A shell's one-shot command, and an interactive shell.
#[cfg(windows)]
fn run(script: &str) -> Vec<String> {
    vec!["cmd.exe".into(), "/d".into(), "/c".into(), script.into()]
}
#[cfg(windows)]
fn interactive() -> Vec<String> {
    vec!["cmd.exe".into(), "/d".into(), "/q".into(), "/k".into()]
}
#[cfg(windows)]
const VAR: &str = "%TERAX_TEST_VAR%";

#[cfg(unix)]
fn run(script: &str) -> Vec<String> {
    vec!["/bin/sh".into(), "-c".into(), script.into()]
}
#[cfg(unix)]
fn interactive() -> Vec<String> {
    vec!["/bin/sh".into(), "-i".into()]
}
#[cfg(unix)]
const VAR: &str = "$TERAX_TEST_VAR";

/// A terminal asking where the cursor is (ConPTY does, at start) waits for
/// the answer, as xterm.js gives it in the app.
fn answer_cursor_query(remote: &RemoteWorkspace, id: PtyId, bytes: &[u8]) {
    if bytes.windows(4).any(|w| w == b"\x1b[6n") {
        remote.pty_write(id, b"\x1b[1;1R").unwrap();
    }
}

/// How a terminal ended: its exit code and the reason it was ended.
type Exit = (Option<u32>, Option<String>);

/// Output (credited as it is read) until `done` sees it or the terminal
/// exits; the exit when it came.
fn read_until(
    remote: &RemoteWorkspace,
    id: PtyId,
    events: &mpsc::Receiver<PtyEvent>,
    done: impl Fn(&str) -> bool,
) -> (String, Option<Exit>) {
    let until = Instant::now() + Duration::from_secs(30);
    let mut out = Vec::new();
    loop {
        let left = until.saturating_duration_since(Instant::now());
        match events.recv_timeout(left) {
            Ok(PtyEvent::Data(bytes)) => {
                remote.pty_credit(id, bytes.len() as u64).unwrap();
                answer_cursor_query(remote, id, &bytes);
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
            Err(RecvTimeoutError::Timeout) => {
                panic!("timed out; so far: {}", String::from_utf8_lossy(&out))
            }
            Err(RecvTimeoutError::Disconnected) => panic!("the channel closed without an exit"),
        }
    }
}

#[test]
fn a_command_runs_in_the_folder_with_the_apps_environment_and_exits_with_its_code() {
    let c = connect();
    let (id, events) = c.open(Some(run(&format!("echo marker-{VAR}-{VAR} && exit 3"))));
    let (out, exit) = read_until(c.remote(), id, &events, |_| false);
    assert!(out.contains("marker-from-the-app-from-the-app"), "{out:?}");
    assert_eq!(exit, Some((Some(3), None)));

    #[cfg(windows)]
    let (id, events) = c.open(Some(run("cd")));
    #[cfg(unix)]
    let (id, events) = c.open(Some(run("pwd")));
    let (out, exit) = read_until(c.remote(), id, &events, |_| false);
    let leaf = c.root.rsplit(['/', '\\']).next().unwrap();
    assert!(out.contains(leaf), "started in the folder: {out:?}");
    assert_eq!(exit, Some((Some(0), None)));
}

#[test]
fn keystrokes_reach_the_shell_and_a_resize_reaches_the_terminal() {
    let c = connect();
    let (id, events) = c.open(Some(interactive()));
    c.remote()
        .pty_write(id, format!("echo typed-{VAR}\r").as_bytes())
        .unwrap();
    let (out, exit) = read_until(c.remote(), id, &events, |out| {
        out.contains("typed-from-the-app")
    });
    assert!(exit.is_none(), "{out:?}");

    c.remote().pty_resize(id, 133, 41).unwrap();
    #[cfg(windows)]
    let ask = "mode con\r";
    #[cfg(unix)]
    let ask = "stty size\r";
    c.remote().pty_write(id, ask.as_bytes()).unwrap();
    #[cfg(windows)]
    let seen = |out: &str| out.contains("133") && out.contains("41");
    #[cfg(unix)]
    let seen = |out: &str| out.contains("41 133");
    let (out, exit) = read_until(c.remote(), id, &events, seen);
    assert!(exit.is_none(), "{out:?}");

    c.remote().pty_write(id, b"exit 5\r").unwrap();
    let (_, exit) = read_until(c.remote(), id, &events, |_| false);
    assert_eq!(exit, Some((Some(5), None)));
}

#[test]
fn a_flooding_terminal_does_not_hold_up_file_calls_and_arrives_whole() {
    let c = connect();
    // Distinct lines, so no terminal can fold them away.
    let lines = 200_000;
    let mut text = String::new();
    for n in 0..lines {
        text.push_str(&format!("line {n:07} of the flood\n"));
    }
    std::fs::write(format!("{}/flood.txt", c.root), &text).unwrap();
    assert!(text.len() as u64 > 3 * PTY_WINDOW);
    #[cfg(windows)]
    let (id, events) = c.open(Some(run("type flood.txt")));
    #[cfg(unix)]
    let (id, events) = c.open(Some(run("cat flood.txt")));

    // Uncredited, the agent holds the rest back; file calls still answer.
    thread::sleep(Duration::from_millis(500));
    let held: usize = events
        .try_iter()
        .map(|event| match event {
            PtyEvent::Data(bytes) => {
                answer_cursor_query(c.remote(), id, &bytes);
                bytes.len()
            }
            PtyEvent::Exit { .. } => panic!("exited before it was read"),
        })
        .sum();
    assert!(held as u64 <= PTY_WINDOW, "{held} bytes past the window");
    let started = Instant::now();
    for _ in 0..20 {
        c.remote()
            .metadata(&format!("{}/flood.txt", c.root))
            .unwrap();
    }
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "file calls took {:?} beside the flood",
        started.elapsed()
    );

    c.remote().pty_credit(id, held as u64).unwrap();
    let last = format!("line {:07} of the flood", lines - 1);
    let (out, _) = read_until(c.remote(), id, &events, |out| out.contains(&last));
    assert!(out.contains(&last));
    c.remote().pty_close(id).unwrap();
}

#[test]
fn closing_a_terminal_kills_it_and_says_so() {
    let c = connect();
    let (id, events) = c.open(Some(interactive()));
    c.remote().pty_write(id, b"echo ready\r").unwrap();
    read_until(c.remote(), id, &events, |out| out.contains("ready"));
    c.remote().pty_close(id).unwrap();
    let (_, exit) = read_until(c.remote(), id, &events, |_| false);
    let (_, reason) = exit.expect("an exit");
    assert_eq!(reason.as_deref(), Some("the terminal was closed"));
    // Its id is gone; writing to it is harmless.
    c.remote().pty_write(id, b"echo nobody\r").unwrap();
    assert!(c.remote().metadata(&c.root).is_ok());
}

#[test]
fn a_folder_outside_the_roots_is_refused_and_an_empty_command_too() {
    let c = connect();
    let outside = tempfile::tempdir().unwrap();
    let spec = |cwd: String, command: Option<Vec<String>>| PtySpec {
        cwd: Some(cwd),
        cols: 80,
        rows: 24,
        command,
        ..PtySpec::default()
    };
    let outside = outside.path().to_string_lossy().into_owned();
    assert!(c
        .remote()
        .pty_open(spec(outside, Some(run("echo no"))))
        .is_err());
    assert!(c
        .remote()
        .pty_open(spec(c.root.clone(), Some(Vec::new())))
        .is_err());
}

#[test]
fn the_end_of_the_session_kills_its_terminals() {
    let mut c = connect();
    // Writes a file only if it lives for three seconds.
    #[cfg(windows)]
    let late = run("ping -n 4 127.0.0.1 >nul & echo late > late.txt");
    #[cfg(unix)]
    let late = run("sleep 3; echo late > late.txt");
    let (_id, events) = c.open(Some(late));
    // The app goes away: the agent's input ends.
    drop(c.remote.take());
    let served = c.agent.take().unwrap().join().unwrap();
    assert!(served.is_ok(), "{served:?}");
    // From the agent as it ends the session, or from the client when the
    // stream closes first.
    let exit = loop {
        match events.recv_timeout(Duration::from_secs(10)) {
            Ok(PtyEvent::Data(_)) => {}
            Ok(PtyEvent::Exit { reason, .. }) => break reason.unwrap_or_default(),
            Err(error) => panic!("no exit: {error}"),
        }
    };
    assert!(
        exit == "the session ended" || exit.contains("connection to the agent ended"),
        "{exit}"
    );
    thread::sleep(Duration::from_secs(5));
    assert!(
        !std::path::Path::new(&format!("{}/late.txt", c.root)).exists(),
        "the terminal outlived its session"
    );
}
