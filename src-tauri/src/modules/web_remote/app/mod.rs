//! Full-app web remote bridge: policy, relay sessions, invoke, callbacks, and schemes.

pub mod audit;
pub mod callbacks;
pub mod invoke;
#[cfg(test)]
mod live_tests;
pub mod policy;
pub mod proxy;
pub mod schemes;
pub mod serve;
pub mod sessions;
