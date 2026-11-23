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
