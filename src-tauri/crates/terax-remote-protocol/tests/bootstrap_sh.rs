//! The bootstrap script run for real, the way `ssh` runs it: the user's
//! login shell gets the command with `-c`. Unix only; a stand-in script plays
//! the agent and says it ran.
#![cfg(unix)]

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use sha2::{Digest, Sha256};
use terax_remote_protocol::bootstrap::{
    install_line, parse_report, plan, remote_command, Plan, Report, INSTALL_DIR, RUN,
};

const VERSION: &str = "0.9.4-dy";
const AGENT: &[u8] = b"#!/bin/sh\necho AGENT-RAN\n";

fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The login shells present here; `sh` always is.
fn shells() -> Vec<&'static str> {
    ["sh", "bash", "dash", "zsh", "fish"]
        .into_iter()
        .filter(|shell| {
            Command::new(shell)
                .arg("-c")
                .arg("true")
                .status()
                .is_ok_and(|s| s.success())
        })
        .collect()
}

struct Session {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
}

impl Session {
    fn start(shell: &str, home: &Path, path: Option<&str>) -> Self {
        let mut command = Command::new(shell);
        command
            .arg("-c")
            .arg(remote_command(VERSION).unwrap())
            .env("HOME", home)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        if let Some(path) = path {
            command.env("PATH", path);
        }
        let mut child = command.spawn().unwrap();
        let stdin = child.stdin.take();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        Self {
            child,
            stdin,
            stdout,
        }
    }

    fn line(&mut self) -> String {
        let mut line = String::new();
        self.stdout.read_line(&mut line).unwrap();
        line.trim_end().to_string()
    }

    fn report(&mut self) -> Report {
        let mut lines = Vec::new();
        loop {
            let line = self.line();
            let end = line == "END" || line.is_empty();
            lines.push(line);
            if end {
                break;
            }
        }
        let lines: Vec<&str> = lines.iter().map(String::as_str).collect();
        parse_report(&lines).unwrap()
    }

    fn send(&mut self, bytes: &[u8]) {
        self.stdin.as_mut().unwrap().write_all(bytes).unwrap();
        self.stdin.as_mut().unwrap().flush().unwrap();
    }

    /// Everything after the instruction, once the session ends.
    fn finish(mut self) -> String {
        drop(self.stdin.take());
        let mut rest = String::new();
        self.stdout.read_to_string(&mut rest).unwrap();
        self.child.wait().unwrap();
        rest
    }
}

fn agent_path(home: &Path) -> PathBuf {
    home.join(INSTALL_DIR).join(VERSION).join("terax-agent")
}

fn mode(path: &Path) -> u32 {
    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
}

fn leftovers(home: &Path) -> Vec<String> {
    std::fs::read_dir(home.join(INSTALL_DIR).join(VERSION))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .filter(|name| name != "terax-agent")
        .collect()
}

fn bundled(triple: &str) -> Option<String> {
    triple.ends_with("linux-musl").then(|| sha256(AGENT))
}

#[test]
fn a_first_connect_installs_and_a_second_reuses_in_every_shell() {
    for shell in shells() {
        let home = tempfile::tempdir().unwrap();

        let mut first = Session::start(shell, home.path(), None);
        let report = first.report();
        assert_eq!(report.os, "Linux", "{shell}");
        assert_eq!(report.sha256, None, "{shell}");
        assert!(
            matches!(plan(&report, bundled), Plan::Install { .. }),
            "{shell}"
        );
        first.send(install_line(AGENT.len() as u64, &sha256(AGENT)).as_bytes());
        first.send(AGENT);
        assert_eq!(first.line(), "READY", "{shell}");
        assert_eq!(first.finish().trim(), "AGENT-RAN", "{shell}");

        let agent = agent_path(home.path());
        assert_eq!(std::fs::read(&agent).unwrap(), AGENT, "{shell}");
        assert_eq!(mode(&agent), 0o700, "{shell}");
        assert_eq!(mode(&home.path().join(INSTALL_DIR)), 0o700, "{shell}");
        assert_eq!(mode(agent.parent().unwrap()), 0o700, "{shell}");
        assert!(leftovers(home.path()).is_empty(), "{shell}");

        let mut second = Session::start(shell, home.path(), None);
        let report = second.report();
        assert_eq!(report.sha256, Some(sha256(AGENT)), "{shell}");
        assert_eq!(plan(&report, bundled), Plan::Run, "{shell}");
        second.send(RUN.as_bytes());
        assert_eq!(second.line(), "READY", "{shell}");
        assert_eq!(second.finish().trim(), "AGENT-RAN", "{shell}");
    }
}

#[test]
fn a_truncated_upload_installs_nothing() {
    let home = tempfile::tempdir().unwrap();
    let mut session = Session::start("sh", home.path(), None);
    session.report();
    session.send(install_line(AGENT.len() as u64, &sha256(AGENT)).as_bytes());
    session.send(&AGENT[..10]);
    let rest = session.finish();
    assert!(rest.starts_with("ERROR checksum mismatch"), "{rest}");
    assert!(!rest.contains("AGENT-RAN"));
    assert!(!agent_path(home.path()).exists());
    assert!(leftovers(home.path()).is_empty());
}

#[test]
fn an_altered_upload_installs_nothing_and_keeps_the_old_agent() {
    let home = tempfile::tempdir().unwrap();
    let agent = agent_path(home.path());
    std::fs::create_dir_all(agent.parent().unwrap()).unwrap();
    std::fs::write(&agent, b"old agent").unwrap();
    let mut session = Session::start("sh", home.path(), None);
    session.report();
    let mut altered = AGENT.to_vec();
    altered[3] ^= 1;
    session.send(install_line(altered.len() as u64, &sha256(AGENT)).as_bytes());
    session.send(&altered);
    let rest = session.finish();
    assert!(rest.starts_with("ERROR checksum mismatch"), "{rest}");
    assert_eq!(std::fs::read(&agent).unwrap(), b"old agent");
    assert!(leftovers(home.path()).is_empty());
}

#[test]
fn a_bad_size_or_an_unknown_instruction_runs_nothing() {
    let home = tempfile::tempdir().unwrap();
    let mut session = Session::start("sh", home.path(), None);
    session.report();
    session.send(b"install 12;rm abc\n");
    assert!(session.finish().starts_with("ERROR bad install size"));

    let mut session = Session::start("sh", home.path(), None);
    session.report();
    session.send(b"quit\n");
    assert_eq!(session.finish(), "");
}

#[test]
fn without_head_c_the_upload_falls_back_to_dd() {
    let home = tempfile::tempdir().unwrap();
    let shim = tempfile::tempdir().unwrap();
    // A `head` that knows no `-c`, ahead of the real one.
    let head = shim.path().join("head");
    std::fs::write(&head, "#!/bin/sh\nexit 1\n").unwrap();
    std::fs::set_permissions(&head, std::fs::Permissions::from_mode(0o755)).unwrap();
    let path = format!(
        "{}:{}",
        shim.path().display(),
        std::env::var("PATH").unwrap()
    );

    let mut session = Session::start("sh", home.path(), Some(&path));
    session.report();
    session.send(install_line(AGENT.len() as u64, &sha256(AGENT)).as_bytes());
    session.send(AGENT);
    assert_eq!(session.line(), "READY");
    assert_eq!(session.finish().trim(), "AGENT-RAN");
    assert_eq!(std::fs::read(agent_path(home.path())).unwrap(), AGENT);
}

#[test]
fn versions_untouched_for_thirty_days_are_removed_and_recent_ones_kept() {
    let home = tempfile::tempdir().unwrap();
    let root = home.path().join(INSTALL_DIR);
    std::fs::create_dir_all(root.join(VERSION)).unwrap();
    for dir in [&root, &root.join(VERSION)] {
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    for (name, stamp) in [("0.9.1-dy", Some("202001010000")), ("0.9.3-dy", None)] {
        std::fs::create_dir_all(root.join(name)).unwrap();
        std::fs::write(root.join(name).join("terax-agent"), b"x").unwrap();
        if let Some(stamp) = stamp {
            assert!(Command::new("touch")
                .args(["-t", stamp])
                .arg(root.join(name))
                .status()
                .unwrap()
                .success());
        }
    }
    let mut session = Session::start("sh", home.path(), None);
    session.report();
    session.send(install_line(AGENT.len() as u64, &sha256(AGENT)).as_bytes());
    session.send(AGENT);
    assert_eq!(session.line(), "READY");
    session.finish();
    // Made before with looser modes; the bootstrap tightens them.
    assert_eq!(mode(&root), 0o700, "the install root is private");
    assert_eq!(mode(&root.join(VERSION)), 0o700);
    assert!(!root.join("0.9.1-dy").exists(), "stale version removed");
    assert!(root.join("0.9.3-dy").exists(), "recent version kept");
    assert!(root.join(VERSION).exists());
}
