//! SSH connections: one per host, shared by the whole app.

pub mod commands;
pub mod connector;
pub mod machine;
pub mod registry;

#[cfg(test)]
mod registry_tests;
