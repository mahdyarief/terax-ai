//! Bounded, rolling replay log for one live PTY.

use std::collections::VecDeque;

pub(super) const MAX_REPLAY_BYTES: usize = 8 * 1024 * 1024;
pub(super) const MAX_REPLAY_EVENTS: usize = 65_536;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ReplayEvent {
    Output { bytes: Vec<u8> },
    Resize { cols: u16, rows: u16 },
}

impl ReplayEvent {
    /// Sequences this event occupies: one per output byte, one per resize.
    pub(crate) fn sequence_len(&self) -> u64 {
        match self {
            Self::Output { bytes } => bytes.len() as u64,
            Self::Resize { .. } => 1,
        }
    }
}

// Only `replay_after` (kept for tests) builds these now: the rolling log never
// refuses a push, and handoffs replay the whole retained history.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ReplayError {
    /// The requested checkpoint is older than the oldest retained output.
    Truncated,
    InvalidCheckpoint,
}

/// Rolling replay log. Sequence numbers are absolute (bytes plus one per
/// resize since the PTY opened), so checkpoints stay valid while the oldest
/// output is evicted to keep the log under [`MAX_REPLAY_BYTES`] and
/// [`MAX_REPLAY_EVENTS`]. A busy terminal therefore always stays movable; it
/// just carries its newest history, like a scrollback limit.
///
/// It used to stop recording for good once full, which made every long-lived
/// terminal unmovable ("terminal history is incomplete").
#[derive(Debug)]
pub(super) struct ReplayLog {
    events: VecDeque<ReplayEvent>,
    bytes: usize,
    /// Sequence of the first retained event.
    base: u64,
    /// Sequence after the last retained event.
    end: u64,
    /// Terminal size in force at `base`, so a replay starts at the right size.
    base_size: (u16, u16),
    /// Output bytes ever pushed (resizes excluded): the PTY output stream
    /// offset right after the newest retained event.
    output_total: u64,
}

impl ReplayLog {
    pub(super) fn new(cols: u16, rows: u16) -> Self {
        Self {
            events: VecDeque::new(),
            bytes: 0,
            base: 0,
            end: 0,
            base_size: (cols, rows),
            output_total: 0,
        }
    }

    pub(super) fn push_output(&mut self, bytes: &[u8]) -> Result<u64, ReplayError> {
        if bytes.is_empty() {
            return Ok(self.end);
        }
        self.output_total += bytes.len() as u64;
        // A single chunk larger than the whole budget keeps only its tail.
        let kept = if bytes.len() > MAX_REPLAY_BYTES {
            let skipped = bytes.len() - MAX_REPLAY_BYTES;
            self.evict_all();
            self.base += skipped as u64;
            self.end = self.base;
            &bytes[skipped..]
        } else {
            bytes
        };
        self.make_room(kept.len());
        self.events.push_back(ReplayEvent::Output {
            bytes: kept.to_vec(),
        });
        self.bytes += kept.len();
        self.end += kept.len() as u64;
        Ok(self.end)
    }

    pub(super) fn push_resize(&mut self, cols: u16, rows: u16) -> Result<u64, ReplayError> {
        self.make_room(0);
        self.events.push_back(ReplayEvent::Resize { cols, rows });
        self.end += 1;
        Ok(self.end)
    }

    /// Stream offset that follows everything [`Self::replay_all`] returns.
    pub(super) fn output_bytes(&self) -> u64 {
        self.output_total
    }

    pub(crate) fn sequence(&self) -> u64 {
        self.end
    }

    pub(crate) fn size(&self) -> (u16, u16) {
        self.events
            .iter()
            .rev()
            .find_map(|event| match event {
                ReplayEvent::Resize { cols, rows } => Some((*cols, *rows)),
                ReplayEvent::Output { .. } => None,
            })
            .unwrap_or(self.base_size)
    }

    /// True while nothing has been evicted since the PTY opened.
    pub(crate) fn is_complete(&self) -> bool {
        self.base == 0
    }

    /// Everything retained, led by the size in force at the oldest event once
    /// older output (and the resize that set that size) has been evicted.
    pub(crate) fn replay_all(&self) -> Result<Vec<ReplayEvent>, ReplayError> {
        let mut replay = Vec::with_capacity(self.events.len() + 1);
        if self.base > 0 {
            let (cols, rows) = self.base_size;
            replay.push(ReplayEvent::Resize { cols, rows });
        }
        replay.extend(self.events.iter().cloned());
        Ok(replay)
    }

    pub(crate) fn replay_after(
        &self,
        checkpoint: u64,
    ) -> Result<Vec<ReplayEvent>, ReplayError> {
        if checkpoint > self.end {
            return Err(ReplayError::InvalidCheckpoint);
        }
        if checkpoint < self.base {
            return Err(ReplayError::Truncated);
        }
        let mut replay = Vec::new();
        let mut start = self.base;
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

    /// Evicts the oldest events until `incoming` more bytes and one more event fit.
    fn make_room(&mut self, incoming: usize) {
        while !self.events.is_empty()
            && (self.bytes + incoming > MAX_REPLAY_BYTES
                || self.events.len() + 1 > MAX_REPLAY_EVENTS)
        {
            self.evict_front();
        }
    }

    fn evict_front(&mut self) {
        if let Some(event) = self.events.pop_front() {
            self.base += event.sequence_len();
            match event {
                ReplayEvent::Output { bytes } => self.bytes -= bytes.len(),
                ReplayEvent::Resize { cols, rows } => self.base_size = (cols, rows),
            }
        }
    }

    fn evict_all(&mut self) {
        while !self.events.is_empty() {
            self.evict_front();
        }
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
    fn size_tracks_the_last_resize_event() {
        let mut log = ReplayLog::new(80, 24);
        assert_eq!(log.size(), (80, 24));
        log.push_resize(120, 40).unwrap();
        log.push_output(b"output").unwrap();
        assert_eq!(log.size(), (120, 40));
    }

    #[test]
    fn overflow_evicts_the_oldest_output_and_keeps_the_log_usable() {
        let mut log = ReplayLog::new(80, 24);
        log.push_resize(100, 30).unwrap();
        log.push_output(&vec![b'a'; MAX_REPLAY_BYTES - 1]).unwrap();
        let end = log.push_output(b"zz").unwrap();
        assert_eq!(end, 1 + MAX_REPLAY_BYTES as u64 + 1);
        assert!(!log.is_complete());
        let events = log.replay_all().unwrap();
        // The evicted resize still sets the replay's starting size.
        assert!(matches!(
            events[0],
            ReplayEvent::Resize {
                cols: 100,
                rows: 30
            }
        ));
        assert!(matches!(events.last(), Some(ReplayEvent::Output { bytes }) if bytes == b"zz"));
        assert_eq!(log.size(), (100, 30));
        assert!(matches!(log.replay_after(1), Err(ReplayError::Truncated)));
        assert!(log.replay_after(end - 1).is_ok());
    }

    #[test]
    fn a_chunk_bigger_than_the_budget_keeps_its_tail() {
        let mut log = ReplayLog::new(80, 24);
        let mut chunk = vec![b'x'; MAX_REPLAY_BYTES];
        chunk.extend_from_slice(b"tail");
        let end = log.push_output(&chunk).unwrap();
        assert_eq!(end, chunk.len() as u64);
        let events = log.replay_all().unwrap();
        let ReplayEvent::Output { bytes } = events.last().unwrap() else {
            panic!("expected output");
        };
        assert_eq!(bytes.len(), MAX_REPLAY_BYTES);
        assert!(bytes.ends_with(b"tail"));
    }

    #[test]
    fn event_cap_evicts_instead_of_refusing() {
        let mut log = ReplayLog::new(80, 24);
        for _ in 0..MAX_REPLAY_EVENTS + 10 {
            log.push_output(b"x").unwrap();
        }
        assert_eq!(log.sequence(), (MAX_REPLAY_EVENTS + 10) as u64);
        // One leading resize restores the starting size, then the newest events.
        assert_eq!(log.replay_all().unwrap().len(), MAX_REPLAY_EVENTS + 1);
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
