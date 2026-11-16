//! Pure attachment state machine for one live PTY.
//!
//! A PTY session has exactly one active host. Moving that host between windows
//! walks `Attached -> Preparing -> Detached -> Attaching -> Attached`, and a
//! failed attempt rolls the source attachment back with fresh credentials.
//! This module performs no I/O: it only validates transitions and returns the
//! next state.

use std::sync::Arc;

pub(super) type HandoffId = u64;
pub(super) type LeaseEpoch = u64;
pub(super) type AttachToken = u64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AttachmentPhase {
    Attached,
    Preparing,
    Detached,
    Attaching,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum TransitionError {
    WrongPhase,
    WrongToken,
    WrongEpoch,
    DuplicateHandoff,
    UnknownHandoff,
    WrongWindow,
}

#[derive(Clone, Debug)]
pub(super) struct Attachment {
    host: Arc<str>,
    token: AttachToken,
    epoch: LeaseEpoch,
    phase: AttachmentPhase,
    // Boxed because `Handoff` owns the source `Attachment` snapshot, which
    // would otherwise make the two types infinitely sized.
    handoff: Option<Box<Handoff>>,
    checkpoint: u64,
}

#[derive(Clone, Debug)]
pub(super) struct Handoff {
    pub(super) id: HandoffId,
    pub(super) from: Arc<str>,
    pub(super) to: Arc<str>,
    pub(super) checkpoint: u64,
    pub(super) source: Attachment,
}

impl Attachment {
    pub(super) fn new(host: Arc<str>, token: AttachToken) -> Self {
        Self {
            host,
            token,
            epoch: 0,
            phase: AttachmentPhase::Attached,
            handoff: None,
            checkpoint: 0,
        }
    }

    /// Freezes the output boundary and starts a move away from this host.
    pub(super) fn prepare(&self, handoff: Handoff) -> Result<Attachment, TransitionError> {
        if self.phase != AttachmentPhase::Attached {
            return Err(TransitionError::WrongPhase);
        }
        if handoff.from != self.host || handoff.to == self.host {
            return Err(TransitionError::WrongWindow);
        }
        if self
            .handoff
            .as_ref()
            .is_some_and(|active| active.id == handoff.id)
        {
            return Err(TransitionError::DuplicateHandoff);
        }
        Ok(Attachment {
            host: self.host.clone(),
            token: self.token,
            epoch: self.epoch,
            phase: AttachmentPhase::Preparing,
            checkpoint: handoff.checkpoint,
            handoff: Some(Box::new(handoff)),
        })
    }

    /// Revokes the source view's authorization; the PTY itself keeps running.
    pub(super) fn detach(
        &self,
        id: HandoffId,
        token: AttachToken,
        epoch: LeaseEpoch,
    ) -> Result<Attachment, TransitionError> {
        if self.phase != AttachmentPhase::Preparing {
            return Err(TransitionError::WrongPhase);
        }
        self.check_handoff(id)?;
        self.check_credentials(token, epoch)?;
        Ok(Attachment {
            host: self.host.clone(),
            token: self.token,
            epoch: self.epoch,
            phase: AttachmentPhase::Detached,
            handoff: self.handoff.clone(),
            checkpoint: self.checkpoint,
        })
    }

    /// Installs the target host with the freshly minted credentials.
    pub(super) fn attach(
        &self,
        id: HandoffId,
        to: Arc<str>,
        token: AttachToken,
        epoch: LeaseEpoch,
    ) -> Result<Attachment, TransitionError> {
        if self.phase != AttachmentPhase::Detached {
            return Err(TransitionError::WrongPhase);
        }
        let handoff = self.check_handoff(id)?;
        if to != handoff.to {
            return Err(TransitionError::WrongWindow);
        }
        Ok(Attachment {
            host: to,
            token,
            epoch,
            phase: AttachmentPhase::Attaching,
            handoff: self.handoff.clone(),
            checkpoint: self.checkpoint,
        })
    }

    /// Finalizes the move. Repeating the same committed handoff is a no-op.
    pub(super) fn commit(
        &self,
        id: HandoffId,
        token: AttachToken,
        epoch: LeaseEpoch,
    ) -> Result<Attachment, TransitionError> {
        if self.phase == AttachmentPhase::Attached {
            let handoff = self.check_handoff(id)?;
            let _ = handoff;
            self.check_credentials(token, epoch)?;
            return Ok(self.clone());
        }
        if self.phase != AttachmentPhase::Attaching {
            return Err(TransitionError::WrongPhase);
        }
        self.check_handoff(id)?;
        self.check_credentials(token, epoch)?;
        Ok(Attachment {
            host: self.host.clone(),
            token,
            epoch,
            phase: AttachmentPhase::Attached,
            handoff: self.handoff.clone(),
            checkpoint: self.checkpoint,
        })
    }

    /// Restores the source host snapshot with caller-supplied fresh credentials.
    pub(super) fn rollback(
        &self,
        id: HandoffId,
        token: AttachToken,
        epoch: LeaseEpoch,
    ) -> Result<Attachment, TransitionError> {
        if !matches!(
            self.phase,
            AttachmentPhase::Preparing | AttachmentPhase::Detached | AttachmentPhase::Attaching
        ) {
            return Err(TransitionError::WrongPhase);
        }
        let handoff = self.check_handoff(id)?;
        let mut restored = handoff.source.clone();
        restored.token = token;
        restored.epoch = epoch;
        restored.phase = AttachmentPhase::Attached;
        restored.handoff = None;
        Ok(restored)
    }

    /// True only for the one host holding the current token and epoch.
    pub(super) fn authorizes(&self, host: &str, token: AttachToken, epoch: LeaseEpoch) -> bool {
        matches!(
            self.phase,
            AttachmentPhase::Attached | AttachmentPhase::Preparing
        ) && &*self.host == host
            && self.token == token
            && self.epoch == epoch
    }

    fn check_handoff(&self, id: HandoffId) -> Result<&Handoff, TransitionError> {
        match self.handoff.as_deref() {
            Some(handoff) if handoff.id == id => Ok(handoff),
            _ => Err(TransitionError::UnknownHandoff),
        }
    }

    fn check_credentials(
        &self,
        token: AttachToken,
        epoch: LeaseEpoch,
    ) -> Result<(), TransitionError> {
        if token != self.token {
            return Err(TransitionError::WrongToken);
        }
        if epoch != self.epoch {
            return Err(TransitionError::WrongEpoch);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn source() -> Attachment {
        Attachment::new(Arc::from("main"), 11)
    }

    fn handoff(id: HandoffId, to: &str, checkpoint: u64, source: Attachment) -> Handoff {
        Handoff {
            id,
            from: source.host.clone(),
            to: Arc::from(to),
            checkpoint,
            source,
        }
    }

    fn committed() -> Attachment {
        let origin = source();
        origin
            .prepare(handoff(7, "space-a", 42, origin.clone()))
            .and_then(|a| a.detach(7, 11, 0))
            .and_then(|a| a.attach(7, Arc::from("space-a"), 12, 1))
            .and_then(|a| a.commit(7, 12, 1))
            .expect("complete handoff")
    }

    #[test]
    fn source_keeps_its_lease_until_detach() {
        let origin = source();
        let preparing = origin
            .prepare(handoff(7, "space-a", 42, origin.clone()))
            .expect("prepare");
        assert!(preparing.authorizes("main", 11, 0));
        let detached = preparing.detach(7, 11, 0).expect("detach");
        assert!(!detached.authorizes("main", 11, 0));
    }

    #[test]
    fn prepare_then_detach_then_attach_then_commit_moves_the_host() {
        let origin = source();
        let preparing = origin
            .prepare(handoff(7, "space-a", 42, origin.clone()))
            .expect("prepare");
        let detached = preparing.detach(7, 11, 0).expect("detach");
        let attaching = detached
            .attach(7, Arc::from("space-a"), 12, 1)
            .expect("attach");
        let committed = attaching.commit(7, 12, 1).expect("commit");
        assert_eq!(committed.host.as_ref(), "space-a");
        assert_eq!(committed.checkpoint, 42);
        assert!(committed.authorizes("space-a", 12, 1));
        assert!(!committed.authorizes("main", 11, 0));
    }

    #[test]
    fn rollback_restores_source_with_a_fresh_token_and_epoch() {
        let origin = source();
        let preparing = origin
            .clone()
            .prepare(handoff(7, "space-a", 42, origin))
            .expect("prepare");
        let restored = preparing.rollback(7, 13, 2).expect("rollback");
        assert_eq!(restored.host.as_ref(), "main");
        assert!(restored.authorizes("main", 13, 2));
        assert!(!restored.authorizes("main", 11, 0));
    }

    #[test]
    fn stale_token_or_epoch_is_rejected() {
        let attachment = source();
        assert!(attachment.authorizes("main", 11, 0));
        assert!(!attachment.authorizes("main", 99, 0));
        assert!(!attachment.authorizes("main", 11, 1));
        assert!(!attachment.authorizes("space-a", 11, 0));
    }

    #[test]
    fn duplicate_commit_is_idempotent() {
        let attachment = committed();
        let again = attachment.commit(7, 12, 1).expect("duplicate commit");
        assert_eq!(again.host.as_ref(), "space-a");
        assert!(again.authorizes("space-a", 12, 1));
    }

    #[test]
    fn duplicate_commit_with_stale_credentials_is_rejected() {
        assert_eq!(
            committed().commit(7, 99, 1).unwrap_err(),
            TransitionError::WrongToken
        );
        assert_eq!(
            committed().commit(7, 12, 9).unwrap_err(),
            TransitionError::WrongEpoch
        );
    }

    #[test]
    fn attach_from_attached_is_wrong_phase() {
        assert_eq!(
            committed()
                .attach(7, Arc::from("space-b"), 12, 1)
                .unwrap_err(),
            TransitionError::WrongPhase
        );
    }

    #[test]
    fn detach_with_stale_token_or_epoch_is_rejected() {
        let origin = source();
        let preparing = origin
            .clone()
            .prepare(handoff(7, "space-a", 42, origin))
            .expect("prepare");
        assert_eq!(
            preparing.detach(7, 99, 0).unwrap_err(),
            TransitionError::WrongToken
        );
        assert_eq!(
            preparing.detach(7, 11, 9).unwrap_err(),
            TransitionError::WrongEpoch
        );
    }

    #[test]
    fn prepare_with_wrong_source_or_target_is_rejected() {
        let origin = source();
        let wrong_source = Handoff {
            id: 7,
            from: Arc::from("space-b"),
            to: Arc::from("space-a"),
            checkpoint: 42,
            source: origin.clone(),
        };
        assert_eq!(
            origin.prepare(wrong_source).unwrap_err(),
            TransitionError::WrongWindow
        );
        assert_eq!(
            source()
                .prepare(handoff(7, "main", 42, source()))
                .unwrap_err(),
            TransitionError::WrongWindow
        );
    }

    #[test]
    fn prepare_while_a_handoff_is_in_flight_is_rejected() {
        let origin = source();
        let preparing = origin
            .prepare(handoff(7, "space-a", 42, origin.clone()))
            .expect("prepare");
        assert_eq!(
            preparing
                .prepare(handoff(7, "space-b", 43, origin))
                .unwrap_err(),
            TransitionError::WrongPhase
        );
    }

    #[test]
    fn attach_to_the_wrong_window_is_rejected() {
        let origin = source();
        let detached = origin
            .clone()
            .prepare(handoff(7, "space-a", 42, origin))
            .and_then(|a| a.detach(7, 11, 0))
            .expect("detach");
        assert_eq!(
            detached.attach(7, Arc::from("space-b"), 12, 1).unwrap_err(),
            TransitionError::WrongWindow
        );
    }

    #[test]
    fn unknown_handoff_id_is_rejected() {
        let origin = source();
        let preparing = origin
            .clone()
            .prepare(handoff(7, "space-a", 42, origin))
            .expect("prepare");
        assert_eq!(
            preparing.detach(8, 11, 0).unwrap_err(),
            TransitionError::UnknownHandoff
        );
        assert_eq!(
            committed().commit(8, 12, 1).unwrap_err(),
            TransitionError::UnknownHandoff
        );
    }
}
