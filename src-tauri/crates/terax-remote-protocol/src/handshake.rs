//! The first message each side sends. Both say which protocol they speak,
//! which program and version they are, where they run and what they can do;
//! `negotiate` decides whether they can talk.
//!
//! A different agent *version* is not a failure here: Phase 3 compares it
//! with the app's and reinstalls. Only the protocol and the capabilities a
//! side requires decide whether this session can go ahead.

use serde::{Deserialize, Serialize};

/// Raised whenever a message changes incompatibly.
pub const PROTOCOL_VERSION: u32 = 3;

/// The `Workspace` operations as `Call` carries them in this protocol.
pub const CAP_WORKSPACE: &str = "workspace.v1";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    pub protocol: u32,
    /// `terax-agent 0.9.4-dy`, `terax 0.9.4-dy`: who is speaking.
    pub program: String,
    pub version: String,
    pub os: String,
    pub arch: String,
    pub capabilities: Vec<String>,
}

impl Hello {
    /// This process, on this machine, offering `capabilities`.
    pub fn local(program: &str, version: &str, capabilities: &[&str]) -> Self {
        Self {
            protocol: PROTOCOL_VERSION,
            program: program.to_string(),
            version: version.to_string(),
            os: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
            capabilities: capabilities.iter().map(|cap| cap.to_string()).collect(),
        }
    }
}

/// What both sides agreed on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Session {
    /// The other side's `Hello`, as it sent it.
    pub peer: Hello,
    /// Capabilities both sides offer.
    pub capabilities: Vec<String>,
}

impl Session {
    pub fn has(&self, capability: &str) -> bool {
        self.capabilities.iter().any(|cap| cap == capability)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Mismatch {
    Protocol { ours: u32, theirs: u32 },
    MissingCapability(String),
}

impl std::fmt::Display for Mismatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Protocol { ours, theirs } => write!(
                f,
                "the other side speaks protocol {theirs}, this side speaks {ours}"
            ),
            Self::MissingCapability(capability) => {
                write!(f, "the other side does not offer `{capability}`")
            }
        }
    }
}

impl std::error::Error for Mismatch {}

/// Whether `ours` can talk to `theirs`, needing every capability in
/// `required` from them.
pub fn negotiate(ours: &Hello, theirs: &Hello, required: &[&str]) -> Result<Session, Mismatch> {
    if ours.protocol != theirs.protocol {
        return Err(Mismatch::Protocol {
            ours: ours.protocol,
            theirs: theirs.protocol,
        });
    }
    if let Some(missing) = required
        .iter()
        .find(|cap| !theirs.capabilities.iter().any(|offered| offered == *cap))
    {
        return Err(Mismatch::MissingCapability(missing.to_string()));
    }
    let capabilities = ours
        .capabilities
        .iter()
        .filter(|cap| theirs.capabilities.contains(cap))
        .cloned()
        .collect();
    Ok(Session {
        peer: theirs.clone(),
        capabilities,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::{decode, encode, Envelope};

    fn agent() -> Hello {
        Hello {
            protocol: PROTOCOL_VERSION,
            program: "terax-agent".into(),
            version: "0.9.5-dy".into(),
            os: "linux".into(),
            arch: "aarch64".into(),
            capabilities: vec![CAP_WORKSPACE.into(), "watch.v1".into()],
        }
    }

    fn app() -> Hello {
        Hello::local("terax", "0.9.4-dy", &[CAP_WORKSPACE, "pty.v1"])
    }

    #[test]
    fn the_same_protocol_agrees_on_shared_capabilities() {
        let session = negotiate(&app(), &agent(), &[CAP_WORKSPACE]).unwrap();
        assert_eq!(session.peer, agent());
        assert_eq!(session.capabilities, [CAP_WORKSPACE]);
        assert!(session.has(CAP_WORKSPACE));
        assert!(!session.has("pty.v1"), "offered by one side only");
        // A different agent version is Phase 3's decision, not a mismatch.
        assert_ne!(session.peer.version, app().version);
    }

    #[test]
    fn a_different_protocol_is_refused_with_both_numbers() {
        let mut old = agent();
        old.protocol = PROTOCOL_VERSION + 1;
        let error = negotiate(&app(), &old, &[CAP_WORKSPACE]).unwrap_err();
        assert_eq!(
            error,
            Mismatch::Protocol {
                ours: PROTOCOL_VERSION,
                theirs: PROTOCOL_VERSION + 1
            }
        );
        assert!(error.to_string().contains(&(PROTOCOL_VERSION + 1).to_string()));
    }

    #[test]
    fn a_missing_required_capability_is_named() {
        let mut bare = agent();
        bare.capabilities.clear();
        assert_eq!(
            negotiate(&app(), &bare, &[CAP_WORKSPACE]).unwrap_err(),
            Mismatch::MissingCapability(CAP_WORKSPACE.into())
        );
        assert!(negotiate(&app(), &bare, &[]).is_ok(), "nothing required");
    }

    #[test]
    fn hello_is_the_first_envelope_and_describes_this_machine() {
        let hello = app();
        assert_eq!(hello.os, std::env::consts::OS);
        assert_eq!(hello.arch, std::env::consts::ARCH);
        let envelope = Envelope::Hello(hello);
        assert_eq!(decode(&encode(&envelope).unwrap()).unwrap(), envelope);
    }
}
