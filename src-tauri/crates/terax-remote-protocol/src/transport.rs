//! Reaching an agent on a server: start the transport (`ssh`, or in tests
//! any program that runs a command), run the bootstrap exchange on its stdio,
//! upload the agent if the server needs it, and hand the same pipes to
//! `RemoteWorkspace::connect`.
//!
//! One process carries the whole session, so a password is asked once. Its
//! stderr (`ssh`'s messages, then the agent's) goes to a `Log` line by line.

use std::collections::VecDeque;
use std::ffi::OsString;
use std::fmt;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStderr, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::Duration;

use terax_workspace_core::env::{SshHost, WorkspaceEnv};

use crate::bootstrap::{
    install_line, parse_report, plan, remote_command, Plan, Report, BOOTSTRAP_PROTOCOL, RUN,
};
use crate::client::{Options, RemoteWorkspace};

/// The oldest OpenSSH with `SSH_ASKPASS_REQUIRE=force`.
pub const MIN_OPENSSH: (u32, u32) = (8, 4);

/// Lines the server's shell may print before the bootstrap starts (startup
/// files that echo); more than this and the output is not ours.
const MAX_NOISE_LINES: usize = 100;
/// The longest bootstrap line read; the rest of a longer one is cut.
const MAX_LINE: u64 = 4096;

/// The program that runs the bootstrap command on the server: its arguments
/// come first, and the command is appended as one last argument.
#[derive(Clone, Debug)]
pub struct Transport {
    pub program: OsString,
    pub args: Vec<OsString>,
    pub env: Vec<(OsString, OsString)>,
}

impl Transport {
    /// `ssh -T` to `host`, with keep-alives (a dead network ends the session
    /// in about 45 s) and a bounded connect. Host keys are checked as the
    /// user's configuration says; `--` keeps the host from ever being read
    /// as an option, on top of `SshHost`'s own checks.
    pub fn ssh(program: impl Into<OsString>, host: &SshHost) -> Self {
        let args = [
            "-T",
            "-o",
            "ServerAliveInterval=15",
            "-o",
            "ServerAliveCountMax=3",
            "-o",
            "ConnectTimeout=20",
            "--",
            host.as_str(),
        ];
        Self {
            program: program.into(),
            args: args.iter().map(OsString::from).collect(),
            env: Vec::new(),
        }
    }

    fn command(&self, remote: &str) -> Command {
        let mut command = Command::new(&self.program);
        command
            .args(&self.args)
            .arg(remote)
            .envs(self.env.iter().map(|(key, value)| (key, value)))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }
        command
    }
}

/// The agent this app would install for a platform.
#[derive(Clone, Debug)]
pub struct AgentFile {
    pub path: PathBuf,
    pub sha256: String,
}

/// The last lines of a connection's stderr and of its bootstrap.
pub struct Log {
    lines: Mutex<VecDeque<String>>,
    capacity: usize,
}

impl Log {
    pub fn new(capacity: usize) -> Self {
        Self {
            lines: Mutex::new(VecDeque::new()),
            capacity: capacity.max(1),
        }
    }

    pub fn push(&self, line: impl Into<String>) {
        let mut lines = self.lines.lock().unwrap_or_else(|p| p.into_inner());
        if lines.len() == self.capacity {
            lines.pop_front();
        }
        lines.push_back(line.into());
    }

    pub fn lines(&self) -> Vec<String> {
        let lines = self.lines.lock().unwrap_or_else(|p| p.into_inner());
        lines.iter().cloned().collect()
    }

    fn last_stderr(&self) -> Option<String> {
        self.lines()
            .into_iter()
            .rev()
            .find_map(|line| line.strip_prefix("stderr: ").map(str::to_string))
            .filter(|line| !line.trim().is_empty())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// The transport is running; it may be asking for a password.
    Connecting,
    /// Uploading the agent (bytes).
    Installing(u64),
    /// The agent is running; exchanging `Hello`s.
    Handshaking,
}

#[derive(Debug)]
pub enum ConnectError {
    /// The transport could not be started (no `ssh`, say).
    Spawn(io::Error),
    /// The session ended before the agent ran: authentication failed, the
    /// host is unreachable, ... `stderr` is its last message, if any.
    Ended { stderr: Option<String> },
    /// The server answered something other than the bootstrap.
    Bootstrap(String),
    /// The server cannot run this app's agent.
    Unsupported(String),
    /// The bootstrap took longer than `ConnectOptions::bootstrap_timeout`.
    TimedOut,
    /// The agent ran but the protocol did not start.
    Handshake(io::Error),
}

impl fmt::Display for ConnectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Spawn(error) => write!(f, "could not start ssh: {error}"),
            Self::Ended { stderr: Some(why) } => write!(f, "{why}"),
            Self::Ended { stderr: None } => {
                write!(f, "the SSH session ended before the agent started")
            }
            Self::Bootstrap(why) => write!(f, "the server could not start the agent: {why}"),
            Self::Unsupported(why) => write!(f, "{why}"),
            Self::TimedOut => write!(f, "the server did not start the agent in time"),
            Self::Handshake(error) => write!(f, "the agent did not answer: {error}"),
        }
    }
}

impl std::error::Error for ConnectError {}

#[derive(Clone, Debug)]
pub struct ConnectOptions {
    /// The client's options; `client.version` also names the agent's install
    /// directory on the server.
    pub client: Options,
    /// From starting the transport to `READY`, prompts and upload included.
    pub bootstrap_timeout: Duration,
}

impl Default for ConnectOptions {
    fn default() -> Self {
        Self {
            client: Options::default(),
            // The user may take up to 300 s to answer a prompt.
            bootstrap_timeout: Duration::from_secs(600),
        }
    }
}

/// The transport process. Dropping it ends the session.
pub struct Process {
    child: Arc<Mutex<Child>>,
}

impl Process {
    /// Whether it has exited, without waiting.
    pub fn exited(&self) -> bool {
        let mut child = self.child.lock().unwrap_or_else(|p| p.into_inner());
        matches!(child.try_wait(), Ok(Some(_)))
    }

    pub fn kill(&self) {
        let mut child = self.child.lock().unwrap_or_else(|p| p.into_inner());
        let _ = child.kill();
        let _ = child.wait();
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        self.kill();
    }
}

pub struct Connected {
    // Dropped first, which closes the agent's input, then the process.
    pub workspace: RemoteWorkspace,
    pub process: Process,
    pub report: Report,
    /// Whether this connect uploaded the agent.
    pub installed: bool,
}

/// Connects to the agent at the other end of `transport`, installing it
/// first if the server does not have this app's. The workspace starts with
/// no roots, so the agent refuses every path until `set_roots`.
pub fn connect(
    transport: &Transport,
    env: WorkspaceEnv,
    bundled: &dyn Fn(&str) -> Option<AgentFile>,
    log: &Arc<Log>,
    on_stage: &dyn Fn(Stage),
    options: ConnectOptions,
) -> Result<Connected, ConnectError> {
    let remote = remote_command(&options.client.version).map_err(ConnectError::Bootstrap)?;
    let mut child = transport
        .command(&remote)
        .spawn()
        .map_err(ConnectError::Spawn)?;
    on_stage(Stage::Connecting);
    let stdin = child.stdin.take().expect("piped stdin");
    let stdout = child.stdout.take().expect("piped stdout");
    let stderr = child.stderr.take().expect("piped stderr");
    let stderr_done = spawn_stderr(stderr, Arc::clone(log));
    let child = Arc::new(Mutex::new(child));
    let process = Process {
        child: Arc::clone(&child),
    };

    // A watchdog ends the process if the bootstrap stalls; the pipes then
    // close and the reads below return.
    let timed_out = Arc::new(AtomicBool::new(false));
    let (done, watch) = mpsc::channel::<()>();
    {
        let child = Arc::clone(&child);
        let timed_out = Arc::clone(&timed_out);
        let timeout = options.bootstrap_timeout;
        thread::spawn(move || {
            if let Err(mpsc::RecvTimeoutError::Timeout) = watch.recv_timeout(timeout) {
                timed_out.store(true, Ordering::SeqCst);
                let mut child = child.lock().unwrap_or_else(|p| p.into_inner());
                let _ = child.kill();
            }
        });
    }

    let mut reader = BufReader::new(stdout);
    let mut writer = stdin;
    let outcome = bootstrap(&mut reader, &mut writer, bundled, log, on_stage);
    drop(done);
    let (report, installed) = match outcome {
        Ok(done) => done,
        Err(error) => {
            if timed_out.load(Ordering::SeqCst) {
                return Err(ConnectError::TimedOut);
            }
            if let BootstrapError::Ended = error {
                // ssh's last words explain it (a refused key, an unknown
                // host), and may follow the end of its output: let it finish
                // writing them before it is stopped.
                drop(writer);
                let _ = stderr_done.recv_timeout(Duration::from_secs(2));
                process.kill();
                return Err(ConnectError::Ended {
                    stderr: log.last_stderr(),
                });
            }
            return Err(error.into_connect());
        }
    };

    on_stage(Stage::Handshaking);
    let workspace = RemoteWorkspace::connect(reader, writer, env, Vec::new(), options.client)
        .map_err(ConnectError::Handshake)?;
    Ok(Connected {
        workspace,
        process,
        report,
        installed,
    })
}

enum BootstrapError {
    /// The output ended (the session closed).
    Ended,
    Bootstrap(String),
    Unsupported(String),
}

impl BootstrapError {
    fn into_connect(self) -> ConnectError {
        match self {
            Self::Ended => ConnectError::Ended { stderr: None },
            Self::Bootstrap(why) => ConnectError::Bootstrap(why),
            Self::Unsupported(why) => ConnectError::Unsupported(why),
        }
    }
}

fn bootstrap(
    reader: &mut impl BufRead,
    writer: &mut impl Write,
    bundled: &dyn Fn(&str) -> Option<AgentFile>,
    log: &Log,
    on_stage: &dyn Fn(Stage),
) -> Result<(Report, bool), BootstrapError> {
    // Skip what the server's startup files print, up to the protocol line.
    let mut noise = 0;
    loop {
        let line = read_line(reader)?;
        if line == BOOTSTRAP_PROTOCOL {
            break;
        }
        log.push(format!("server: {line}"));
        noise += 1;
        if noise > MAX_NOISE_LINES {
            return Err(BootstrapError::Bootstrap(
                "the server printed too much before the bootstrap (check the shell's startup files)"
                    .into(),
            ));
        }
    }
    let mut lines = vec![BOOTSTRAP_PROTOCOL.to_string()];
    while lines.last().map(String::as_str) != Some("END") && lines.len() < 16 {
        lines.push(read_line(reader)?);
    }
    let borrowed: Vec<&str> = lines.iter().map(String::as_str).collect();
    let report = parse_report(&borrowed).map_err(BootstrapError::Bootstrap)?;
    log.push(format!(
        "bootstrap: {} {}, installed agent {}",
        report.os,
        report.arch,
        report.sha256.as_deref().unwrap_or("none")
    ));

    let decision = plan(&report, |platform| {
        bundled(platform).map(|agent| agent.sha256)
    });
    let installed = match decision {
        Plan::Unsupported(why) => {
            log.push(format!("bootstrap: {why}"));
            return Err(BootstrapError::Unsupported(why));
        }
        Plan::Run => {
            send(writer, RUN.as_bytes())?;
            false
        }
        Plan::Install { platform } => {
            let agent = bundled(platform).ok_or_else(|| {
                BootstrapError::Bootstrap(format!("the agent for {platform} disappeared"))
            })?;
            let mut file = std::fs::File::open(&agent.path).map_err(|error| {
                BootstrapError::Bootstrap(format!("cannot read the agent for {platform}: {error}"))
            })?;
            let len = file
                .metadata()
                .map_err(|error| BootstrapError::Bootstrap(error.to_string()))?
                .len();
            log.push(format!(
                "bootstrap: installing the {platform} agent ({len} bytes)"
            ));
            on_stage(Stage::Installing(len));
            send(writer, install_line(len, &agent.sha256).as_bytes())?;
            // A server that gave up mid-upload closes the pipe; what it said
            // is read below.
            let copied = io::copy(&mut (&mut file).take(len), writer).and_then(|n| {
                writer.flush()?;
                Ok(n)
            });
            if copied.is_ok_and(|n| n != len) {
                return Err(BootstrapError::Bootstrap(
                    "the agent changed while it was uploaded".into(),
                ));
            }
            true
        }
    };

    let answer = read_line(reader)?;
    if answer == "READY" {
        return Ok((report, installed));
    }
    let why = answer.strip_prefix("ERROR ").unwrap_or(&answer).to_string();
    log.push(format!("bootstrap: {why}"));
    Err(BootstrapError::Bootstrap(why))
}

fn send(writer: &mut impl Write, bytes: &[u8]) -> Result<(), BootstrapError> {
    writer
        .write_all(bytes)
        .and_then(|()| writer.flush())
        .map_err(|_| BootstrapError::Ended)
}

fn read_line(reader: &mut impl BufRead) -> Result<String, BootstrapError> {
    let mut buffer = Vec::new();
    let read = reader
        .take(MAX_LINE)
        .read_until(b'\n', &mut buffer)
        .map_err(|_| BootstrapError::Ended)?;
    if read == 0 {
        return Err(BootstrapError::Ended);
    }
    let line = String::from_utf8_lossy(&buffer);
    Ok(line.trim_end_matches(['\n', '\r']).to_string())
}

/// Copies stderr into the log; the channel closes when stderr does.
fn spawn_stderr(stderr: ChildStderr, log: Arc<Log>) -> mpsc::Receiver<()> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let _closed_when_done = tx;
        let mut reader = BufReader::new(stderr);
        let mut buffer = Vec::new();
        loop {
            buffer.clear();
            match (&mut reader).take(MAX_LINE).read_until(b'\n', &mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    let line = String::from_utf8_lossy(&buffer);
                    log.push(format!("stderr: {}", line.trim_end_matches(['\n', '\r'])));
                }
            }
        }
    });
    rx
}

/// The version from `ssh -V` (which prints to stderr), e.g.
/// `OpenSSH_for_Windows_9.5p2, LibreSSL 3.8.2` or `OpenSSH_9.9p1, OpenSSL ...`.
pub fn parse_openssh_version(text: &str) -> Option<(u32, u32)> {
    let start = text.find("OpenSSH_")? + "OpenSSH_".len();
    let rest = &text[start..];
    let rest = rest.strip_prefix("for_Windows_").unwrap_or(rest);
    let mut numbers = rest.split(|c: char| !c.is_ascii_digit());
    let major = numbers.next()?.parse().ok()?;
    let minor = numbers.next()?.parse().ok()?;
    Some((major, minor))
}

/// Refuses a client too old to ask for passwords through Terax.
pub fn check_openssh_version(text: &str) -> Result<(u32, u32), String> {
    let first = text.lines().next().unwrap_or("").trim();
    match parse_openssh_version(text) {
        Some(version) if version >= MIN_OPENSSH => Ok(version),
        Some((major, minor)) => Err(format!(
            "OpenSSH {major}.{minor} is too old to ask for passwords through Terax (8.4 or newer is needed); \
             update it, or set up key authentication with ssh-agent"
        )),
        None => Err(format!("`ssh -V` did not report an OpenSSH version: `{first}`")),
    }
}

/// Runs `<program> -V`.
pub fn openssh_version(program: &OsString) -> Result<(u32, u32), String> {
    let mut command = Command::new(program);
    command.arg("-V").stdin(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let output = command
        .output()
        .map_err(|error| format!("could not run ssh: {error}"))?;
    let mut text = String::from_utf8_lossy(&output.stderr).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stdout));
    check_openssh_version(&text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openssh_versions_parse_on_every_platform() {
        assert_eq!(
            parse_openssh_version("OpenSSH_for_Windows_9.5p2, LibreSSL 3.8.2"),
            Some((9, 5))
        );
        assert_eq!(
            parse_openssh_version("OpenSSH_9.9p1, OpenSSL 3.2.3 3 Sep 2024"),
            Some((9, 9))
        );
        assert_eq!(
            parse_openssh_version("OpenSSH_8.4p1 Debian-5, OpenSSL 1.1.1n"),
            Some((8, 4))
        );
        assert_eq!(parse_openssh_version("OpenSSH_10.0p2"), Some((10, 0)));
        assert_eq!(parse_openssh_version("PuTTY Release 0.81"), None);
        assert_eq!(parse_openssh_version("OpenSSH_x"), None);
    }

    #[test]
    fn clients_older_than_8_4_are_refused() {
        assert_eq!(check_openssh_version("OpenSSH_8.4p1"), Ok((8, 4)));
        assert_eq!(check_openssh_version("OpenSSH_10.0p1"), Ok((10, 0)));
        let old = check_openssh_version("OpenSSH_for_Windows_8.1p1, LibreSSL 3.0.2").unwrap_err();
        assert!(
            old.contains("8.1") && old.contains("key authentication"),
            "{old}"
        );
        assert!(check_openssh_version("OpenSSH_7.9p1").is_err());
        assert!(check_openssh_version("plink: unknown option").is_err());
    }

    #[test]
    fn the_ssh_command_ends_options_before_the_host() {
        let host = SshHost::try_from("me@box:2222").unwrap();
        let transport = Transport::ssh("ssh", &host);
        let args: Vec<_> = transport.args.iter().map(|a| a.to_str().unwrap()).collect();
        assert_eq!(args[0], "-T");
        assert_eq!(&args[args.len() - 2..], ["--", "me@box:2222"]);
        assert!(args.contains(&"ServerAliveInterval=15"));
        assert!(!args.iter().any(|a| a.contains("StrictHostKeyChecking")));
    }

    #[test]
    fn the_log_keeps_the_last_lines() {
        let log = Log::new(2);
        log.push("stderr: one");
        log.push("server: two");
        log.push("stderr: three");
        assert_eq!(log.lines(), ["server: two", "stderr: three"]);
        assert_eq!(log.last_stderr().as_deref(), Some("three"));
    }
}
