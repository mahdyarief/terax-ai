//! One terminal's output, streamed to a browser viewer.
//!
//! The viewer reads the session's replay log, never the desktop's credit
//! stream: watching costs the desktop nothing. On connect it gets the size,
//! the theme and a snapshot; afterwards only what the log appended. A
//! reconnect resumes from its last event id, and a viewer that fell past the
//! retained log gets a fresh snapshot instead of a gap.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tauri::Manager;
use tokio::sync::{mpsc, watch};

use crate::modules::pty::{ReplayError, ReplayEvent, ReplaySnapshot, Session, TerminalColors};

use super::devices::DeviceStore;
use super::sse::{self, Item};
use super::tabs::TabRegistry;

/// Streams one device may hold at once.
pub const MAX_STREAMS_PER_DEVICE: usize = 16;
/// Bytes a viewer may fall behind before it is rebuilt from a snapshot.
pub const LAG_BYTES: usize = 2 * 1024 * 1024;
/// How often a quiet terminal is re-checked, so an exit is never missed.
const IDLE_POLL: Duration = Duration::from_secs(1);

/// A terminal as a viewer needs it. Implemented over a live [`Session`], and
/// over a fake in tests.
pub(crate) trait Terminal: Send + Sync {
    fn subscribe(&self) -> watch::Receiver<u64>;
    fn snapshot(&self) -> ReplaySnapshot;
    fn replay_after(&self, checkpoint: u64) -> Result<Vec<ReplayEvent>, ReplayError>;
    fn exit_code(&self) -> Option<i32>;
    fn theme(&self) -> Option<TerminalColors>;
    /// Typing shares the session's ordered writer queue with the desktop; it
    /// never takes the attachment lease.
    fn write_remote<'a>(
        &'a self,
        bytes: Vec<u8>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send + 'a>>;
}

struct SessionTerminal(Arc<Session>);

impl Terminal for SessionTerminal {
    fn subscribe(&self) -> watch::Receiver<u64> {
        self.0.subscribe_replay()
    }

    fn snapshot(&self) -> ReplaySnapshot {
        self.0.replay_snapshot()
    }

    fn replay_after(&self, checkpoint: u64) -> Result<Vec<ReplayEvent>, ReplayError> {
        self.0.replay_after(checkpoint)
    }

    fn exit_code(&self) -> Option<i32> {
        self.0.exit_status()
    }

    fn theme(&self) -> Option<TerminalColors> {
        self.0.theme()
    }

    fn write_remote<'a>(
        &'a self,
        bytes: Vec<u8>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send + 'a>> {
        Box::pin(self.0.write_remote(bytes))
    }
}

/// Finds live terminals by PTY id.
pub(crate) trait PtySessions: Send + Sync {
    fn terminal(&self, id: u32) -> Option<Arc<dyn Terminal>>;
}

pub(crate) struct AppSessions(pub tauri::AppHandle);

impl PtySessions for AppSessions {
    fn terminal(&self, id: u32) -> Option<Arc<dyn Terminal>> {
        let state = self
            .0
            .try_state::<crate::modules::pty::PtyState>()?
            .session(id)?;
        Some(Arc::new(SessionTerminal(state)))
    }
}

/// Per-device stream accounting, so one browser tab cannot exhaust the host.
#[derive(Default)]
pub struct StreamLimits {
    counts: Mutex<HashMap<String, usize>>,
}

impl StreamLimits {
    pub fn acquire(self: &Arc<Self>, device: &str) -> Option<StreamSlot> {
        let mut counts = self.counts.lock().unwrap();
        let count = counts.entry(device.to_string()).or_default();
        if *count >= MAX_STREAMS_PER_DEVICE {
            return None;
        }
        *count += 1;
        Some(StreamSlot {
            limits: self.clone(),
            device: device.to_string(),
        })
    }

    fn release(&self, device: &str) {
        let mut counts = self.counts.lock().unwrap();
        if let Some(count) = counts.get_mut(device) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                counts.remove(device);
            }
        }
    }
}

pub struct StreamSlot {
    limits: Arc<StreamLimits>,
    device: String,
}

impl Drop for StreamSlot {
    fn drop(&mut self) {
        self.limits.release(&self.device);
    }
}

/// Whether a PTY belongs to a tab a viewer can see. Ids that are not listed
/// are not even told apart from ids that do not exist.
pub fn listed(tabs: &TabRegistry, id: u32) -> bool {
    tabs.pty_ids().contains(&id)
}

struct Cursor {
    /// Sequence the next event starts after.
    sequence: u64,
    /// Output bytes sent since the last snapshot, for the lag check.
    since_snapshot: usize,
}

/// Serves one viewer until it disconnects, its terminal exits or its device
/// is revoked.
pub(crate) async fn run(
    terminal: Arc<dyn Terminal>,
    resume: Option<u64>,
    tx: mpsc::Sender<Item>,
    devices: Arc<DeviceStore>,
    device_id: String,
    _slot: StreamSlot,
) {
    let mut notices = terminal.subscribe();
    let mut device_updates = devices.subscribe();
    let mut ticker = tokio::time::interval(IDLE_POLL);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    let mut cursor = Cursor {
        sequence: 0,
        since_snapshot: 0,
    };
    // The first pass opens the stream: resume by id, or send a snapshot.
    let mut resume_from = resume;
    let mut started = false;
    let mut failed_at: Option<u64> = None;
    loop {
        if !started {
            started = true;
            match resume_from.take() {
                Some(sequence) => cursor.sequence = sequence,
                None => send_snapshot(&terminal, &tx, &mut cursor),
            }
        }
        let before = cursor.sequence;
        match drain(&terminal, &tx, &mut cursor) {
            Ok(()) => failed_at = None,
            Err(()) => {
                // A log that cannot serve even the snapshot's own sequence is
                // broken: rebuild once, then end instead of spinning.
                if failed_at == Some(before) {
                    break;
                }
                failed_at = Some(before);
                send_snapshot(&terminal, &tx, &mut cursor);
                continue;
            }
        }
        if cursor.since_snapshot > LAG_BYTES {
            send_snapshot(&terminal, &tx, &mut cursor);
            continue;
        }
        if let Some(code) = terminal.exit_code() {
            let data = serde_json::json!({ "code": code }).to_string();
            sse::send(&tx, sse::event(Some(cursor.sequence), "exit", &data));
            break;
        }
        tokio::select! {
            _ = tx.closed() => break,
            _ = ticker.tick() => {}
            _ = notices.changed() => {}
            _ = device_updates.changed() => {
                if !devices.contains(&device_id) {
                    break;
                }
            }
        }
    }
}

fn send_snapshot(terminal: &Arc<dyn Terminal>, tx: &mpsc::Sender<Item>, cursor: &mut Cursor) {
    let snapshot = terminal.snapshot();
    let meta = serde_json::json!({
        "cols": snapshot.cols,
        "rows": snapshot.rows,
        "truncated": snapshot.truncated,
        "theme": terminal.theme(),
    })
    .to_string();
    sse::send(tx, sse::event(None, "meta", &meta));
    let mut bytes = Vec::new();
    for event in &snapshot.events {
        if let ReplayEvent::Output { bytes: chunk } = event {
            bytes.extend_from_slice(chunk);
        }
    }
    let data = super::server::base64(&bytes);
    sse::send(tx, sse::event(Some(snapshot.sequence), "snapshot", &data));
    cursor.sequence = snapshot.sequence;
    cursor.since_snapshot = 0;
}

/// Sends everything the log appended after the cursor. `Err` means the log
/// no longer reaches back that far: only a snapshot can repair it.
fn drain(
    terminal: &Arc<dyn Terminal>,
    tx: &mpsc::Sender<Item>,
    cursor: &mut Cursor,
) -> Result<(), ()> {
    loop {
        let events = terminal.replay_after(cursor.sequence).map_err(|_| ())?;
        if events.is_empty() {
            return Ok(());
        }
        for event in events {
            cursor.sequence += event.sequence_len();
            match event {
                ReplayEvent::Output { bytes } => {
                    cursor.since_snapshot += bytes.len();
                    if cursor.since_snapshot > LAG_BYTES {
                        return Ok(());
                    }
                    let data = super::server::base64(&bytes);
                    sse::send(tx, sse::event(Some(cursor.sequence), "data", &data));
                }
                ReplayEvent::Resize { cols, rows } => {
                    let data = serde_json::json!({ "cols": cols, "rows": rows }).to_string();
                    sse::send(tx, sse::event(Some(cursor.sequence), "resize", &data));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex as StdMutex;

    use super::*;

    #[derive(Clone)]
    enum Event {
        Output(Vec<u8>),
        Resize(u16, u16),
    }

    struct FakeLog {
        events: Vec<Event>,
        /// Sequence before the oldest retained event (truncation point).
        base: u64,
        seq: u64,
    }

    struct FakeTerminal {
        log: StdMutex<FakeLog>,
        notices: watch::Sender<u64>,
        _keepalive: watch::Receiver<u64>,
        exit: StdMutex<Option<i32>>,
        dimensions: StdMutex<(u16, u16)>,
        writes: StdMutex<Vec<u8>>,
    }

    impl FakeTerminal {
        fn new(cols: u16, rows: u16, initial: &[u8]) -> Arc<Self> {
            let (notices, keepalive) = watch::channel(0u64);
            let terminal = Arc::new(Self {
                log: StdMutex::new(FakeLog {
                    events: Vec::new(),
                    base: 0,
                    seq: 0,
                }),
                notices,
                _keepalive: keepalive,
                exit: StdMutex::new(None),
                dimensions: StdMutex::new((cols, rows)),
                writes: StdMutex::new(Vec::new()),
            });
            if !initial.is_empty() {
                terminal.push(initial);
            }
            terminal
        }

        fn push(&self, bytes: &[u8]) {
            let seq = {
                let mut log = self.log.lock().unwrap();
                log.events.push(Event::Output(bytes.to_vec()));
                log.seq += bytes.len() as u64;
                log.seq
            };
            let _ = self.notices.send(seq);
        }

        fn resize(&self, cols: u16, rows: u16) {
            *self.dimensions.lock().unwrap() = (cols, rows);
            let seq = {
                let mut log = self.log.lock().unwrap();
                log.events.push(Event::Resize(cols, rows));
                log.seq += 1;
                log.seq
            };
            let _ = self.notices.send(seq);
        }

        fn truncate(&self, at: u64) {
            let mut log = self.log.lock().unwrap();
            let mut removed = log.base;
            while removed < at {
                let Some(first) = log.events.first() else {
                    break;
                };
                let len = match first {
                    Event::Output(bytes) => bytes.len() as u64,
                    Event::Resize(..) => 1,
                };
                if removed + len > at {
                    break;
                }
                log.events.remove(0);
                removed += len;
            }
            log.base = removed;
        }

        fn exit(&self, code: i32) {
            *self.exit.lock().unwrap() = Some(code);
            let seq = self.log.lock().unwrap().seq;
            let _ = self.notices.send(seq);
        }
    }

    impl Terminal for FakeTerminal {
        fn subscribe(&self) -> watch::Receiver<u64> {
            self.notices.subscribe()
        }

        fn snapshot(&self) -> ReplaySnapshot {
            let log = self.log.lock().unwrap();
            let (cols, rows) = *self.dimensions.lock().unwrap();
            ReplaySnapshot {
                events: log
                    .events
                    .iter()
                    .map(|event| match event {
                        Event::Output(bytes) => ReplayEvent::Output {
                            bytes: bytes.clone(),
                        },
                        Event::Resize(cols, rows) => ReplayEvent::Resize {
                            cols: *cols,
                            rows: *rows,
                        },
                    })
                    .collect(),
                sequence: log.seq,
                cols,
                rows,
                truncated: log.base > 0,
            }
        }

        fn replay_after(&self, checkpoint: u64) -> Result<Vec<ReplayEvent>, ReplayError> {
            let log = self.log.lock().unwrap();
            if checkpoint > log.seq {
                return Err(ReplayError::InvalidCheckpoint);
            }
            if checkpoint < log.base {
                return Err(ReplayError::Truncated);
            }
            let mut at = log.base;
            let mut out = Vec::new();
            for event in &log.events {
                let len = match event {
                    Event::Output(bytes) => bytes.len() as u64,
                    Event::Resize(..) => 1,
                };
                let end = at + len;
                if end > checkpoint {
                    match event {
                        Event::Output(bytes) if at < checkpoint => {
                            out.push(ReplayEvent::Output {
                                bytes: bytes[(checkpoint - at) as usize..].to_vec(),
                            });
                        }
                        Event::Output(bytes) => out.push(ReplayEvent::Output {
                            bytes: bytes.clone(),
                        }),
                        Event::Resize(cols, rows) => out.push(ReplayEvent::Resize {
                            cols: *cols,
                            rows: *rows,
                        }),
                    }
                }
                at = end;
            }
            Ok(out)
        }

        fn exit_code(&self) -> Option<i32> {
            *self.exit.lock().unwrap()
        }

        fn theme(&self) -> Option<TerminalColors> {
            None
        }

        fn write_remote<'a>(
            &'a self,
            bytes: Vec<u8>,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send + 'a>>
        {
            self.writes.lock().unwrap().extend_from_slice(&bytes);
            Box::pin(async { Ok(()) })
        }
    }

    fn device() -> (Arc<DeviceStore>, String) {
        let store = Arc::new(DeviceStore::memory());
        let (_info, token) = store.add("test");
        let caller = store.authenticate(&token).unwrap();
        (store, caller.id)
    }

    fn collect(rx: &mut mpsc::Receiver<Item>) -> String {
        let mut text = String::new();
        while let Ok(item) = rx.try_recv() {
            let frame = item.expect("no stream errors");
            if let Ok(data) = frame.into_data() {
                text.push_str(&String::from_utf8_lossy(&data));
            }
        }
        text
    }

    fn start(
        terminal: Arc<FakeTerminal>,
        resume: Option<u64>,
    ) -> (tokio::runtime::Runtime, mpsc::Receiver<Item>) {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap();
        let (tx, rx) = mpsc::channel(sse::STREAM_BUFFER);
        let (devices, device_id) = device();
        let slot = Arc::new(StreamLimits::default())
            .acquire(&device_id)
            .unwrap();
        let terminal: Arc<dyn Terminal> = terminal;
        runtime.spawn(run(terminal.clone(), resume, tx, devices, device_id, slot));
        (runtime, rx)
    }

    #[test]
    fn a_viewer_gets_meta_then_a_snapshot_then_only_the_new_output() {
        let terminal = FakeTerminal::new(100, 30, b"hello");
        let (runtime, mut rx) = start(terminal.clone(), None);
        runtime.block_on(async {
            tokio::time::sleep(Duration::from_millis(50)).await;
        });
        let first = collect(&mut rx);
        assert!(first.contains("event: meta"), "{first}");
        assert!(first.contains("\"cols\":100"), "{first}");
        assert!(first.contains("event: snapshot"), "{first}");
        assert!(first.contains("aGVsbG8="), "base64 of hello: {first}");

        terminal.push(b"world");
        runtime.block_on(async {
            tokio::time::sleep(Duration::from_millis(50)).await;
        });
        let next = collect(&mut rx);
        assert!(next.contains("event: data"), "{next}");
        assert!(next.contains("d29ybGQ="), "{next}");
        assert!(!next.contains("aGVsbG8="), "no repeated snapshot: {next}");

        terminal.resize(120, 40);
        runtime.block_on(async {
            tokio::time::sleep(Duration::from_millis(50)).await;
        });
        let resized = collect(&mut rx);
        assert!(resized.contains("event: resize"), "{resized}");
        assert!(resized.contains("\"cols\":120"), "{resized}");
    }

    #[test]
    fn a_reconnect_resumes_after_its_last_event_id_without_duplicates() {
        let terminal = FakeTerminal::new(80, 24, b"hello");
        terminal.push(b"world");
        let (runtime, mut rx) = start(terminal, Some(5));
        runtime.block_on(async {
            tokio::time::sleep(Duration::from_millis(50)).await;
        });
        let events = collect(&mut rx);
        assert!(!events.contains("event: snapshot"), "{events}");
        assert!(events.contains("d29ybGQ="), "{events}");
        assert!(!events.contains("aGVsbG8="), "{events}");
        assert!(events.contains("id: 10"), "{events}");
    }

    #[test]
    fn a_checkpoint_older_than_the_log_leads_to_a_snapshot() {
        let terminal = FakeTerminal::new(80, 24, b"hello");
        terminal.push(b"world");
        terminal.truncate(5);
        let (runtime, mut rx) = start(terminal, Some(0));
        runtime.block_on(async {
            tokio::time::sleep(Duration::from_millis(50)).await;
        });
        let events = collect(&mut rx);
        assert!(events.contains("event: snapshot"), "{events}");
        assert!(events.contains("\"truncated\":true"), "{events}");
    }

    #[test]
    fn an_exit_ends_the_stream_with_its_code() {
        let terminal = FakeTerminal::new(80, 24, b"bye");
        terminal.exit(3);
        let (runtime, mut rx) = start(terminal, None);
        runtime.block_on(async {
            tokio::time::sleep(Duration::from_millis(150)).await;
        });
        let events = collect(&mut rx);
        assert!(events.contains("event: exit"), "{events}");
        assert!(events.contains("\"code\":3"), "{events}");
    }

    #[test]
    fn typed_bytes_reach_the_terminal_in_order() {
        let terminal = FakeTerminal::new(80, 24, b"");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap();
        runtime.block_on(async {
            let terminal: Arc<dyn Terminal> = terminal.clone();
            terminal.write_remote(b"one".to_vec()).await.unwrap();
            terminal.write_remote(b"two".to_vec()).await.unwrap();
        });
        assert_eq!(&*terminal.writes.lock().unwrap(), b"onetwo");
    }

    #[test]
    fn each_device_may_hold_sixteen_streams() {
        let limits = Arc::new(StreamLimits::default());
        let slots: Vec<_> = (0..MAX_STREAMS_PER_DEVICE)
            .map(|_| limits.acquire("phone").expect("a slot"))
            .collect();
        assert!(limits.acquire("phone").is_none());
        assert!(
            limits.acquire("laptop").is_some(),
            "other devices unaffected"
        );
        drop(slots);
        assert!(limits.acquire("phone").is_some(), "dropping frees slots");
    }

    #[test]
    fn only_listed_ptys_can_be_streamed() {
        let tabs = TabRegistry::new();
        assert!(!listed(&tabs, 7));
    }
}
