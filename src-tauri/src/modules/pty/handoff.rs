//! Transaction layer for moving one live PTY between windows.
//!
//! `prepare -> detach -> attach -> commit` is driven by the source window, and
//! any failure before `commit` is repaired by `rollback`, which restores the
//! source host with fresh credentials instead of respawning the process.
//!
//! The logic is generic over [`Target`] so it can be tested without a real
//! pseudoconsole, and so the Tauri commands stay thin wrappers.

use std::collections::HashMap;
use std::sync::Arc;

use super::attachment::{
    Attachment, AttachmentPhase, Handoff, HandoffId, LeaseEpoch, TransitionError,
};
use super::replay::{ReplayError, ReplayEvent};
use super::session::Session;

/// One live terminal, reduced to what a handoff needs.
pub(super) trait Target {
    fn attachment(&self) -> Attachment;
    fn install(&self, attachment: Attachment);
    fn checkpoint(&self) -> Result<u64, ReplayError>;
    fn replay_all(&self) -> Result<Vec<ReplayEvent>, ReplayError>;
    fn replay_after(&self, checkpoint: u64) -> Result<Vec<ReplayEvent>, ReplayError>;
    fn size(&self) -> (u16, u16);
}

pub(super) struct Frozen {
    pub pty_id: u32,
    pub checkpoint: u64,
    pub cols: u16,
    pub rows: u16,
    pub token: u64,
}

pub(super) struct Replay {
    pub pty_id: u32,
    pub cols: u16,
    pub rows: u16,
    pub token: u64,
    pub epoch: u64,
    pub events: Vec<ReplayEvent>,
}

impl Target for Session {
    #[allow(dead_code)]
    fn attachment(&self) -> Attachment {
        self.attachment.read().unwrap().clone()
    }

    fn install(&self, attachment: Attachment) {
        self.replace_attachment(attachment);
    }

    fn checkpoint(&self) -> Result<u64, ReplayError> {
        Session::checkpoint(self)
    }

    fn replay_all(&self) -> Result<Vec<ReplayEvent>, ReplayError> {
        Session::replay_all(self)
    }

    fn replay_after(&self, checkpoint: u64) -> Result<Vec<ReplayEvent>, ReplayError> {
        Session::replay_after(self, checkpoint)
    }

    fn size(&self) -> (u16, u16) {
        Session::size(self)
    }
}

fn explain(error: TransitionError) -> String {
    match error {
        TransitionError::WrongPhase => "terminal is not in a handoff phase".into(),
        TransitionError::WrongToken => "attachment token is stale".into(),
        TransitionError::WrongEpoch => "lease epoch is stale".into(),
        TransitionError::DuplicateHandoff => "handoff id was already used".into(),
        TransitionError::UnknownHandoff => "unknown handoff id".into(),
        TransitionError::WrongWindow => "handoff window does not match".into(),
    }
}

fn replay_error(error: ReplayError) -> String {
    match error {
        ReplayError::Truncated => "terminal history is incomplete; cannot move it".into(),
        ReplayError::InvalidCheckpoint => "checkpoint is ahead of the recorded history".into(),
    }
}

fn session_targets(
    state: &super::PtyState,
    ids: &[u32],
) -> Result<HashMap<u32, Arc<dyn Target>>, String> {
    let sessions = state.sessions.read().unwrap();
    ids.iter()
        .map(|id| {
            sessions
                .get(id)
                .map(|session| (*id, session.clone() as Arc<dyn Target>))
                .ok_or_else(|| format!("no terminal {id}"))
        })
        .collect()
}

pub(super) fn handoff_prepare(
    state: &super::PtyState,
    handoff: HandoffId,
    ids: &[u32],
    from: &str,
    to: &str,
) -> Result<Vec<Frozen>, String> {
    let targets = session_targets(state, ids)?;
    prepare(&targets, handoff, ids, from, to)
}

pub(super) fn handoff_detach(
    state: &super::PtyState,
    handoff: HandoffId,
    ids: &[u32],
    from: &str,
) -> Result<(), String> {
    let targets = session_targets(state, ids)?;
    detach(&targets, handoff, ids, from)
}

pub(super) fn handoff_attach(
    state: &super::PtyState,
    handoff: HandoffId,
    ids: &[u32],
    to: &str,
) -> Result<Vec<Replay>, String> {
    let targets = session_targets(state, ids)?;
    let mut mint = || {
        state
            .next_token
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    };
    attach(&targets, handoff, ids, to, &mut mint)
}

pub(super) fn handoff_commit(
    state: &super::PtyState,
    handoff: HandoffId,
    leases: &[(u32, u64, LeaseEpoch)],
) -> Result<(), String> {
    let ids = leases.iter().map(|(id, _, _)| *id).collect::<Vec<_>>();
    let targets = session_targets(state, &ids)?;
    commit(&targets, handoff, leases)
}

pub(super) fn handoff_rollback(
    state: &super::PtyState,
    handoff: HandoffId,
    ids: &[u32],
) -> Result<Vec<Replay>, String> {
    let targets = session_targets(state, ids)?;
    let mut mint = || {
        state
            .next_token
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    };
    rollback(&targets, handoff, ids, &mut mint)
}

/// Freezes each terminal's output boundary and marks it as moving away from
/// `from`. Repeating the same handoff id returns the same checkpoints.
pub(super) fn prepare(
    targets: &HashMap<u32, Arc<dyn Target>>,
    handoff: HandoffId,
    ids: &[u32],
    from: &str,
    to: &str,
) -> Result<Vec<Frozen>, String> {
    let mut frozen = Vec::with_capacity(ids.len());
    for id in ids {
        let target = targets.get(id).ok_or_else(|| format!("no terminal {id}"))?;
        let mut attachment = target.attachment();
        if attachment.phase() == AttachmentPhase::Attached
            && attachment.handoff_id() == Some(handoff)
            && attachment.handoff_from().as_deref() == Some(from)
            && attachment.handoff_to().as_deref() == Some(attachment.host().as_ref())
        {
            attachment.clear_handoff();
            target.install(attachment.clone());
        }
        if attachment.host().as_ref() != from {
            return Err(format!("terminal {id} does not belong to {from}"));
        }
        if attachment.phase() == AttachmentPhase::Preparing
            && attachment.handoff_id() == Some(handoff)
        {
            let (cols, rows) = target.size();
            frozen.push(Frozen {
                pty_id: *id,
                checkpoint: attachment.checkpoint(),
                cols,
                rows,
                token: attachment.token(),
            });
            continue;
        }
        let checkpoint = target.checkpoint().map_err(replay_error)?;
        let (cols, rows) = target.size();
        let next = attachment
            .prepare(Handoff {
                id: handoff,
                from: Arc::from(from),
                to: Arc::from(to),
                checkpoint,
                source: attachment.clone(),
            })
            .map_err(explain)?;
        target.install(next);
        frozen.push(Frozen {
            pty_id: *id,
            checkpoint,
            cols,
            rows,
            token: attachment.token(),
        });
    }
    Ok(frozen)
}

/// Revokes the source view's authorization without touching the process.
pub(super) fn detach(
    targets: &HashMap<u32, Arc<dyn Target>>,
    handoff: HandoffId,
    ids: &[u32],
    from: &str,
) -> Result<(), String> {
    for id in ids {
        let target = targets.get(id).ok_or_else(|| format!("no terminal {id}"))?;
        let attachment = target.attachment();
        if attachment.host().as_ref() != from {
            return Err(format!("terminal {id} does not belong to {from}"));
        }
        if attachment.phase() == AttachmentPhase::Detached
            && attachment.handoff_id() == Some(handoff)
        {
            continue;
        }
        let next = attachment
            .detach(handoff, attachment.token(), attachment.epoch())
            .map_err(explain)?;
        target.install(next);
    }
    Ok(())
}

/// Installs the target host and returns everything the new view must replay.
pub(super) fn attach(
    targets: &HashMap<u32, Arc<dyn Target>>,
    handoff: HandoffId,
    ids: &[u32],
    to: &str,
    mint: &mut dyn FnMut() -> u64,
) -> Result<Vec<Replay>, String> {
    let mut replays = Vec::with_capacity(ids.len());
    for id in ids {
        let target = targets.get(id).ok_or_else(|| format!("no terminal {id}"))?;
        let attachment = target.attachment();
        if matches!(
            attachment.phase(),
            AttachmentPhase::Attaching | AttachmentPhase::Attached
        ) && attachment.handoff_id() == Some(handoff)
            && attachment.host().as_ref() == to
        {
            replays.push(Replay {
                pty_id: *id,
                cols: target.size().0,
                rows: target.size().1,
                token: attachment.token(),
                epoch: attachment.epoch(),
                events: target.replay_all().map_err(replay_error)?,
            });
            continue;
        }
        let events = target.replay_all().map_err(replay_error)?;
        let token = mint();
        let epoch = attachment.epoch() + 1;
        let next = attachment
            .attach(handoff, Arc::from(to), token, epoch)
            .map_err(explain)?;
        target.install(next);
        let (cols, rows) = target.size();
        replays.push(Replay {
            pty_id: *id,
            cols,
            rows,
            token,
            epoch,
            events,
        });
    }
    Ok(replays)
}

/// Finalizes the move handed off to the target lease.
pub(super) fn commit(
    targets: &HashMap<u32, Arc<dyn Target>>,
    handoff: HandoffId,
    leases: &[(u32, u64, u64)],
) -> Result<(), String> {
    for (id, token, epoch) in leases {
        let target = targets.get(id).ok_or_else(|| format!("no terminal {id}"))?;
        let next = target
            .attachment()
            .commit(handoff, *token, *epoch)
            .map_err(explain)?;
        target.install(next);
    }
    Ok(())
}

/// Restores the source host with fresh credentials and returns the output the
/// source missed while the move was in flight.
pub(super) fn rollback(
    targets: &HashMap<u32, Arc<dyn Target>>,
    handoff: HandoffId,
    ids: &[u32],
    mint: &mut dyn FnMut() -> u64,
) -> Result<Vec<Replay>, String> {
    let mut replays = Vec::with_capacity(ids.len());
    for id in ids {
        let target = targets.get(id).ok_or_else(|| format!("no terminal {id}"))?;
        let attachment = target.attachment();
        // A retried rollback must not mint new credentials and strand the
        // window that already rebuilt its view from the first response.
        if matches!(
            attachment.phase(),
            AttachmentPhase::Attached | AttachmentPhase::Attaching
        ) && attachment.handoff_id() == Some(handoff)
            && attachment.handoff_from().as_deref() == Some(attachment.host().as_ref())
        {
            replays.push(Replay {
                pty_id: *id,
                cols: target.size().0,
                rows: target.size().1,
                token: attachment.token(),
                epoch: attachment.epoch(),
                events: Vec::new(),
            });
            continue;
        }
        let checkpoint = attachment.checkpoint();
        let token = mint();
        let epoch = attachment.epoch() + 1;
        let next = attachment
            .rollback(handoff, token, epoch)
            .map_err(explain)?;
        target.install(next);
        let (cols, rows) = target.size();
        replays.push(Replay {
            pty_id: *id,
            cols,
            rows,
            token,
            epoch,
            events: target.replay_after(checkpoint).map_err(replay_error)?,
        });
    }
    Ok(replays)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::pty::replay::ReplayLog;
    use std::sync::Mutex;

    struct Fake {
        attachment: Mutex<Attachment>,
        replay: Mutex<ReplayLog>,
        size: (u16, u16),
    }

    impl Fake {
        fn new(host: &str, token: u64, cols: u16, rows: u16) -> Arc<Self> {
            Arc::new(Self {
                attachment: Mutex::new(Attachment::new(Arc::from(host), token)),
                replay: Mutex::new(ReplayLog::new(cols, rows)),
                size: (cols, rows),
            })
        }

        fn output(&self, bytes: &[u8]) {
            let _ = self.replay.lock().unwrap().push_output(bytes);
        }
    }

    impl Target for Fake {
        fn attachment(&self) -> Attachment {
            self.attachment.lock().unwrap().clone()
        }
        fn install(&self, attachment: Attachment) {
            *self.attachment.lock().unwrap() = attachment;
        }
        fn checkpoint(&self) -> Result<u64, ReplayError> {
            let log = self.replay.lock().unwrap();
            if !log.is_complete() {
                return Err(ReplayError::Truncated);
            }
            Ok(log.sequence())
        }
        fn replay_all(&self) -> Result<Vec<ReplayEvent>, ReplayError> {
            self.replay.lock().unwrap().replay_all()
        }
        fn replay_after(&self, checkpoint: u64) -> Result<Vec<ReplayEvent>, ReplayError> {
            self.replay.lock().unwrap().replay_after(checkpoint)
        }
        fn size(&self) -> (u16, u16) {
            self.size
        }
    }

    fn targets(fake: &Arc<Fake>) -> HashMap<u32, Arc<dyn Target>> {
        let mut map: HashMap<u32, Arc<dyn Target>> = HashMap::new();
        map.insert(1, fake.clone());
        map
    }

    #[test]
    fn prepare_is_stable_for_a_repeated_handoff_id() {
        let fake = Fake::new("main", 7, 80, 24);
        fake.output(b"hello");
        let map = targets(&fake);
        let first = prepare(&map, 1, &[1], "main", "space-a").expect("prepare");
        let again = prepare(&map, 1, &[1], "main", "space-a").expect("prepare again");
        assert_eq!(first[0].checkpoint, again[0].checkpoint);
        assert_eq!(first[0].checkpoint, 5);
        assert_eq!(fake.attachment().phase(), AttachmentPhase::Preparing);
    }

    #[test]
    fn attach_returns_full_history_and_a_fresh_lease() {
        let fake = Fake::new("main", 7, 80, 24);
        fake.output(b"abc");
        let map = targets(&fake);
        prepare(&map, 1, &[1], "main", "space-a").expect("prepare");
        detach(&map, 1, &[1], "main").expect("detach");
        let mut mint = || 99;
        let replays = attach(&map, 1, &[1], "space-a", &mut mint).expect("attach");
        assert!(matches!(&replays[0].events[0], ReplayEvent::Output { bytes } if bytes == b"abc"));
        assert_eq!(replays[0].token, 99);
        assert_eq!(replays[0].epoch, 1);
        assert_eq!(fake.attachment().host().as_ref(), "space-a");
        assert!(!fake.attachment().authorizes("space-a", 99, 1));
        let committed = fake.attachment().commit(1, 99, 1).expect("commit");
        fake.install(committed);
        assert!(fake.attachment().authorizes("space-a", 99, 1));
    }

    #[test]
    fn rollback_restores_the_source_with_outage_output_only() {
        let fake = Fake::new("main", 7, 80, 24);
        fake.output(b"before");
        let map = targets(&fake);
        prepare(&map, 1, &[1], "main", "space-a").expect("prepare");
        detach(&map, 1, &[1], "main").expect("detach");
        fake.output(b"after");
        let mut mint = || 42;
        let replays = rollback(&map, 1, &[1], &mut mint).expect("rollback");
        assert_eq!(replays[0].epoch, 1);
        assert_eq!(replays[0].token, 42);
        assert!(
            matches!(&replays[0].events[0], ReplayEvent::Output { bytes } if bytes == b"after")
        );
        assert_eq!(fake.attachment().host().as_ref(), "main");
        assert!(fake.attachment().authorizes("main", 42, 1));
    }

    #[test]
    fn rollback_after_commit_is_rejected() {
        let fake = Fake::new("main", 7, 80, 24);
        let map = targets(&fake);
        prepare(&map, 1, &[1], "main", "space-a").expect("prepare");
        detach(&map, 1, &[1], "main").expect("detach");
        let mut mint = || 99;
        attach(&map, 1, &[1], "space-a", &mut mint).expect("attach");
        commit(&map, 1, &[(1, 99, 1)]).expect("commit");
        let mut mint = || 123;
        assert!(rollback(&map, 1, &[1], &mut mint).is_err());
        assert_eq!(fake.attachment().host().as_ref(), "space-a");
    }

    #[test]
    fn foreign_window_cannot_prepare_or_detach() {
        let fake = Fake::new("main", 7, 80, 24);
        let map = targets(&fake);
        assert!(prepare(&map, 1, &[1], "space-b", "space-a").is_err());
        prepare(&map, 1, &[1], "main", "space-a").expect("prepare");
        assert!(detach(&map, 1, &[1], "space-b").is_err());
    }

    #[test]
    fn detach_with_a_stale_token_cannot_revoke_the_lease() {
        let fake = Fake::new("main", 7, 80, 24);
        let map = targets(&fake);
        prepare(&map, 1, &[1], "main", "space-a").expect("prepare");
        let prepared = fake.attachment();
        assert!(prepared.detach(1, 8, 0).is_err());
        assert!(fake.attachment().authorizes("main", 7, 0));
        detach(&map, 1, &[1], "main").expect("detach with current token");
        assert!(!fake.attachment().authorizes("main", 7, 0));
    }

    #[test]
    fn repeated_rollback_returns_the_restored_lease_without_minting_again() {
        let fake = Fake::new("main", 7, 80, 24);
        let map = targets(&fake);
        prepare(&map, 1, &[1], "main", "space-a").expect("prepare");
        detach(&map, 1, &[1], "main").expect("detach");
        let mut minted = 41;
        let mut mint = || {
            minted += 1;
            minted
        };
        let first = rollback(&map, 1, &[1], &mut mint).expect("rollback");
        let second = rollback(&map, 1, &[1], &mut mint).expect("repeat rollback");
        assert_eq!(second[0].token, first[0].token);
        assert_eq!(second[0].epoch, first[0].epoch);
        assert_eq!(minted, 42);
        assert!(fake
            .attachment()
            .authorizes("main", first[0].token, first[0].epoch));
    }

    #[test]
    fn a_handoff_can_be_retried_after_rolling_it_back() {
        let fake = Fake::new("main", 7, 80, 24);
        let map = targets(&fake);
        prepare(&map, 1, &[1], "main", "space-a").expect("prepare");
        detach(&map, 1, &[1], "main").expect("detach");
        let mut mint = || 42;
        rollback(&map, 1, &[1], &mut mint).expect("rollback");
        let retried = prepare(&map, 1, &[1], "main", "space-a").expect("retry prepare");
        assert_eq!(retried[0].pty_id, 1);
        detach(&map, 1, &[1], "main").expect("retry detach");
    }

    #[test]
    fn attach_to_a_different_window_than_prepared_is_rejected() {
        let fake = Fake::new("main", 7, 80, 24);
        let map = targets(&fake);
        prepare(&map, 1, &[1], "main", "space-a").expect("prepare");
        detach(&map, 1, &[1], "main").expect("detach");
        let mut mint = || 99;
        assert!(attach(&map, 1, &[1], "space-b", &mut mint).is_err());
    }

    #[test]
    fn replay_payload_serializes_decimal_u64s_and_base64_output() {
        let replay = Replay {
            pty_id: 7,
            cols: 80,
            rows: 24,
            token: u64::MAX,
            epoch: 2,
            events: vec![
                ReplayEvent::Output {
                    bytes: b"hello".to_vec(),
                },
                ReplayEvent::Resize {
                    cols: 100,
                    rows: 30,
                },
            ],
        };
        let json = serde_json::to_value(super::super::to_replay_payload(replay)).unwrap();
        assert_eq!(json["ptyId"], 7);
        assert_eq!(json["attachToken"], u64::MAX.to_string());
        assert_eq!(json["leaseEpoch"], "2");
        assert_eq!(json["events"][0]["kind"], "output");
        assert_eq!(json["events"][0]["data"], "aGVsbG8=");
        assert_eq!(json["events"][1]["kind"], "resize");
        assert_eq!(json["events"][1]["cols"], 100);
        assert_eq!(json["events"][1]["rows"], 30);
    }

    #[test]
    fn lease_acks_accept_decimal_u64s_and_reject_invalid_numbers() {
        let leases = vec![super::super::PtyLeaseAck {
            pty_id: 7,
            attach_token: u64::MAX.to_string(),
            lease_epoch: "2".into(),
        }];
        assert_eq!(
            super::super::parse_leases(&leases).unwrap(),
            vec![(7, u64::MAX, 2)]
        );
        let invalid = vec![super::super::PtyLeaseAck {
            pty_id: 7,
            attach_token: "not-a-number".into(),
            lease_epoch: "2".into(),
        }];
        assert_eq!(
            super::super::parse_leases(&invalid).unwrap_err(),
            "invalid attach token for terminal 7"
        );
    }
}
