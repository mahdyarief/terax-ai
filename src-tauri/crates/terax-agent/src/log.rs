//! Diagnostics on stderr, which the app keeps as the connection's log:
//! warnings and errors, and everything the watch engine says (a directory
//! the server will not watch, such as past the inotify limit, is a `debug`
//! line there, and the only sign of why a folder stopped updating).

use log::{Level, LevelFilter, Log, Metadata, Record};

/// Starts every line, so the app can tell the agent's notes from ssh's
/// last word on why a connection ended.
pub const PREFIX: &str = "terax-agent log: ";

struct Stderr;

static LOGGER: Stderr = Stderr;

/// Whether a record goes to the connection's log.
pub fn wanted(level: Level, target: &str) -> bool {
    level <= Level::Warn || target.starts_with("terax_workspace_core::watch")
}

impl Log for Stderr {
    fn enabled(&self, metadata: &Metadata) -> bool {
        wanted(metadata.level(), metadata.target())
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            eprintln!("{PREFIX}{}: {}", record.level(), record.args());
        }
    }

    fn flush(&self) {}
}

/// Installs the logger once; later calls do nothing.
pub fn install() {
    if log::set_logger(&LOGGER).is_ok() {
        log::set_max_level(LevelFilter::Debug);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn watch_failures_and_warnings_reach_the_connection_log() {
        assert!(wanted(Level::Debug, "terax_workspace_core::watch::portable"));
        assert!(wanted(Level::Warn, "terax_workspace_core::host"));
        assert!(wanted(Level::Error, "terax_agent::serve"));
        assert!(!wanted(Level::Debug, "terax_workspace_core::host"));
        assert!(!wanted(Level::Info, "terax_agent::serve"));
    }
}
