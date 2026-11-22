pub mod rss;

#[cfg(windows)]
pub mod job;

// The bounded runner and the console flag live in the workspace core, which
// runs git and helpers for every workspace backend.
pub use terax_workspace_core::exec::{capture, hide_console};
