//! A connection's life, as a pure state machine: events in, the new state
//! and what to do out. Time comes in with the events, so tests drive it with
//! a fake clock.
//!
//! A first connect that fails stays failed, with the reason: the user is
//! looking and decides. A drop after the connection was up reconnects with
//! backoff, without prompting (an automatic attempt may not ask for a
//! password), and stops when retrying cannot help: a refused login retried
//! every 30 s would only get the client banned (fail2ban).

use std::time::{Duration, Instant};

use serde::Serialize;

/// Waits before automatic attempts 1, 2, 3, ...; the last repeats.
pub const BACKOFF: [Duration; 5] = [
    Duration::from_secs(1),
    Duration::from_secs(2),
    Duration::from_secs(5),
    Duration::from_secs(10),
    Duration::from_secs(30),
];
pub const HEARTBEAT_EVERY: Duration = Duration::from_secs(15);
/// Missed heartbeats in a row that count as a drop.
pub const HEARTBEAT_MISSES: u32 = 3;

pub fn backoff(retry: u32) -> Duration {
    BACKOFF[(retry as usize).min(BACKOFF.len() - 1)]
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum State {
    Idle,
    Connecting,
    Installing {
        bytes: u64,
    },
    Handshaking,
    Ready,
    /// Waiting to try again after a drop; `retry` counts the attempts so far.
    Reconnecting {
        retry: u32,
        #[serde(skip)]
        at: Instant,
        reason: String,
    },
    Failed {
        reason: String,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Connecting,
    Installing(u64),
    Handshaking,
}

/// Why an attempt failed, as far as retrying goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure {
    /// The network or the server may come back: retry.
    Transient,
    /// Retrying the same way cannot help (a refused login, an unsupported
    /// server, no ssh): stop and show the reason.
    Permanent,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// The user asks to connect (or to reconnect now).
    Connect,
    /// The user disconnects.
    Disconnect,
    Stage {
        attempt: u64,
        stage: Stage,
    },
    Connected {
        attempt: u64,
    },
    AttemptFailed {
        attempt: u64,
        reason: String,
        failure: Failure,
    },
    /// The live connection ended.
    Dropped {
        reason: String,
    },
    HeartbeatOk,
    HeartbeatMissed,
    Tick,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// Start attempt `attempt`; `interactive` attempts may prompt.
    Start { attempt: u64, interactive: bool },
    /// End the attempt or connection in hand.
    Close,
}

#[derive(Debug)]
pub struct Machine {
    state: State,
    /// The attempt whose results count; older ones are ignored.
    attempt: u64,
    /// `Some(n)` while the current attempt is automatic retry `n`.
    retry: Option<u32>,
    misses: u32,
}

impl Default for Machine {
    fn default() -> Self {
        Self {
            state: State::Idle,
            attempt: 0,
            retry: None,
            misses: 0,
        }
    }
}

impl Machine {
    pub fn state(&self) -> &State {
        &self.state
    }

    /// When the next `Tick` matters (a scheduled retry), if ever.
    pub fn deadline(&self) -> Option<Instant> {
        match &self.state {
            State::Reconnecting { at, .. } => Some(*at),
            _ => None,
        }
    }

    fn attempting(&self) -> bool {
        matches!(
            self.state,
            State::Connecting | State::Installing { .. } | State::Handshaking
        )
    }

    fn start(&mut self, interactive: bool) -> Vec<Action> {
        self.attempt += 1;
        self.misses = 0;
        self.state = State::Connecting;
        vec![Action::Start {
            attempt: self.attempt,
            interactive,
        }]
    }

    pub fn apply(&mut self, event: Event, now: Instant) -> Vec<Action> {
        match event {
            Event::Connect => match self.state {
                State::Idle | State::Failed { .. } | State::Reconnecting { .. } => {
                    self.retry = None;
                    self.start(true)
                }
                _ => Vec::new(),
            },
            Event::Disconnect => {
                if self.state == State::Idle {
                    return Vec::new();
                }
                self.state = State::Idle;
                self.retry = None;
                // Results of the attempt in flight no longer count.
                self.attempt += 1;
                vec![Action::Close]
            }
            Event::Stage { attempt, stage } if attempt == self.attempt && self.attempting() => {
                self.state = match stage {
                    Stage::Connecting => State::Connecting,
                    Stage::Installing(bytes) => State::Installing { bytes },
                    Stage::Handshaking => State::Handshaking,
                };
                Vec::new()
            }
            Event::Connected { attempt } if attempt == self.attempt && self.attempting() => {
                self.state = State::Ready;
                self.retry = None;
                self.misses = 0;
                Vec::new()
            }
            Event::AttemptFailed {
                attempt,
                reason,
                failure,
            } if attempt == self.attempt && self.attempting() => match (self.retry, failure) {
                (Some(retry), Failure::Transient) => {
                    self.state = State::Reconnecting {
                        retry,
                        at: now + backoff(retry),
                        reason,
                    };
                    vec![Action::Close]
                }
                _ => {
                    self.state = State::Failed { reason };
                    self.retry = None;
                    vec![Action::Close]
                }
            },
            Event::Dropped { reason } if self.state == State::Ready => {
                self.drop_to_reconnect(reason, now)
            }
            Event::HeartbeatOk if self.state == State::Ready => {
                self.misses = 0;
                Vec::new()
            }
            Event::HeartbeatMissed if self.state == State::Ready => {
                self.misses += 1;
                if self.misses >= HEARTBEAT_MISSES {
                    self.drop_to_reconnect(
                        format!(
                            "the server stopped answering ({HEARTBEAT_MISSES} heartbeats missed)"
                        ),
                        now,
                    )
                } else {
                    Vec::new()
                }
            }
            Event::Tick => match &self.state {
                State::Reconnecting { retry, at, .. } if now >= *at => {
                    self.retry = Some(retry + 1);
                    self.start(false)
                }
                _ => Vec::new(),
            },
            // Stale results, and events that do not apply in this state.
            _ => Vec::new(),
        }
    }

    fn drop_to_reconnect(&mut self, reason: String, now: Instant) -> Vec<Action> {
        self.state = State::Reconnecting {
            retry: 0,
            at: now + backoff(0),
            reason,
        };
        self.retry = None;
        vec![Action::Close]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Run {
        machine: Machine,
        now: Instant,
    }

    impl Run {
        fn new() -> Self {
            Self {
                machine: Machine::default(),
                now: Instant::now(),
            }
        }
        fn on(&mut self, event: Event) -> Vec<Action> {
            self.machine.apply(event, self.now)
        }
        fn later(&mut self, by: Duration) -> Vec<Action> {
            self.now += by;
            self.on(Event::Tick)
        }
        fn attempt(&self) -> u64 {
            self.machine.attempt
        }
        fn state(&self) -> &State {
            self.machine.state()
        }
        fn up(&mut self) {
            assert_eq!(
                self.on(Event::Connect),
                [Action::Start {
                    attempt: 1,
                    interactive: true
                }]
            );
            let attempt = self.attempt();
            self.on(Event::Connected { attempt });
            assert_eq!(self.state(), &State::Ready);
        }
        fn fail(&mut self, failure: Failure) -> Vec<Action> {
            let attempt = self.attempt();
            self.on(Event::AttemptFailed {
                attempt,
                reason: "why".into(),
                failure,
            })
        }
    }

    #[test]
    fn a_connect_goes_through_its_stages_to_ready() {
        let mut run = Run::new();
        assert_eq!(
            run.on(Event::Connect),
            [Action::Start {
                attempt: 1,
                interactive: true
            }]
        );
        assert_eq!(run.state(), &State::Connecting);
        run.on(Event::Stage {
            attempt: 1,
            stage: Stage::Installing(2_863_928),
        });
        assert_eq!(run.state(), &State::Installing { bytes: 2_863_928 });
        run.on(Event::Stage {
            attempt: 1,
            stage: Stage::Handshaking,
        });
        assert_eq!(run.state(), &State::Handshaking);
        assert_eq!(run.on(Event::Connected { attempt: 1 }), []);
        assert_eq!(run.state(), &State::Ready);
        // Connecting again while up does nothing.
        assert_eq!(run.on(Event::Connect), []);
    }

    #[test]
    fn a_first_connect_that_fails_stays_failed_even_if_transient() {
        let mut run = Run::new();
        run.on(Event::Connect);
        run.on(Event::Stage {
            attempt: 1,
            stage: Stage::Installing(10),
        });
        assert_eq!(run.fail(Failure::Transient), [Action::Close]);
        assert_eq!(
            run.state(),
            &State::Failed {
                reason: "why".into()
            }
        );
        assert_eq!(run.later(Duration::from_secs(3600)), []);
        // The user retries.
        assert_eq!(
            run.on(Event::Connect),
            [Action::Start {
                attempt: 2,
                interactive: true
            }]
        );
    }

    #[test]
    fn a_drop_reconnects_with_backoff_and_without_prompting() {
        let mut run = Run::new();
        run.up();
        assert_eq!(
            run.on(Event::Dropped {
                reason: "reset".into()
            }),
            [Action::Close]
        );
        assert!(
            matches!(run.state(), State::Reconnecting { retry: 0, reason, .. } if reason == "reset")
        );
        let mut waits = Vec::new();
        for _ in 0..7 {
            // Nothing happens before the deadline...
            let wait = run.machine.deadline().unwrap() - run.now;
            assert_eq!(run.later(wait - Duration::from_millis(1)), []);
            // ...then an automatic attempt, which fails transiently.
            let actions = run.later(Duration::from_millis(1));
            assert!(
                matches!(
                    actions[..],
                    [Action::Start {
                        interactive: false,
                        ..
                    }]
                ),
                "{actions:?}"
            );
            waits.push(wait.as_secs());
            run.fail(Failure::Transient);
        }
        assert_eq!(waits, [1, 2, 5, 10, 30, 30, 30]);
    }

    #[test]
    fn a_reconnect_that_succeeds_is_ready_and_resets_the_backoff() {
        let mut run = Run::new();
        run.up();
        run.on(Event::Dropped {
            reason: "reset".into(),
        });
        run.later(Duration::from_secs(1));
        run.fail(Failure::Transient);
        run.later(Duration::from_secs(2));
        let attempt = run.attempt();
        run.on(Event::Connected { attempt });
        assert_eq!(run.state(), &State::Ready);
        run.on(Event::Dropped {
            reason: "again".into(),
        });
        assert_eq!(
            run.machine.deadline().unwrap() - run.now,
            Duration::from_secs(1)
        );
    }

    #[test]
    fn a_refused_login_while_reconnecting_stops_retrying() {
        let mut run = Run::new();
        run.up();
        run.on(Event::Dropped {
            reason: "reset".into(),
        });
        run.later(Duration::from_secs(1));
        assert_eq!(run.fail(Failure::Permanent), [Action::Close]);
        assert_eq!(
            run.state(),
            &State::Failed {
                reason: "why".into()
            }
        );
        assert_eq!(run.machine.deadline(), None);
        assert_eq!(run.later(Duration::from_secs(3600)), []);
    }

    #[test]
    fn a_drop_during_install_is_a_failed_attempt() {
        let mut run = Run::new();
        run.up();
        run.on(Event::Dropped {
            reason: "reset".into(),
        });
        run.later(Duration::from_secs(1));
        let attempt = run.attempt();
        run.on(Event::Stage {
            attempt,
            stage: Stage::Installing(10),
        });
        // The session ends mid-upload: transient, so the retry continues.
        run.fail(Failure::Transient);
        assert!(matches!(run.state(), State::Reconnecting { retry: 1, .. }));
    }

    #[test]
    fn disconnecting_stops_everything_and_late_results_are_ignored() {
        let mut run = Run::new();
        run.on(Event::Connect);
        assert_eq!(run.on(Event::Disconnect), [Action::Close]);
        assert_eq!(run.state(), &State::Idle);
        // The cancelled attempt reports in late.
        run.on(Event::Connected { attempt: 1 });
        run.on(Event::Stage {
            attempt: 1,
            stage: Stage::Handshaking,
        });
        assert_eq!(run.state(), &State::Idle);
        assert_eq!(run.on(Event::Disconnect), []);
    }

    #[test]
    fn disconnecting_while_reconnecting_cancels_the_retry() {
        let mut run = Run::new();
        run.up();
        run.on(Event::Dropped {
            reason: "reset".into(),
        });
        assert_eq!(run.on(Event::Disconnect), [Action::Close]);
        assert_eq!(run.machine.deadline(), None);
        assert_eq!(run.later(Duration::from_secs(60)), []);
        assert_eq!(run.state(), &State::Idle);
    }

    #[test]
    fn the_user_can_reconnect_now_instead_of_waiting() {
        let mut run = Run::new();
        run.up();
        run.on(Event::Dropped {
            reason: "reset".into(),
        });
        assert_eq!(
            run.on(Event::Connect),
            [Action::Start {
                attempt: 2,
                interactive: true
            }]
        );
        // A user's attempt that fails is shown, not retried.
        run.fail(Failure::Transient);
        assert!(matches!(run.state(), State::Failed { .. }));
    }

    #[test]
    fn three_missed_heartbeats_drop_the_connection() {
        let mut run = Run::new();
        run.up();
        run.on(Event::HeartbeatMissed);
        run.on(Event::HeartbeatMissed);
        run.on(Event::HeartbeatOk);
        run.on(Event::HeartbeatMissed);
        run.on(Event::HeartbeatMissed);
        assert_eq!(run.state(), &State::Ready);
        assert_eq!(run.on(Event::HeartbeatMissed), [Action::Close]);
        assert!(
            matches!(run.state(), State::Reconnecting { reason, .. } if reason.contains("heartbeats"))
        );
        // Heartbeats mean nothing when not connected.
        assert_eq!(run.on(Event::HeartbeatMissed), []);
    }

    #[test]
    fn the_state_serializes_for_the_ui() {
        assert_eq!(
            serde_json::to_value(State::Installing { bytes: 5 }).unwrap(),
            serde_json::json!({ "state": "installing", "bytes": 5 })
        );
        let reconnecting = State::Reconnecting {
            retry: 2,
            at: Instant::now(),
            reason: "reset".into(),
        };
        assert_eq!(
            serde_json::to_value(reconnecting).unwrap(),
            serde_json::json!({ "state": "reconnecting", "retry": 2, "reason": "reset" })
        );
    }
}
