//! The parts of a workspace that do not depend on Tauri: the environment
//! model, path mapping, and (as Phase 1 of the remote SSH plan proceeds) the
//! `Workspace` trait, its host backend and the search/watch/exec engines.
//! Shared by the app and, later, by the remote agent.

pub mod env;
pub mod exec;
pub mod grep;
pub mod host;
pub mod import;
pub mod index;
pub mod paths;
pub mod watch;
pub mod workspace;

/// Behaviour checks shared by every backend's tests.
#[doc(hidden)]
pub mod contract;
