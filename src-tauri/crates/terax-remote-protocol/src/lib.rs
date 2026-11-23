//! The protocol Terax speaks with `terax-agent` over one byte stream (an SSH
//! session's stdio, or local pipes in tests). Pure apart from the client: no
//! Tauri, no async runtime, so the agent can link it.

pub mod client;
pub mod bootstrap;
pub mod frame;
pub mod handshake;
pub mod transport;
pub mod message;
