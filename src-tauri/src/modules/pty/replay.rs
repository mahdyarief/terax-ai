//! Bounded, sequence-zero replay log for one live PTY.
//!
//! Consumed by `Session` in Task 4; the allow disappears once it is wired.
#![allow(dead_code)]

pub(super) const MAX_REPLAY_BYTES: usize = 8 * 1024 * 1024;
pub(super) const MAX_REPLAY_EVENTS: usize = 65_536;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum ReplayEvent {
    Output { bytes: Vec<u8> },
    Resize { cols: u16, rows: u16 },
}

impl ReplayEvent {
    fn sequence_len(&self) -> u64 {
        match self {
            Self::Output { bytes } => bytes.len() as u64,
            Self::Resize { .. } => 1,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ReplayError {
    Truncated,
    InvalidCheckpoint,
}

#[derive(Debug)]
pub(super) struct ReplayLog {
    events: Vec<ReplayEvent>,
    bytes: usize,
    complete: bool,
    creation_cols: u16,
    creation_rows: u16,
}

impl ReplayLog {
    pub(super) fn new(cols: u16, rows: u16) -> Self {
        Self {
            events: Vec::new(),
            bytes: 0,
            complete: true,
            creation_cols: cols,
            creation_rows: rows,
        }
    }

    pub(super) fn push_output(&mut self, bytes: &[u8]) -> Result<u64, ReplayError> {
        if bytes.is_empty() {
            return Ok(self.sequence());
        }
        if !self.complete {
            return Err(ReplayError::Truncated);
        }
        if self.bytes + bytes.len() > MAX_REPLAY_BYTES || self.events.len() + 1 > MAX_REPLAY_EVENTS
        {
            self.complete = false;
            return Err(ReplayError::Truncated);
        }
        self.events.push(ReplayEvent::Output {
            bytes: bytes.to_vec(),
        });
        self.bytes += bytes.len();
        Ok(self.sequence())
    }

    pub(super) fn push_resize(&mut self, cols: u16, rows: u16) -> Result<u64, ReplayError> {
        if !self.complete {
            return Err(ReplayError::Truncated);
        }
        if self.events.len() + 1 > MAX_REPLAY_EVENTS {
            self.complete = false;
            return Err(ReplayError::Truncated);
        }
        self.events.push(ReplayEvent::Resize { cols, rows });
        Ok(self.sequence())
    }

    pub(super) fn sequence(&self) -> u64 {
        self.events.iter().map(ReplayEvent::sequence_len).sum()
    }

    pub(super) fn is_complete(&self) -> bool {
        self.complete
    }

    pub(super) fn replay_all(&self) -> Result<Vec<ReplayEvent>, ReplayError> {
        if !self.complete {
            return Err(ReplayError::Truncated);
        }
        Ok(self.events.clone())
    }

    pub(super) fn replay_after(&self, checkpoint: u64) -> Result<Vec<ReplayEvent>, ReplayError> {
        if !self.complete {
            return Err(ReplayError::Truncated);
        }
        if checkpoint > self.sequence() {
            return Err(ReplayError::InvalidCheckpoint);
        }
        let mut replay = Vec::new();
        let mut start = 0;
        for event in &self.events {
            let end = start + event.sequence_len();
            if end > checkpoint {
                match event {
                    ReplayEvent::Output { bytes } if start < checkpoint => {
                        replay.push(ReplayEvent::Output {
                            bytes: bytes[(checkpoint - start) as usize..].to_vec(),
                        });
                    }
                    event => replay.push(event.clone()),
                }
            }
            start = end;
        }
        Ok(replay)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replay_preserves_output_and_resize_order_from_sequence_zero() {
        let mut log = ReplayLog::new(80, 24);
        assert_eq!(log.push_output(b"one").unwrap(), 3);
        assert_eq!(log.push_resize(100, 30).unwrap(), 4);
        assert_eq!(log.push_output(b"two").unwrap(), 7);
        let events = log.replay_all().unwrap();
        assert_eq!(events.len(), 3);
        assert!(matches!(&events[0], ReplayEvent::Output { bytes } if bytes == b"one"));
        assert!(matches!(
            events[1],
            ReplayEvent::Resize {
                cols: 100,
                rows: 30
            }
        ));
        assert!(matches!(&events[2], ReplayEvent::Output { bytes } if bytes == b"two"));
    }

    #[test]
    fn overflow_marks_the_log_incomplete_and_prevents_replay() {
        let mut log = ReplayLog::new(80, 24);
        let payload = vec![b'x'; MAX_REPLAY_BYTES + 1];
        let _ = log.push_output(&payload);
        assert!(!log.is_complete());
        assert!(matches!(log.replay_all(), Err(ReplayError::Truncated)));
    }

    #[test]
    fn replay_after_slices_mid_output_without_gaps_or_duplicates() {
        let mut log = ReplayLog::new(80, 24);
        log.push_output(b"hello").unwrap();
        log.push_resize(120, 40).unwrap();
        log.push_output(b"world").unwrap();

        let events = log.replay_after(2).unwrap();
        assert_eq!(events.len(), 3);
        assert!(matches!(&events[0], ReplayEvent::Output { bytes } if bytes == b"llo"));
        assert!(matches!(
            events[1],
            ReplayEvent::Resize {
                cols: 120,
                rows: 40
            }
        ));
        assert!(matches!(&events[2], ReplayEvent::Output { bytes } if bytes == b"world"));
    }

    #[test]
    fn replay_after_the_sequence_end_is_empty() {
        let mut log = ReplayLog::new(80, 24);
        log.push_output(b"hello").unwrap();
        assert!(log.replay_after(5).unwrap().is_empty());
    }

    #[test]
    fn invalid_checkpoint_is_rejected() {
        let log = ReplayLog::new(80, 24);
        assert!(matches!(
            log.replay_after(1),
            Err(ReplayError::InvalidCheckpoint)
        ));
    }
}
