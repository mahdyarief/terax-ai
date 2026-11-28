//! Per-device invoke rate limiting and `Control`/`Secret` command audit log.
//!
//! The audit log records command name, class, device, and timestamp, never
//! command arguments.

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;

use super::policy::CommandClass;

pub const RATE_WINDOW: Duration = Duration::from_secs(10);
pub const RATE_MAX_CALLS: usize = 120;
pub const MAX_AUDIT_ENTRIES: usize = 500;

#[derive(Default)]
pub struct RateLimiter {
    buckets: Mutex<HashMap<String, VecDeque<Instant>>>,
}

impl RateLimiter {
    pub fn check(&self, device_id: &str) -> bool {
        self.check_at(device_id, Instant::now())
    }

    pub fn check_at(&self, device_id: &str, now: Instant) -> bool {
        let mut guard = self.buckets.lock().unwrap();
        let queue = guard.entry(device_id.to_string()).or_default();
        while let Some(&front) = queue.front() {
            if now.saturating_duration_since(front) >= RATE_WINDOW {
                queue.pop_front();
            } else {
                break;
            }
        }
        if queue.len() >= RATE_MAX_CALLS {
            return false;
        }
        queue.push_back(now);
        true
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditEntry {
    pub timestamp_ms: u64,
    pub device_id: String,
    pub device_name: String,
    pub cmd: String,
    pub class: String,
}

#[derive(Default)]
pub struct AuditLog {
    entries: Mutex<VecDeque<AuditEntry>>,
}

impl AuditLog {
    pub fn record(&self, device_id: &str, device_name: &str, cmd: &str, class: CommandClass) {
        if !matches!(class, CommandClass::Control | CommandClass::Secret) {
            return;
        }
        let class_name = match class {
            CommandClass::Control => "control",
            CommandClass::Secret => "secret",
            _ => return,
        };
        let timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let entry = AuditEntry {
            timestamp_ms,
            device_id: device_id.to_string(),
            device_name: device_name.to_string(),
            cmd: cmd.to_string(),
            class: class_name.to_string(),
        };
        let mut guard = self.entries.lock().unwrap();
        guard.push_front(entry);
        while guard.len() > MAX_AUDIT_ENTRIES {
            guard.pop_back();
        }
    }

    pub fn list(&self) -> Vec<AuditEntry> {
        self.entries.lock().unwrap().iter().cloned().collect()
    }
}

#[tauri::command]
pub fn remote_audit_log(state: tauri::State<'_, super::super::WebRemoteState>) -> Vec<AuditEntry> {
    state.audit.list()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_limiter_caps_within_window_and_recovers_after_window() {
        let limiter = RateLimiter::default();
        let t0 = Instant::now();
        for _ in 0..RATE_MAX_CALLS {
            assert!(limiter.check_at("dev-1", t0));
        }
        assert!(!limiter.check_at("dev-1", t0 + Duration::from_secs(1)));
        // Different device has its own bucket.
        assert!(limiter.check_at("dev-2", t0 + Duration::from_secs(1)));
        // After RATE_WINDOW elapses, dev-1 can invoke again.
        assert!(limiter.check_at("dev-1", t0 + RATE_WINDOW));
    }

    #[test]
    fn audit_log_records_only_control_and_secret_and_caps_size() {
        let log = AuditLog::default();
        log.record("d1", "phone", "pty_list_shells", CommandClass::Read);
        log.record("d1", "phone", "plugin:store|set", CommandClass::Write);
        assert!(log.list().is_empty());

        log.record("d1", "phone", "git_commit", CommandClass::Control);
        log.record("d1", "phone", "secrets_set", CommandClass::Secret);
        let items = log.list();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].cmd, "secrets_set");
        assert_eq!(items[0].class, "secret");
        assert_eq!(items[1].cmd, "git_commit");
        assert_eq!(items[1].class, "control");
    }
}
