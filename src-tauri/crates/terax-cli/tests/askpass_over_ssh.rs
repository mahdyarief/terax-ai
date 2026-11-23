//! A manual check, not run by default: OpenSSH really runs `terax-cli` as
//! its askpass program (with no terminal, `SSH_ASKPASS_REQUIRE=force`), and
//! the questions arrive at the control endpoint. A stand-in endpoint answers:
//! it trusts the new host key, then gives the passphrase of a throwaway key
//! the server accepts for this test only (restricted to running `true`).
//! The login succeeds, so the server sees no failed attempt (fail2ban
//! counts those, and banned this machine while a wrong answer was used).
//!
//! ```text
//! TERAX_AGENT_SSH_HOST=myserver \
//! cargo test -p terax-cli --test askpass_over_ssh -- --ignored --nocapture
//! ```

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;

use terax_control_protocol::askpass::{classify, AskpassKind, AskpassParams, METHOD_ASKPASS};
use terax_control_protocol::{ControlRequest, ControlResponse};

/// A line in the server's `authorized_keys`, removed on drop.
struct Authorized {
    host: String,
    marker: String,
}

impl Authorized {
    fn add(host: &str, line: &str, marker: &str) -> Self {
        let added = Command::new("ssh")
            .args([
                "-o",
                "BatchMode=yes",
                host,
                "umask 077; mkdir -p ~/.ssh; cat >> ~/.ssh/authorized_keys",
            ])
            .stdin(Stdio::piped())
            .spawn()
            .and_then(|mut child| {
                let mut stdin = child.stdin.take().unwrap();
                stdin.write_all(line.as_bytes())?;
                stdin.write_all(b"\n")?;
                drop(stdin);
                child.wait()
            })
            .unwrap();
        assert!(added.success());
        Self {
            host: host.into(),
            marker: marker.into(),
        }
    }
}

impl Drop for Authorized {
    fn drop(&mut self) {
        let removed = Command::new("ssh")
            .args(["-o", "BatchMode=yes", &self.host])
            .arg(format!(
                "sed -i '/{}/d' ~/.ssh/authorized_keys",
                self.marker
            ))
            .status();
        assert!(
            removed.is_ok_and(|s| s.success()),
            "remove the test key from authorized_keys"
        );
    }
}

const PASSPHRASE: &str = "the-throwaway-passphrase";
const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

#[test]
#[ignore = "needs TERAX_AGENT_SSH_HOST"]
fn ssh_asks_through_terax_cli() {
    let host = std::env::var("TERAX_AGENT_SSH_HOST").expect("set TERAX_AGENT_SSH_HOST");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap().to_string();
    let asked: Arc<Mutex<Vec<(AskpassKind, String)>>> = Arc::default();
    {
        let asked = Arc::clone(&asked);
        thread::spawn(move || {
            for stream in listener.incoming() {
                let mut stream = stream.unwrap();
                let mut line = String::new();
                BufReader::new(&mut stream).read_line(&mut line).unwrap();
                let request: ControlRequest = serde_json::from_str(&line).unwrap();
                assert_eq!(request.method, METHOD_ASKPASS);
                assert_eq!(request.token, TOKEN);
                let params: AskpassParams = serde_json::from_value(request.params).unwrap();
                let kind = classify(&params.prompt, params.hint.as_deref());
                asked.lock().unwrap().push((kind, params.prompt.clone()));
                let answer = match kind {
                    AskpassKind::YesNo { .. } => "yes",
                    _ => PASSPHRASE,
                };
                let response =
                    ControlResponse::success(request.id, serde_json::json!({ "answer": answer }));
                let mut bytes = serde_json::to_vec(&response).unwrap();
                bytes.push(b'\n');
                stream.write_all(&bytes).unwrap();
            }
        });
    }

    let dir = tempfile::tempdir().unwrap();
    let key = dir.path().join("throwaway");
    let made = Command::new("ssh-keygen")
        .args(["-q", "-t", "ed25519", "-N", PASSPHRASE, "-f"])
        .arg(&key)
        .status()
        .unwrap();
    assert!(made.success());
    // ssh asks for a passphrase only once the server accepts the key, so
    // the server accepts it for the length of the test: restricted to
    // running `true`, and removed afterwards even if the test fails.
    let marker = format!("terax-askpass-test-{}", std::process::id());
    let public = std::fs::read_to_string(key.with_extension("pub")).unwrap();
    let public = public
        .split_whitespace()
        .take(2)
        .collect::<Vec<_>>()
        .join(" ");
    let _authorized = Authorized::add(
        &host,
        &format!("restrict,command=\"true\" {public} {marker}"),
        &marker,
    );
    // The host's address from the user's config, then no config at all, so
    // the throwaway key is the only one offered.
    let resolved = Command::new("ssh").args(["-G", &host]).output().unwrap();
    let resolved = String::from_utf8_lossy(&resolved.stdout).into_owned();
    let setting = |name: &str| {
        resolved
            .lines()
            .find_map(|line| line.strip_prefix(&format!("{name} ")))
            .unwrap_or_else(|| panic!("ssh -G gave no {name}"))
            .to_string()
    };
    let target = format!("{}@{}", setting("user"), setting("hostname"));
    let mut command = Command::new("ssh");
    command
        .args(["-F", "none", "-p", &setting("port")])
        .args([
            "-T",
            "-o",
            "ConnectTimeout=15",
            "-o",
            "NumberOfPasswordPrompts=1",
        ])
        .args([
            "-o",
            "PreferredAuthentications=publickey",
            "-o",
            "IdentitiesOnly=yes",
        ])
        .args([
            "-o",
            "IdentityAgent=none",
            "-o",
            "StrictHostKeyChecking=ask",
        ])
        .arg("-i")
        .arg(&key)
        .arg("-o")
        .arg(format!(
            "UserKnownHostsFile={}",
            dir.path().join("known_hosts").display()
        ))
        .args(["--", &target, "true"])
        .env("SSH_ASKPASS", env!("CARGO_BIN_EXE_terax-cli"))
        .env("SSH_ASKPASS_REQUIRE", "force")
        .env("TERAX_SSH_ASKPASS", "1")
        .env("TERAX_CONTROL_ADDR", &address)
        .env("TERAX_CONTROL_TOKEN", TOKEN)
        .stdin(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let output = command.output().unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    println!(
        "ssh exit: {:?}\nssh stderr:\n{stderr}",
        output.status.code()
    );
    let asked = asked.lock().unwrap();
    for (kind, prompt) in asked.iter() {
        println!("asked {kind:?}: {prompt:?}");
    }
    assert!(output.status.success(), "the answered passphrase logs in");
    assert!(
        matches!(
            asked.first(),
            Some((AskpassKind::YesNo { host_key: true }, _))
        ),
        "the new host key is asked first"
    );
    assert!(
        asked.iter().any(|(kind, _)| *kind == AskpassKind::Secret),
        "then the key's passphrase"
    );
    assert!(
        dir.path().join("known_hosts").exists(),
        "the trusted key was recorded"
    );
    assert!(!stderr.contains(PASSPHRASE));
}
