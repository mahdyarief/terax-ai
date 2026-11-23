//! The real `Connector`: the user's `ssh`, the bundled agent, and Terax's
//! askpass for attempts the user started.

use std::ffi::OsString;
use std::sync::Arc;

use terax_remote_protocol::client::Options;
use terax_remote_protocol::transport::{
    self, openssh_version, AgentFile, ConnectError, ConnectOptions, Log, Transport,
};
use terax_workspace_core::env::{SshHost, WorkspaceEnv};

use super::machine::{Failure, Stage};
use super::registry::{Connector, Link};
use crate::modules::control::ControlState;
use crate::modules::remote_agent::AgentBundle;

pub struct SshConnector {
    pub ssh: OsString,
    pub version: String,
    pub agents: AgentBundle,
    pub control: ControlState,
    /// Extra `-o` options ahead of the user's config (the app passes none;
    /// tests use a throwaway `UserKnownHostsFile`).
    pub ssh_options: Vec<String>,
}

impl Connector for SshConnector {
    fn connect(
        &self,
        host: &SshHost,
        interactive: bool,
        log: &Arc<Log>,
        on_stage: &dyn Fn(Stage),
    ) -> Result<Link, (String, Failure)> {
        let version = openssh_version(&self.ssh).map_err(|why| (why, Failure::Permanent))?;
        log.push(format!("ssh: OpenSSH {}.{}", version.0, version.1));

        let mut transport = Transport::ssh(&self.ssh, host);
        for option in self.ssh_options.iter().rev() {
            transport
                .args
                .splice(0..0, ["-o".into(), option.clone().into()]);
        }
        // Prompts only for an attempt the user started; an automatic
        // reconnect that would need one fails instead and waits for them.
        let grant = if interactive {
            self.control.askpass_grant(host.as_str())
        } else {
            None
        };
        match &grant {
            Some(grant) => transport.env.extend(grant.env().iter().cloned()),
            None => {
                if interactive {
                    log.push("askpass is unavailable: only keys and ssh-agent can sign in");
                }
                transport
                    .args
                    .splice(0..0, ["-o".into(), "BatchMode=yes".into()]);
            }
        }

        let agents = &self.agents;
        let bundled = |platform: &str| {
            agents.get(platform).ok().flatten().map(|agent| AgentFile {
                path: agent.path,
                sha256: agent.sha256,
            })
        };
        let stage = |stage| {
            on_stage(match stage {
                transport::Stage::Connecting => Stage::Connecting,
                transport::Stage::Installing(bytes) => Stage::Installing(bytes),
                transport::Stage::Handshaking => Stage::Handshaking,
            })
        };
        let options = ConnectOptions {
            client: Options {
                program: "terax".into(),
                version: self.version.clone(),
                ..Options::default()
            },
            ..ConnectOptions::default()
        };
        let env = WorkspaceEnv::Ssh { host: host.clone() };
        let connected = transport::connect(&transport, env, &bundled, log, &stage, options);
        // Questions are over once the session is up (or has failed).
        drop(grant);
        let connected = connected.map_err(|error| {
            let failure = failure_of(&error);
            (error.to_string(), failure)
        })?;
        if connected.installed {
            log.push("the agent was installed");
        }
        Ok(Link {
            workspace: connected.workspace,
            process: Some(connected.process),
        })
    }
}

/// Whether retrying may help. A refused login or host key is permanent:
/// retried every 30 s it only gets this machine banned by the server.
pub fn failure_of(error: &ConnectError) -> Failure {
    match error {
        ConnectError::TimedOut | ConnectError::Handshake(_) => Failure::Transient,
        ConnectError::Ended { stderr } => {
            let refused = stderr.as_deref().is_some_and(|line| {
                let line = line.to_ascii_lowercase();
                [
                    "permission denied",
                    "host key verification failed",
                    "too many authentication failures",
                    "no more authentication methods",
                    "remote host identification has changed",
                    "no matching host key type",
                    "no matching key exchange",
                    "bad configuration option",
                ]
                .iter()
                .any(|needle| line.contains(needle))
            });
            if refused {
                Failure::Permanent
            } else {
                Failure::Transient
            }
        }
        ConnectError::Spawn(_) | ConnectError::Bootstrap(_) | ConnectError::Unsupported(_) => {
            Failure::Permanent
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;

    fn ended(line: &str) -> ConnectError {
        ConnectError::Ended {
            stderr: Some(line.into()),
        }
    }

    #[test]
    fn refused_logins_and_host_keys_are_not_retried() {
        for line in [
            "ubuntu@203.0.113.7: Permission denied (publickey).",
            "Host key verification failed.",
            "Received disconnect from 203.0.113.7 port 22:2: Too many authentication failures",
            "me@box: Permission denied (publickey,password).",
            "Unable to negotiate with 203.0.113.7 port 22: no matching host key type found.",
        ] {
            assert_eq!(failure_of(&ended(line)), Failure::Permanent, "{line}");
        }
        assert_eq!(
            failure_of(&ConnectError::Unsupported("riscv64".into())),
            Failure::Permanent
        );
        assert_eq!(
            failure_of(&ConnectError::Spawn(io::Error::from(
                io::ErrorKind::NotFound
            ))),
            Failure::Permanent
        );
    }

    #[test]
    fn network_trouble_is_retried() {
        for line in [
            "ssh: connect to host 203.0.113.7 port 22: Connection timed out",
            "ssh: connect to host 203.0.113.7 port 22: Connection refused",
            "ssh: Could not resolve hostname dypod: No such host is known.",
            "Connection reset by 203.0.113.7 port 22",
            "client_loop: send disconnect: Broken pipe",
        ] {
            assert_eq!(failure_of(&ended(line)), Failure::Transient, "{line}");
        }
        assert_eq!(
            failure_of(&ConnectError::Ended { stderr: None }),
            Failure::Transient
        );
        assert_eq!(failure_of(&ConnectError::TimedOut), Failure::Transient);
    }
}
