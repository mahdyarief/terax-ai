//! SSH connections: one per host, shared by the whole app.

pub mod commands;
pub mod connector;
pub mod machine;
pub mod registry;
pub mod ssh_config;

#[cfg(test)]
mod registry_tests;

#[cfg(test)]
mod e2e_tests;

#[cfg(test)]
mod answers_guard_tests;

#[cfg(test)]
pub(crate) mod testing;

pub mod watch;

#[cfg(test)]
mod authorize_tests;

#[cfg(test)]
mod fs_tests;

#[cfg(test)]
mod search_tests;

#[cfg(test)]
mod watch_tests;

#[cfg(test)]
mod assets_tests;

#[cfg(test)]
mod git_tests;
