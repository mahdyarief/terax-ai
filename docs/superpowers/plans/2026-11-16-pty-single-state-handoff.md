# PTY Single-State Handoff Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace terminal respawn during every cross-window tab or Space move with one live PTY state attached by exactly one active host window at a time.

**Architecture:** A pure Rust attachment state machine and bounded replay log own the transaction. `PtyState` keeps the child process alive independently of any WebView, Tauri commands are thin wrappers, and frontend windows are disposable views. The target reconstructs a fresh Ghostty model from ordered output/resize replay before live streaming resumes. A failed transaction rolls back with a fresh source token and epoch.

**Tech Stack:** Rust 2021/Tauri v2, portable-pty, React 19/TypeScript, Zustand, Vitest (`node`, no jsdom), pnpm 11.9.0, Vite, Tailwind, Ghostty Wasm.

## Global Constraints

- Approved spec: `docs/superpowers/specs/2026-11-16-pty-single-state-handoff-design.md`.
- Keep `1 Space = 1 host window`; one PTY has exactly one active attachment.
- No daemon, detached OS process, endpoint adoption, read-only mirror, multi-writer terminal, or survival across application update/restart.
- No Ghostty Wasm change and no full native emulator serializer in this phase.
- Replay starts at sequence zero; if the bounded log cannot reconstruct state, `pty_prepare_handoff` fails before any detach.
- Handoff failure rolls back to the source with a fresh token and epoch; silent respawn is forbidden.
- All `u64` values crossing Tauri IPC are decimal strings on the TypeScript side.
- Existing `pty_close` is only for explicit user close or real application shutdown.
- Keep the `space-*` guard: only `main` may open another Space window.
- Functional core logic is pure Rust or pure TypeScript; Tauri commands and React hooks are thin shells.
- Every production change has a failing test first. Tests assert observable state or behavior, not mock call counts.
- Package manager and scripts: `pnpm 11.9.0`, `pnpm lint` (`biome lint ./src`), `pnpm check-types` (`tsc --noEmit`), `pnpm test` (`vitest run`), `pnpm build` (`tsc && vite build`).
- Rust checks: `cargo clippy --all-targets --locked -- -D warnings`, `cargo nextest run --locked`, fallback `cargo test --locked`; `rustfmt` is not installed locally.
- Fast app feedback: `pnpm exec tauri build --debug --no-bundle`; no release installer in implementation verification.

---

## File Structure

- Create `src-tauri/src/modules/pty/attachment.rs`: pure state machine for `Attached → Preparing → Detached → Attaching → Attached`, token/epoch authorization, and idempotent commit/rollback.
- Create `src-tauri/src/modules/pty/replay.rs`: bounded sequence-zero replay log for raw output and resize events.
- Create `src/modules/terminal/lib/ptyHandoff.ts`: shared TypeScript payload types, u64 decimal helpers, and pure transaction payload builders.
- Create tests beside each new module and extend existing colocated test files.
- Modify `src-tauri/src/modules/pty/{mod.rs,session.rs,hub.rs,queue.rs,wire.rs,gate.rs,protocol.rs}`: attach sessions to leases, record replay, enforce epoch in ACK, and expose commands.
- Modify `src-tauri/src/lib.rs`: register the five new PTY commands in `invoke_handler`.
- Modify `src/modules/spaces/lib/{serialize.ts,crossWindowSync.ts}`: carry stable PTY attachment/checkpoint metadata in `spaces:tabs-moved`.
- Modify `src/modules/terminal/lib/{pty-bridge.ts,PtyOutputHub.ts,PtyOutputReceiver.ts,ptyWire.ts,terminalSessionApi.ts}`: attach/detach views and carry epoch on stream requests.
- Modify `src/modules/terminal/ghostty/useGhosttyTerminalSession.ts`: detach view without `pty_close`, replay into a fresh model, and attach live output.
- Modify `src/modules/tabs/lib/useTabs.ts` and `src/app/App.tsx`: route every cross-window move through one transaction; keep explicit close on dispose.
- Modify `docs/multi-window-space-plan.md`: mark live PTY transfer as activated and replace the terminal-restart warning copy.

## Shared Test Helper Contract

Tambahkan helper berikut hanya di bawah `#[cfg(test)]` saat Task 3, 4, 5, atau 11 pertama membutuhkannya. Helper ini membungkus API produksi; tidak ada helper yang menyimpan state tersembunyi.

```rust
#[cfg(test)]
impl Session {
    fn attachment_token_for_test(&self) -> u64 {
        self.attachment.read().unwrap().token
    }

    fn queue_closed(&self) -> bool {
        self.queue.is_closed()
    }

    fn replay_output_for_test(&self, bytes: &[u8]) {
        self.replay.lock().unwrap().push_output(bytes).unwrap();
    }

    fn replay_resize_for_test(&self, cols: u16, rows: u16) {
        self.replay.lock().unwrap().push_resize(cols, rows).unwrap();
    }
}

#[cfg(test)]
fn reset_webview_for_test(state: &Arc<PtyState>, label: &str) {
    state.reset_webview(label);
}

#[cfg(test)]
fn cleanup_webview_for_test(
    state: &Arc<PtyState>,
    label: &str,
    reason: CleanupReason,
) -> usize {
    cleanup_webview(state, label, reason)
}

#[cfg(test)]
fn prepare_for_test(
    state: &Arc<PtyState>,
    id: &str,
    ids: &[u32],
    to: &str,
) -> Result<Vec<PtyHandoffCheckpoint>, String> {
    handoff_prepare(
        state,
        id.parse().expect("decimal handoff id"),
        ids,
        to,
        "main",
    )
}
```

`handoff_prepare` didefinisikan pada Task 5 dengan signature `pub(super) fn handoff_prepare(state: &PtyState, id: HandoffId, ids: &[u32], to: &str, from: &str) -> Result<Vec<PtyHandoffCheckpoint>, String>`.

### Task 1: Pure Attachment State Machine

**Files:**
- Create: `src-tauri/src/modules/pty/attachment.rs`
- Test: `src-tauri/src/modules/pty/attachment.rs` (`#[cfg(test)] mod tests`)
- Modify: `src-tauri/src/modules/pty/mod.rs:1-16`

**Interfaces:**
- Consumes: none.
- Produces:
  - `pub(super) type HandoffId = u64;`
  - `pub(super) type LeaseEpoch = u64;`
  - `pub(super) type AttachToken = u64;`
  - `pub(super) enum AttachmentPhase { Attached, Preparing, Detached, Attaching }`
  - `pub(super) struct Attachment { host: Arc<str>, token: AttachToken, epoch: LeaseEpoch, phase: AttachmentPhase, handoff: Option<Handoff>, checkpoint: u64 }`
  - `pub(super) struct Handoff { id: HandoffId, from: Arc<str>, to: Arc<str>, checkpoint: u64, source: Attachment }`
  - `pub(super) enum TransitionError { WrongPhase, WrongToken, WrongEpoch, DuplicateHandoff, UnknownHandoff, WrongWindow }`
  - `impl Attachment { pub(super) fn new(host: Arc<str>, token: AttachToken) -> Self; pub(super) fn prepare(&self, handoff: Handoff) -> Result<Attachment, TransitionError>; pub(super) fn detach(&self, id: HandoffId, token: AttachToken, epoch: LeaseEpoch) -> Result<Attachment, TransitionError>; pub(super) fn attach(&self, id: HandoffId, to: Arc<str>, token: AttachToken, epoch: LeaseEpoch) -> Result<Attachment, TransitionError>; pub(super) fn commit(&self, id: HandoffId, token: AttachToken, epoch: LeaseEpoch) -> Result<Attachment, TransitionError>; pub(super) fn rollback(&self, id: HandoffId, token: AttachToken, epoch: LeaseEpoch) -> Result<Attachment, TransitionError>; pub(super) fn authorizes(&self, host: &str, token: AttachToken, epoch: LeaseEpoch) -> bool }`

- [ ] **Step 1: Write the failing test**

Tambahkan ke dalam `attachment.rs` sebelum implementasi:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn prepare_then_detach_then_attach_then_commit_moves_the_host() {
        let source = Attachment::new(Arc::from("main"), 11);
        let handoff = Handoff {
            id: 7,
            from: Arc::from("main"),
            to: Arc::from("space-a"),
            checkpoint: 42,
            source: source.clone(),
        };
        let preparing = source.prepare(handoff).expect("prepare");
        let detached = preparing.detach(7, 11, 0).expect("detach");
        let attaching = detached
            .attach(7, Arc::from("space-a"), 12, 1)
            .expect("attach");
        let committed = attaching.commit(7, 12, 1).expect("commit");
        assert_eq!(committed.host.as_ref(), "space-a");
        assert!(committed.authorizes("space-a", 12, 1));
        assert!(!committed.authorizes("main", 11, 0));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd src-tauri && cargo test --lib modules::pty::attachment::tests::prepare_then_detach_then_attach_then_commit_moves_the_host -- --exact`
Expected: FAIL dengan `E0432` / unresolved import atau `E0433`, karena modul dan tipe belum ada. Jika error terjadi karena modul belum dideklarasikan, itu RED yang benar.

- [ ] **Step 3: Write minimal implementation**

Buat `attachment.rs` dengan `#[derive(Clone)]` pada `Attachment` dan `Handoff`, enum/error di atas, dan transisi murni:
`prepare` hanya dari `Attached`; `detach` hanya dari `Preparing`; `attach` hanya dari `Detached`; `commit` hanya dari `Attaching`; `rollback` dari `Preparing`, `Detached`, atau `Attaching` mengembalikan `Handoff.source` dengan token/epoch baru dari parameter. `prepare` menolak `from != host`, `to == host`, dan `id` yang sama pada handoff aktif. Tambahkan `mod attachment;` pada `src-tauri/src/modules/pty/mod.rs:1-16`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cd src-tauri && cargo test --lib modules::pty::attachment::tests::prepare_then_detach_then_attach_then_commit_moves_the_host -- --exact`
Expected: PASS tanpa warning.

- [ ] **Step 5: Add rollback and authorization tests**

Tambahkan test berikut ke modul yang sama, jalankan, lalu implementasikan penolakan yang belum ada:

```rust
#[test]
fn rollback_restores_source_with_a_fresh_token_and_epoch() {
    let source = Attachment::new(Arc::from("main"), 11);
    let preparing = source
        .clone()
        .prepare(Handoff { id: 7, from: Arc::from("main"), to: Arc::from("space-a"), checkpoint: 42, source })
        .expect("prepare");
    let restored = preparing.rollback(7, 13, 2).expect("rollback");
    assert_eq!(restored.host.as_ref(), "main");
    assert!(restored.authorizes("main", 13, 2));
    assert!(!restored.authorizes("main", 11, 0));
}

#[test]
fn stale_token_or_epoch_is_rejected() {
    let attachment = Attachment::new(Arc::from("main"), 11);
    assert!(attachment.authorizes("main", 11, 0));
    assert!(!attachment.authorizes("main", 99, 0));
    assert!(!attachment.authorizes("main", 11, 1));
    assert!(!attachment.authorizes("space-a", 11, 0));
}
```

Run: `cd src-tauri && cargo test --lib modules::pty::attachment::tests -- --nocapture`
Expected: PASS setelah kedua perilaku diimplementasikan.

- [ ] **Step 6: Add idempotency and invalid-transition tests**

Tambahkan test untuk `commit` dua kali dengan `handoffId` sama, `attach` dari `Attached`, `detach` memakai token lama, dan `prepare` dengan `from` salah. Implementasikan agar duplicate `commit` mengembalikan state tanpa error, sedangkan transisi lain mengembalikan `TransitionError` yang tepat. Jalankan: `cd src-tauri && cargo test --lib modules::pty::attachment::tests`. Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/modules/pty/attachment.rs src-tauri/src/modules/pty/mod.rs
git commit -m "feat(pty): add attachment handoff state machine"
```

### Task 2: Bounded Sequence-Zero Replay Log

**Files:**
- Create: `src-tauri/src/modules/pty/replay.rs`
- Test: `src-tauri/src/modules/pty/replay.rs` (`#[cfg(test)] mod tests`)
- Modify: `src-tauri/src/modules/pty/mod.rs:1-16`

**Interfaces:**
- Consumes: `attachment` types from Task 1.
- Produces:
  - `pub(super) const MAX_REPLAY_BYTES: usize = 8 * 1024 * 1024;`
  - `pub(super) const MAX_REPLAY_EVENTS: usize = 65_536;`
  - `pub(super) enum ReplayEvent { Output { bytes: Vec<u8> }, Resize { cols: u16, rows: u16 } }`
  - `pub(super) struct ReplayLog { events: Vec<ReplayEvent>, bytes: usize, complete: bool, creation_cols: u16, creation_rows: u16 }`
  - `impl ReplayLog { pub(super) fn new(cols: u16, rows: u16) -> Self; pub(super) fn push_output(&mut self, bytes: &[u8]) -> Result<u64, ReplayError>; pub(super) fn push_resize(&mut self, cols: u16, rows: u16) -> Result<u64, ReplayError>; pub(super) fn sequence(&self) -> u64; pub(super) fn is_complete(&self) -> bool; pub(super) fn replay_all(&self) -> Result<Vec<ReplayEvent>, ReplayError>; pub(super) fn replay_after(&self, checkpoint: u64) -> Result<Vec<ReplayEvent>, ReplayError> }`
  - `pub(super) enum ReplayError { Truncated, InvalidCheckpoint }`

- [ ] **Step 1: Write the failing test**

```rust
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
        assert!(matches!(events[1], ReplayEvent::Resize { cols: 100, rows: 30 }));
        assert!(matches!(&events[2], ReplayEvent::Output { bytes } if bytes == b"two"));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd src-tauri && cargo test --lib modules::pty::replay::tests::replay_preserves_output_and_resize_order_from_sequence_zero -- --exact`
Expected: FAIL karena `replay.rs` belum dideklarasikan atau tipe belum ada.

- [ ] **Step 3: Write minimal implementation**

Buat `replay.rs` dengan event log yang menyimpan output bytes dan resize events secara berurutan. `sequence()` adalah jumlah byte output ditambah satu per resize event. `push_output` menolak payload kosong dan menandai `complete = false` bila `MAX_REPLAY_BYTES` atau `MAX_REPLAY_EVENTS` terlampaui; `push_resize` memakai aturan event limit yang sama. `replay_all()` menolak bila `complete == false` dan mengembalikan seluruh event dari sequence nol. `replay_after(checkpoint)` mengembalikan event baru setelah boundary sequence; checkpoint harus lebih kecil atau sama dengan `sequence()`. Simpan `sequence()` pada setiap event agar boundary dapat dihitung tanpa menebak panjang payload. Tambahkan `mod replay;` di `mod.rs`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cd src-tauri && cargo test --lib modules::pty::replay::tests::replay_preserves_output_and_resize_order_from_sequence_zero -- --exact`
Expected: PASS.

- [ ] **Step 5: Add overflow and boundary tests**

Tambahkan test berikut, jalankan RED bila perlu, lalu lengkapi implementasi:

```rust
#[test]
fn overflow_marks_the_log_incomplete_and_prevents_replay() {
    let mut log = ReplayLog::new(80, 24);
    let payload = vec![b'x'; MAX_REPLAY_BYTES + 1];
    let _ = log.push_output(&payload);
    assert!(!log.is_complete());
    assert!(matches!(log.replay_all(), Err(ReplayError::Truncated)));
}

#[test]
fn invalid_checkpoint_is_rejected() {
    let log = ReplayLog::new(80, 24);
    assert!(matches!(log.replay_after(1), Err(ReplayError::InvalidCheckpoint)));
}
```

Run: `cd src-tauri && cargo test --lib modules::pty::replay::tests`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/modules/pty/replay.rs src-tauri/src/modules/pty/mod.rs
git commit -m "feat(pty): add bounded replay log"
```

### Task 3: Session Owns an Attachment Instead of a WebView Label

**Files:**
- Modify: `src-tauri/src/modules/pty/session.rs:60-76,93-110,219-230`
- Modify: `src-tauri/src/modules/pty/mod.rs:38-54,172-240`
- Test: `src-tauri/src/modules/pty/mod.rs` (`#[cfg(all(test, unix))] mod tests`, existing)

**Interfaces:**
- Consumes: `Attachment`, `AttachToken`, `LeaseEpoch` from Task 1.
- Produces:
  - `Session.attachment: RwLock<Attachment>`
  - `PaneTarget { pty_id: u32, host: Arc<str>, link: Arc<agent_detect::AgentLink> }`
  - `impl Session { pub(super) fn host(&self) -> Arc<str>; pub(super) fn authorizes(&self, host: &str, token: AttachToken, epoch: LeaseEpoch) -> bool; pub(super) fn replace_attachment(&self, attachment: Attachment) }`
  - `pty_open(...) -> Result<u32, String>` now creates `Attachment::new(Arc::from(webview.label()), initial_token)`.

- [ ] **Step 1: Write the failing test**

Tambahkan ke modul test PTY yang sudah ada:

```rust
#[test]
fn pty_write_uses_attachment_host_not_the_callers_webview_label() {
    let state = Arc::new(PtyState::default());
    let session = open(&state, 1, "main");
    assert!(session.authorizes("main", session.attachment_token_for_test(), 0));
    assert!(!session.authorizes("space-a", session.attachment_token_for_test(), 0));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd src-tauri && cargo test --lib modules::pty::tests::pty_write_uses_attachment_host_not_the_callers_webview_label -- --exact`
Expected: FAIL karena `attachment`/`authorizes` belum ada.

- [ ] **Step 3: Write minimal implementation**

Ganti field `owner: Arc<str>` dengan `attachment: RwLock<Attachment>`. Tambahkan helper `host()`, `authorizes()`, dan `replace_attachment()`. Di `pty_open`, buat `Attachment::new(Arc::from(webview.label()), token)` memakai token unik dari `PtyState` (ambil dari `next_token` yang ditambahkan ke `PtyState`). Di `pty_write`, `pty_resize`, dan `pty_session_shell`, ganti pemeriksaan `session.owner` dengan `session.authorizes(webview.label(), token, epoch)`; parameter `token`/`epoch` ditambahkan ke command pada Task 5.

- [ ] **Step 4: Run test to verify it passes**

Run: `cd src-tauri && cargo test --lib modules::pty::tests::pty_write_uses_attachment_host_not_the_callers_webview_label -- --exact`
Expected: PASS.

- [ ] **Step 5: Update remaining owner checks**

Grep `session.owner` di `src-tauri/src/modules/pty` dan ganti setiap pemakaian dengan `session.host()` atau `session.authorizes(...)` sesuai konteks cleanup. Tambahkan test bahwa `reset_webview`/`destroy_webview` hanya melepas attachment dan tidak menutup output queue:

```rust
#[test]
fn reset_webview_detaches_but_keeps_the_child_alive() {
    let state = Arc::new(PtyState::default());
    let session = open(&state, 1, "main");
    reset_webview_for_test(&state, "main");
    assert!(!session.queue_closed());
}
```

Run: `cd src-tauri && cargo test --lib modules::pty::tests`
Expected: seluruh test PTY lama tetap PASS.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/modules/pty/session.rs src-tauri/src/modules/pty/mod.rs
git commit -m "refactor(pty): bind sessions to attachment leases"
```

### Task 4: Record Every Output and Resize in the Replay Log

**Files:**
- Modify: `src-tauri/src/modules/pty/session.rs` (reader loop and resize path)
- Modify: `src-tauri/src/modules/pty/mod.rs:303-330` (`pty_resize`)
- Test: `src-tauri/src/modules/pty/mod.rs` (existing test module)

**Interfaces:**
- Consumes: `ReplayLog`, `ReplayEvent` from Task 2.
- Produces:
  - `Session.replay: Mutex<ReplayLog>`
  - `Session.replay_all() -> Result<Vec<ReplayEvent>, ReplayError>`
  - `Session.replay_after(checkpoint: u64) -> Result<Vec<ReplayEvent>, ReplayError>`
  - `Session.checkpoint() -> Result<u64, ReplayError>`

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn session_records_output_and_resize_for_replay() {
    let state = Arc::new(PtyState::default());
    let session = open(&state, 1, "main");
    session.replay_output_for_test(b"hello");
    session.replay_resize_for_test(120, 40);
    let events = session.replay_all().expect("replay");
    assert!(matches!(&events[0], ReplayEvent::Output { bytes } if bytes == b"hello"));
    assert!(matches!(events[1], ReplayEvent::Resize { cols: 120, rows: 40 }));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd src-tauri && cargo test --lib modules::pty::tests::session_records_output_and_resize_for_replay -- --exact`
Expected: FAIL karena helper replay belum ada.

- [ ] **Step 3: Write minimal implementation**

Tambahkan `replay: Mutex<ReplayLog>` ke `Session`, inisialisasi dengan `cols`/`rows` dari `pty_open`. Di jalur reader yang menerima chunk dari master PTY, panggil `push_output(bytes)` sebelum/tanpa mengubah `OutputQueue`. Di `pty_resize`, setelah resize berhasil, panggil `push_resize(cols, rows)`. `checkpoint()` mengembalikan `sequence()`; `replay_all` dan `replay_after` meneruskan ke metode `ReplayLog` yang sesuai.

- [ ] **Step 4: Run test to verify it passes**

Run: `cd src-tauri && cargo test --lib modules::pty::tests::session_records_output_and_resize_for_replay -- --exact`
Expected: PASS.

- [ ] **Step 5: Verify existing output tests still pass**

Run: `cd src-tauri && cargo test --lib modules::pty`
Expected: PASS; tidak ada output byte yang hilang atau terkirim dua kali ke `OutputQueue`.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/modules/pty/session.rs src-tauri/src/modules/pty/mod.rs
git commit -m "feat(pty): record ordered terminal replay"
```

### Task 5: Add Handoff Commands and IPC Types

**Files:**
- Modify: `src-tauri/src/modules/pty/mod.rs` (new commands after `pty_stream_open`)
- Modify: `src-tauri/src/lib.rs` (`generate_handler!` list)
- Test: `src-tauri/src/modules/pty/mod.rs` (existing test module)
- Test: `src-tauri/src/lib.rs` (new `#[cfg(test)] mod pty_handoff_tests` if the command list is not directly testable)

**Interfaces:**
- Consumes: attachment state machine and replay from Tasks 1-4.
- Produces:
  - `#[tauri::command] pub async fn pty_prepare_handoff(state: tauri::State<'_, PtyState>, webview: tauri::Webview, handoff_id: String, ids: Vec<u32>, to: String) -> Result<Vec<PtyHandoffCheckpoint>, String>`
  - `#[tauri::command] pub async fn pty_detach(state: tauri::State<'_, PtyState>, webview: tauri::Webview, handoff_id: String, ids: Vec<u32>) -> Result<(), String>`
  - `#[tauri::command] pub async fn pty_attach(state: tauri::State<'_, PtyState>, webview: tauri::Webview, handoff_id: String, ids: Vec<u32>) -> Result<Vec<PtyReplayPayload>, String>` mengeluarkan `attach_token` dan `lease_epoch` baru untuk setiap sesi.
  - `#[tauri::command] pub async fn pty_commit_handoff(state: tauri::State<'_, PtyState>, webview: tauri::Webview, handoff_id: String, leases: Vec<PtyLeaseAck>) -> Result<(), String>` dengan `PtyLeaseAck { pty_id: u32, attach_token: String, lease_epoch: String }`.
  - `#[tauri::command] pub async fn pty_rollback_handoff(state: tauri::State<'_, PtyState>, webview: tauri::Webview, handoff_id: String, ids: Vec<u32>) -> Result<Vec<PtyReplayPayload>, String>` mengeluarkan `attach_token` dan `lease_epoch` baru untuk source.
  - `#[derive(serde::Serialize)] #[serde(rename_all = "camelCase")] pub struct PtyHandoffCheckpoint { pty_id: u32, checkpoint: String, cols: u16, rows: u16, attach_token: String }`
  - `#[derive(serde::Serialize)] #[serde(rename_all = "camelCase")] pub struct PtyReplayPayload { pty_id: u32, cols: u16, rows: u16, attach_token: String, lease_epoch: String, events: Vec<PtyReplayEvent> }`
  - `#[derive(serde::Serialize)] #[serde(tag = "kind", rename_all = "camelCase")] pub enum PtyReplayEvent { Output { data: String }, Resize { cols: u16, rows: u16 } }`

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn prepare_handoff_checkpoint_is_stable_until_detach() {
    let state = Arc::new(PtyState::default());
    let session = open(&state, 1, "main");
    let a = prepare_for_test(&state, "h1", &[1], "space-a").expect("prepare");
    let b = prepare_for_test(&state, "h1", &[1], "space-a").expect("prepare again");
    assert_eq!(a[0].checkpoint, b[0].checkpoint);
    assert!(!session.queue_closed());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd src-tauri && cargo test --lib modules::pty::tests::prepare_handoff_checkpoint_is_stable_until_detach -- --exact`
Expected: FAIL karena helper/command belum ada.

- [ ] **Step 3: Write minimal implementation**

Implementasikan lima command sebagai wrapper tipis di sekitar helper `handoff_prepare`, `handoff_detach`, `handoff_attach`, `handoff_commit`, `handoff_rollback`. Ubah `u64` menjadi decimal `String` sebelum serialisasi. `prepare` mengembalikan checkpoint atau gagal tanpa mengubah attachment. `detach` mengganti attachment ke `Detached`. `attach` memvalidasi target window, memasang attachment baru, dan mengembalikan replay lengkap beserta token/epoch target. `commit` memakai `leases` untuk memastikan target adalah pemegang token/epoch yang benar. `rollback` mengembalikan replay bertambah sejak checkpoint dan token/epoch source yang baru. Semua operasi idempotent berdasarkan `handoffId`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cd src-tauri && cargo test --lib modules::pty::tests::prepare_handoff_checkpoint_is_stable_until_detach -- --exact`
Expected: PASS.

- [ ] **Step 5: Register commands and add uniqueness tests**

Tambahkan kelima command ke `generate_handler!` di `src-tauri/src/lib.rs`. Tambahkan test bahwa `detach` dengan token salah gagal, `attach` ke window yang salah gagal, dan `rollback` setelah `commit` tidak mengubah host lagi. Jalankan: `cd src-tauri && cargo test --lib modules::pty && cargo check --all-targets`.
Expected: PASS dan command terdaftar.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/modules/pty/mod.rs src-tauri/src/lib.rs
git commit -m "feat(pty): add cross-window handoff commands"
```

### Task 6: Enforce Token and Epoch in Hub Stream Requests

**Files:**
- Modify: `src-tauri/src/modules/pty/wire.rs` (`StreamRequest`)
- Modify: `src-tauri/src/modules/pty/gate.rs`
- Modify: `src-tauri/src/modules/pty/hub.rs:217-230,348-367`
- Test: existing `gate.rs`, `hub.rs`, `wire.rs` test modules

**Interfaces:**
- Consumes: `AttachToken`, `LeaseEpoch` from Task 1.
- Produces:
  - `StreamRequest { token: u64, epoch: u64, acks: Vec<Ack> }`
  - `Hub::Command::OpenStream { label, token: u64, epoch: u64, reply }`
  - `Hub::Command::Request { label, endpoint, request, reply }` unchanged shape but request now carries epoch.
  - `Command::OpenStream` publishes exactly the token/epoch pair into `StreamTokens`.

- [ ] **Step 1: Write the failing test**

Tambahkan ke test `gate.rs`:

```rust
#[test]
fn a_valid_token_with_a_stale_epoch_is_forbidden() {
    let hub = Hub::new();
    let token = open_stream(&hub, "main", 21, 1);
    let body = encode_request(token, 1, &[Ack { id: 1, bytes: 0 }]);
    assert_eq!(call(&hub, Method::POST, "/poll", &body), (200, Vec::new()));
    let stale = encode_request(token, 0, &[Ack { id: 1, bytes: 0 }]);
    assert_eq!(call(&hub, Method::POST, "/poll", &stale).0, 403);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd src-tauri && cargo test --lib modules::pty::gate::tests::a_valid_token_with_a_stale_epoch_is_forbidden -- --exact`
Expected: FAIL karena `open_stream`/`encode_request` belum menerima epoch atau `StreamTokens` belum menyimpannya.

- [ ] **Step 3: Write minimal implementation**

Perluas format wire request dengan 8 byte epoch setelah token; bump konstanta header dan validasi panjang. Simpan `(token, epoch)` per label di `StreamTokens`, publish keduanya pada `OpenStream`, dan periksa keduanya di `admit` serta `HubCore::request`. Perbarui semua test helper lama agar memakai epoch `0`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cd src-tauri && cargo test --lib modules::pty::gate::tests`
Expected: PASS.

- [ ] **Step 5: Verify ACK isolation and hub tests**

Tambahkan test bahwa ACK dari label lain tetap diabaikan, ACK dengan epoch lama ditolak, dan `OpenStream` baru mencabut token/epoch sebelumnya. Jalankan: `cd src-tauri && cargo test --lib modules::pty::hub && cargo test --lib modules::pty::wire`.
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/modules/pty/wire.rs src-tauri/src/modules/pty/gate.rs src-tauri/src/modules/pty/hub.rs
git commit -m "feat(pty): enforce attachment epoch on streams"
```

### Task 7: Shared TypeScript Handoff Types and u64 Helpers

**Files:**
- Create: `src/modules/terminal/lib/ptyHandoff.ts`
- Test: `src/modules/terminal/lib/ptyHandoff.test.ts`

**Interfaces:**
- Consumes: backend response shapes from Task 5.
- Produces:
  - `export type PtyHandoffCheckpoint = { ptyId: number; checkpoint: string; cols: number; rows: number; attachToken: string };`
  - `export type PtyReplayEvent = { kind: "output"; data: string } | { kind: "resize"; cols: number; rows: number };`
  - `export type PtyReplayPayload = { ptyId: number; cols: number; rows: number; attachToken: string; leaseEpoch: string; events: PtyReplayEvent[] };`
  - `export type PtyAttachmentRef = { ptyId: number; checkpoint: string; cols: number; rows: number };`
  - `export function buildPrepareHandoff(handoffId: string, ids: number[], to: string): { handoffId: string; ids: number[]; to: string }`
  - `export function buildDetachHandoff(handoffId: string, ids: number[]): { handoffId: string; ids: number[] }`
  - `export function buildAttachHandoff(handoffId: string, ids: number[]): { handoffId: string; ids: number[] }`
  - `export function buildCommitHandoff(handoffId: string, leases: Array<{ ptyId: number; attachToken: string; leaseEpoch: string }>): { handoffId: string; leases: Array<{ ptyId: number; attachToken: string; leaseEpoch: string }> }`
  - `export function buildRollbackHandoff(handoffId: string, ids: number[]): { handoffId: string; ids: number[] }`
  - `export function decodeReplayOutput(data: string): Uint8Array`
  - `export function applyReplayEvents(model: { write(bytes: Uint8Array): void; resize(cols: number, rows: number): void }, events: PtyReplayEvent[]): void`

- [ ] **Step 1: Write the failing test**

```ts
import { describe, expect, it, vi } from "vitest";
import {
  applyReplayEvents,
  buildPrepareHandoff,
  decodeReplayOutput,
} from "./ptyHandoff";

describe("ptyHandoff", () => {
  it("keeps u64 values as decimal strings in the prepare payload", () => {
    expect(buildPrepareHandoff("18446744073709551615", [7], "space-a")).toEqual({
      handoffId: "18446744073709551615",
      ids: [7],
      to: "space-a",
    });
  });

  it("decodes base64 output and applies resize in order", () => {
    const written: Uint8Array[] = [];
    const resized: Array<[number, number]> = [];
    applyReplayEvents(
      { write: (bytes) => written.push(bytes), resize: (cols, rows) => resized.push([cols, rows]) },
      [
        { kind: "output", data: btoa("hello") },
        { kind: "resize", cols: 100, rows: 30 },
      ],
    );
    expect(new TextDecoder().decode(written[0])).toBe("hello");
    expect(resized).toEqual([[100, 30]]);
    expect(decodeReplayOutput(btoa("x"))).toEqual(new Uint8Array([120]));
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `pnpm exec vitest run src/modules/terminal/lib/ptyHandoff.test.ts`
Expected: FAIL karena modul belum ada.

- [ ] **Step 3: Write minimal implementation**

Buat `ptyHandoff.ts` dengan tipe di atas dan fungsi murni. `decodeReplayOutput` memakai `atob` lalu mengonversi setiap karakter ke byte. `applyReplayEvents` memanggil `model.write`/`model.resize` sesuai urutan event. Tidak ada akses Tauri atau React di file ini.

- [ ] **Step 4: Run test to verify it passes**

Run: `pnpm exec vitest run src/modules/terminal/lib/ptyHandoff.test.ts`
Expected: PASS.

- [ ] **Step 5: Add invalid input test**

Tambahkan test bahwa `applyReplayEvents` melempar error ketika `resize` memiliki nilai negatif atau nol, lalu implementasikan validasi di boundary ini. Jalankan ulang test. Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src/modules/terminal/lib/ptyHandoff.ts src/modules/terminal/lib/ptyHandoff.test.ts
git commit -m "feat(terminal): add PTY handoff payload helpers"
```

### Task 8: Serialize Stable PTY Attachment Metadata

**Files:**
- Modify: `src/modules/spaces/lib/serialize.ts:14-27,42-55,91-98,144-166`
- Modify: `src/modules/tabs/lib/useTabs.ts` (`TerminalTab` type, around the tab type definitions)
- Test: `src/modules/spaces/lib/serialize.test.ts`

**Interfaces:**
- Consumes: `PtyAttachmentRef` from Task 7.
- Produces:
  - `SerializedNode` leaf variant: `{ kind: "leaf"; cwd?: string; active?: boolean; ptyAttachment?: PtyAttachmentRef }`
  - `SerializedTab` terminal variant: `{ kind: "terminal"; tree: SerializedNode; blocks?: boolean; customTitle?: string }`
  - `PaneLeaf` gains optional `ptyAttachment?: PtyAttachmentRef`; no new field is added to `TerminalTab`.
  - `hydrateTabs(serialized, spaceId, allocId, opts?: { keepPty?: boolean })`; when `keepPty` is true, hydrate preserves `ptyAttachment` and sets `cold: false`, otherwise cold behavior is unchanged.

- [ ] **Step 1: Write the failing test**

Tambahkan ke `serialize.test.ts`:

```ts
it("preserves stable PTY attachment metadata during handoff hydration", () => {
  const tab = term({
    paneTree: {
      kind: "leaf",
      id: 2,
      cwd: "/a",
      ptyAttachment: { ptyId: 41, checkpoint: "128", cols: 80, rows: 24 },
    },
  });
  const [serialized] = serializeTabs([tab]);
  const [hydrated] = hydrateTabs([serialized], "s1", counter(), { keepPty: true });
  const leaf = hydrated.kind === "terminal" ? hydrated.paneTree : null;
  expect(leaf).toMatchObject({
    kind: "leaf",
    ptyAttachment: { ptyId: 41, checkpoint: "128", cols: 80, rows: 24 },
  });
  expect(hydrated.kind === "terminal" && hydrated.cold).toBe(false);
});

it("keeps legacy state cold when no PTY attachment exists", () => {
  const [serialized] = serializeTabs([term({})]);
  const [hydrated] = hydrateTabs([serialized], "s1", counter());
  expect(hydrated.kind === "terminal" && hydrated.cold).toBe(true);
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `pnpm exec vitest run src/modules/spaces/lib/serialize.test.ts`
Expected: FAIL karena properti `ptyAttachment` dibuang dan `hydrateTabs` belum menerima opsi `keepPty`.

- [ ] **Step 3: Write minimal implementation**

Tambahkan tipe `PtyAttachmentRef` pada leaf dan teruskan lewat `serializeNode`/`hydrateTree`. `hydrateTabs` menerima opsi `keepPty`; bila `true` dan leaf memiliki `ptyAttachment`, set `cold: false`, pertahankan metadata, dan jangan membuat ID PTY baru. Bila metadata tidak ada, jalur lama tetap `cold: true`.

- [ ] **Step 4: Run test to verify it passes**

Run: `pnpm exec vitest run src/modules/spaces/lib/serialize.test.ts`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/modules/spaces/lib/serialize.ts src/modules/spaces/lib/serialize.test.ts src/modules/tabs/lib/useTabs.ts
git commit -m "feat(spaces): serialize stable PTY attachment refs"
```

### Task 9: Attach and Detach Ghostty Views Without Closing the PTY

**Files:**
- Modify: `src/modules/terminal/lib/pty-bridge.ts:102-229`
- Modify: `src/modules/terminal/ghostty/useGhosttyTerminalSession.ts:432-540,838-880`
- Modify: `src/modules/terminal/lib/PtyOutputHub.ts:120-150`
- Test: `src/modules/terminal/lib/pty-bridge.test.ts`
- Test: `src/modules/terminal/ghostty/replaceSessionSurface.test.ts`, `sessionInitialization.test.ts`

**Interfaces:**
- Consumes: `PtyReplayPayload`, `PtyHandoffCheckpoint` from Task 7; backend attach/detach commands from Task 5.
- Produces:
  - `PtySession` gains `releaseView(): void` and `identity(): { id: number; token: string; epoch: string }`. `releaseView()` stops local stream polling and unregisters handlers only; backend detach happens once in `moveTabsAcrossWindows`.
  - `openPty(..., opts?: { attach?: { ptyId: number; replay: PtyReplayPayload } }): Promise<PtySession>`. The target first invokes `pty_attach` in its own window, then passes the returned replay/lease to `openPty`.
  - `export function detachGhosttyView(leafId: number): boolean` disposes model/surface/input but never calls `pty.close()`.
  - `export function attachGhosttyView(leafId: number, replay: PtyReplayPayload): Promise<boolean>` builds a new model, applies replay, then starts live stream.
  - `PtyOutputHub.register(id, receiver, onExit, lease: { token: string; epoch: string }): PtyStreamHandle`, replacing the current 3-argument signature at `src/modules/terminal/lib/PtyOutputHub.ts:120-144`. Update every existing `register` call and test helper to pass the active lease; tests that do not exercise handoff pass `{ token: "0", epoch: "0" }`.

- [ ] **Step 1: Write the failing test**

Tambahkan ke `pty-bridge.test.ts`:

```ts
it("releases a local view without invoking pty_close", async () => {
  const invokeMock = vi.fn().mockResolvedValue(41);
  vi.doMock("@tauri-apps/api/core", () => ({ invoke: invokeMock, convertFileSrc: vi.fn() }));
  const session = await openPty(80, 24, { onData: vi.fn() });
  session.releaseView();
  expect(invokeMock).not.toHaveBeenCalledWith("pty_close", expect.anything());
  expect(invokeMock).not.toHaveBeenCalledWith("pty_detach", expect.anything());
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `pnpm exec vitest run src/modules/terminal/lib/pty-bridge.test.ts`
Expected: FAIL karena `detach` belum ada pada `PtySession`.

- [ ] **Step 3: Write minimal implementation**

Pisahkan `releaseHandlers()` dari `pty_close`. Tambahkan `releaseView()` yang hanya memanggil `releaseHandlers()`. `close()` tetap memanggil `pty_close`. Backend detach dilakukan sekali oleh `moveTabsAcrossWindows`, bukan oleh setiap `PtySession`. Pada `useGhosttyTerminalSession.ts`, ekstrak cleanup view dari `disposeGhosttySession` menjadi `detachGhosttyView`; `disposeGhosttySession` memanggil `detachGhosttyView` lalu `pty.close()`.

- [ ] **Step 4: Run test to verify it passes**

Run: `pnpm exec vitest run src/modules/terminal/lib/pty-bridge.test.ts src/modules/terminal/ghostty/replaceSessionSurface.test.ts src/modules/terminal/ghostty/sessionInitialization.test.ts`
Expected: PASS.

- [ ] **Step 5: Add attach replay test**

Tambahkan test bahwa `attachGhosttyView` memanggil `model.write` untuk setiap event `output` dan `model.resize` untuk setiap event `resize` sebelum membuka stream live. Implementasikan dengan `applyReplayEvents` dari Task 7. Jalankan test. Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src/modules/terminal/lib/pty-bridge.ts src/modules/terminal/lib/PtyOutputHub.ts src/modules/terminal/ghostty/useGhosttyTerminalSession.ts src/modules/terminal/lib/pty-bridge.test.ts src/modules/terminal/ghostty/replaceSessionSurface.test.ts src/modules/terminal/ghostty/sessionInitialization.test.ts
git commit -m "feat(terminal): detach and attach PTY views without respawn"
```

### Task 10: Route All Cross-Window Moves Through One Transaction

**Files:**
- Create: `src/modules/spaces/lib/moveTabsAcrossWindows.ts`
- Modify: `src/modules/tabs/lib/useTabs.ts:1528-1620`
- Modify: `src/app/App.tsx:1283-1331,1434-1533`
- Modify: `src/modules/spaces/lib/crossWindowSync.ts:29-40`
- Test: `src/modules/spaces/lib/moveTabsAcrossWindows.test.ts`
- Test: `src/modules/spaces/lib/crossWindowSync.test.ts`
- Test: `src/modules/tabs/lib/useTabs.transfer.test.ts`

**Interfaces:**
- Consumes: all prior tasks.
- Produces:
  - `export type TabsMovedPayload = { spaceId: string; fromWindow: string; toWindow: string; handoffId: string; tabs: SerializedTab[]; checkpoints: PtyHandoffCheckpoint[] }`
  - `export type TabsAttachedPayload = { handoffId: string; replays: PtyReplayPayload[]; error?: string }`
  - `export const SPACE_TABS_ATTACHED_EVENT = "spaces:tabs-attached"`
  - `export async function moveTabsAcrossWindows({ handoffId, tabs, targetWindow, spaceId, invoke, emitTo, waitForAttach }): Promise<{ ok: true; attached: TabsAttachedPayload } | { ok: false; error: string }>`
  - `waitForAttach(handoffId): Promise<TabsAttachedPayload>` is implemented by the source window as a listener for `spaces:tabs-attached`. The target window performs `pty_attach` because it requires the target's `Webview` handle.
  - `useTabs` return gains `detachTabsForHandoff(spaceId: string, handoffId: string): Promise<PtyHandoffCheckpoint[]>`; existing `detachTabsForSpace` remains for destruction/cleanup and still disposes.
  - `App.tsx` uses `moveTabsAcrossWindows` in `handleOpenSpaceInWindow`, `handleBringSpaceToWindow`, and `handleMoveTab`; on failure it restores `hostWindowLabel` and source tabs.

- [ ] **Step 1: Write the failing test**

Buat `moveTabsAcrossWindows.test.ts`:

```ts
import { describe, expect, it, vi } from "vitest";
import { moveTabsAcrossWindows } from "./moveTabsAcrossWindows";

describe("moveTabsAcrossWindows", () => {
  it("prepares, detaches, emits, waits for target attach, then commits", async () => {
    const calls: string[] = [];
    const invoke = vi.fn(async (cmd: string) => {
      calls.push(cmd);
      if (cmd === "pty_prepare_handoff") return [{ ptyId: 1, checkpoint: "7", cols: 80, rows: 24, attachToken: "1" }];
      return undefined;
    });
    const result = await moveTabsAcrossWindows({
      handoffId: "h1",
      tabs: [{ kind: "terminal", tree: { kind: "leaf", active: true } }],
      targetWindow: "space-a",
      spaceId: "s",
      invoke,
      emitTo: vi.fn(async () => calls.push("emit")),
      waitForAttach: vi.fn(async () => {
        calls.push("attached");
        return {
          handoffId: "h1",
          replays: [{ ptyId: 1, cols: 80, rows: 24, attachToken: "2", leaseEpoch: "1", events: [] }],
        };
      }),
    });
    expect(result.ok).toBe(true);
    expect(calls).toEqual([
      "pty_prepare_handoff",
      "pty_detach",
      "emit",
      "attached",
      "pty_commit_handoff",
    ]);
  });

  it("rolls back and does not emit when detach fails", async () => {
    const invoke = vi.fn(async (cmd: string) => {
      if (cmd === "pty_prepare_handoff") return [{ ptyId: 1, checkpoint: "7", cols: 80, rows: 24, attachToken: "1" }];
      if (cmd === "pty_detach") throw new Error("nope");
      return undefined;
    });
    const emitTo = vi.fn();
    const result = await moveTabsAcrossWindows({
      handoffId: "h2",
      tabs: [{ kind: "terminal", tree: { kind: "leaf", active: true } }],
      targetWindow: "space-a",
      spaceId: "s",
      invoke,
      emitTo,
      waitForAttach: vi.fn(),
    });
    expect(result.ok).toBe(false);
    expect(emitTo).not.toHaveBeenCalled();
    expect(invoke).toHaveBeenCalledWith("pty_rollback_handoff", expect.anything());
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `pnpm exec vitest run src/modules/spaces/lib/moveTabsAcrossWindows.test.ts`
Expected: FAIL karena modul belum ada.

- [ ] **Step 3: Write minimal implementation**

Implementasikan `moveTabsAcrossWindows` sebagai pure source-side coordinator: `pty_prepare_handoff → pty_detach → emitTo(SPACE_TABS_MOVED_EVENT, payload) → waitForAttach(handoffId) → pty_commit_handoff(leases)`. Source tidak memanggil `pty_attach`. Target, pada listener `spaces:tabs-moved`, memanggil `pty_attach` memakai `Webview` miliknya, menghidrasi tab dari `replays`, lalu mengirim `spaces:tabs-attached` berisi `TabsAttachedPayload`. Bila target gagal, ia mengirim `error`, lalu source memanggil `pty_rollback_handoff`. Bila kegagalan terjadi sebelum `emit`, source langsung rollback tanpa emit. Bila kegagalan setelah `emit`, source juga mengirim event koreksi `spaces:tabs-moved` dengan array kosong agar target membuang view parsial. `commit` memakai `buildCommitHandoff` untuk meneruskan token/epoch dari `replays`.

- [ ] **Step 4: Run test to verify it passes**

Run: `pnpm exec vitest run src/modules/spaces/lib/moveTabsAcrossWindows.test.ts`
Expected: PASS.

- [ ] **Step 5: Wire every move path**

Ganti isi `handleOpenSpaceInWindow`, `handleBringSpaceToWindow`, dan cabang lintas-window `handleMoveTab` agar memakai `moveTabsAcrossWindows`. `detachTabsForHandoff` mengumpulkan `ptyAttachment` dari setiap leaf dan memanggil `detachGhosttyView`, bukan `disposeSession`. Ganti pesan konfirmasi “terminal sessions restart there” karena state kini live.

- [ ] **Step 6: Run targeted tests**

Run: `pnpm exec vitest run src/modules/spaces/lib/moveTabsAcrossWindows.test.ts src/modules/spaces/lib/crossWindowSync.test.ts src/modules/tabs/lib/useTabs.transfer.test.ts src/modules/spaces/lib/handoff.test.ts`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add src/modules/spaces/lib/moveTabsAcrossWindows.ts src/modules/spaces/lib/moveTabsAcrossWindows.test.ts src/modules/spaces/lib/crossWindowSync.ts src/modules/tabs/lib/useTabs.ts src/app/App.tsx src/modules/spaces/lib/crossWindowSync.test.ts src/modules/tabs/lib/useTabs.transfer.test.ts
git commit -m "feat(spaces): move tabs across windows with live PTY handoff"
```

### Task 11: Window Cleanup and Source-Disappeared Failure Handling

**Files:**
- Modify: `src-tauri/src/modules/pty/mod.rs` (`reset_webview`, `destroy_webview`, startup cleanup)
- Modify: `src-tauri/src/lib.rs` (window destroyed hooks and startup `pty_close_all` caller, exact lines verified before editing)
- Modify: `src/modules/spaces/lib/moveTabsAcrossWindows.ts`
- Test: `src-tauri/src/modules/pty/mod.rs` test module
- Test: `src/modules/spaces/lib/moveTabsAcrossWindows.test.ts`

**Interfaces:**
- Consumes: attachment state machine from Task 1 and command wrappers from Task 5.
- Produces:
  - `pub fn cleanup_webview(state: &PtyState, label: &str, reason: CleanupReason) -> usize`
  - `pub enum CleanupReason { Navigation, WindowDestroyed, AppShutdown }`
  - `moveTabsAcrossWindows` returns `{ ok: false; error: "source-unavailable" }` when rollback cannot restore the source.

- [ ] **Step 1: Write the failing test**

Tambahkan ke test Rust:

```rust
#[test]
fn window_destroyed_during_handoff_does_not_kill_the_session() {
    let state = Arc::new(PtyState::default());
    let session = open(&state, 1, "main");
    let _ = prepare_for_test(&state, "h1", &[1], "space-a").expect("prepare");
    let count = cleanup_webview_for_test(&state, "main", CleanupReason::WindowDestroyed);
    assert_eq!(count, 0);
    assert!(!session.queue_closed());
}
```

Dan ke test TypeScript:

```ts
it("closes the PTY explicitly when rollback cannot restore the source", async () => {
  const invoke = vi.fn(async (cmd: string) => {
    if (cmd === "pty_prepare_handoff") return [{ ptyId: 1, checkpoint: "7", cols: 80, rows: 24, attachToken: "1" }];
    if (cmd === "pty_detach") throw new Error("source window gone");
    if (cmd === "pty_rollback_handoff") throw new Error("source unavailable");
    return undefined;
  });
  const result = await moveTabsAcrossWindows({
    handoffId: "h3",
    tabs: [{ kind: "terminal", tree: { kind: "leaf", active: true } }],
    targetWindow: "space-a",
    spaceId: "s",
    invoke,
    emitTo: vi.fn(),
  });
  expect(result).toEqual({ ok: false, error: "source-unavailable" });
  expect(invoke).toHaveBeenCalledWith("pty_close", { id: 1 });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd src-tauri && cargo test --lib modules::pty::tests::window_destroyed_during_handoff_does_not_kill_the_session -- --exact && pnpm exec vitest run src/modules/spaces/lib/moveTabsAcrossWindows.test.ts`
Expected: masing-masing FAIL karena cleanup dan `source-unavailable` belum ada.

- [ ] **Step 3: Write minimal implementation**

`CleanupReason::Navigation` dan `WindowDestroyed` hanya mencabut stream token/epoch serta menandai attachment untuk rollback; tidak memanggil `reap`. `AppShutdown` memanggil `pty_close_all`. Jika rollback gagal karena source tidak ada, `moveTabsAcrossWindows` memanggil `pty_close` untuk setiap `ptyId` checkpoint dan mengembalikan error yang terlihat.

- [ ] **Step 4: Run test to verify it passes**

Run: `cd src-tauri && cargo test --lib modules::pty::tests && pnpm exec vitest run src/modules/spaces/lib/moveTabsAcrossWindows.test.ts`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/modules/pty/mod.rs src-tauri/src/lib.rs src/modules/spaces/lib/moveTabsAcrossWindows.ts src/modules/spaces/lib/moveTabsAcrossWindows.test.ts
git commit -m "fix(pty): clean up handoffs without orphan sessions"
```

### Task 12: Regression Guard for Space Window Recursion

**Files:**
- Test: `src/modules/spaces/lib/handoff.test.ts`
- Test: `src/modules/spaces/SpaceSwitcher.test.tsx`
- Test: `src-tauri/src/lib.rs` (`space_window_tests`)
- Modify: only if a guard regressed during Tasks 10-11; otherwise no production edit.

**Interfaces:**
- Consumes: `planSpaceHandoff`, `SpaceSwitcher`, `can_open_space_window`, `open_space_window`, `focus_space_window`.
- Produces: no new API; proves the earlier `427ddef`/`791e72b` behavior remains intact.

- [ ] **Step 1: Write the failing guard test**

Tambahkan ke `handoff.test.ts` bila belum ada:

```ts
it("does not hand off a space from another space window", () => {
  expect(
    planSpaceHandoff({ spaceId: "a", hostWindowLabel: "space-a" }, "space-a"),
  ).toBeNull();
});
```

Pastikan `SpaceSwitcher.test.tsx` tetap memuat assertion bahwa action `Open in new window`, `Focus window`, dan `Bring back to this window` tidak dirender pada `windowLabel = "space-1"`.

- [ ] **Step 2: Run tests to verify they fail correctly**

Run: `pnpm exec vitest run src/modules/spaces/lib/handoff.test.ts src/modules/spaces/SpaceSwitcher.test.tsx`
Expected: PASS sekarang; ini guard regression. Jika salah satu FAIL, perbaiki jalur yang mengubah guard sebelum lanjut.

- [ ] **Step 3: Run Rust window guard tests**

Run: `cd src-tauri && cargo test --lib space_window_tests`
Expected: 4 PASS, termasuk `only_main_can_open_space_windows`.

- [ ] **Step 4: Commit only if code changed**

```bash
git status --short
```

Jika ada perubahan produksi untuk memulihkan guard, commit dengan pesan `fix(spaces): preserve window recursion guard`. Jika tidak ada, jangan buat commit kosong.

### Task 13: Documentation, Full Verification, and Debug Smoke Test

**Files:**
- Modify: `docs/multi-window-space-plan.md:176-182,293-306`
- Modify: `docs/superpowers/plans/2026-11-16-pty-single-state-handoff.md` (checkbox tracking)
- Test: `docs/superpowers/specs/2026-11-16-pty-single-state-handoff-design.md` (read-only reference)

**Interfaces:**
- Consumes: completed Tasks 1-12.
- Produces: updated plan status, no new production API.

- [ ] **Step 1: Update the older design document**

Ganti bagian yang menunda "Transfer live PTY / hub rebind" dengan catatan bahwa Fase 1 sudah mengaktifkannya. Ganti copy lama "terminal sessions restart there" dengan penjelasan bahwa proses dan output history tetap hidup, sambil menyebut batas bahwa TUI mungkin perlu redraw. Tambahkan tautan ke spec tanggal 2026-11-16.

- [ ] **Step 2: Run full frontend checks**

```bash
pnpm lint
pnpm check-types
pnpm test
pnpm build
```

Expected: semua PASS. Jika `pnpm lint` melaporkan error lama di luar diff, catat secara eksplisit dan jangan memformat seluruh repo.

- [ ] **Step 3: Run full Rust checks**

```bash
cd src-tauri
cargo clippy --all-targets --locked -- -D warnings
cargo nextest run --locked
# Jika cargo-nextest tidak tersedia:
cargo test --locked
cargo check --all-targets
```

Expected: PASS. Jangan menjalankan `cargo fmt --check` karena `rustfmt` tidak terpasang; sebutkan keterbatasan ini pada laporan akhir.

- [ ] **Step 4: Run debug app and manual smoke checklist**

```bash
pnpm exec tauri build --debug --no-bundle
```

Jalankan executable debug, lalu uji: `vim`, `htop`, proses panjang, Open Space in new window, Bring back to this window, move tab, tutup target window saat transaksi, dan gagalnya pembuatan window. Pastikan PID proses dan output history bertahan pada perpindahan sukses, dan source tetap bisa dipakai saat perpindahan gagal. Catat bahwa TUI boleh meminta redraw.

- [ ] **Step 5: Final commit**

```bash
git add docs/multi-window-space-plan.md docs/superpowers/plans/2026-11-16-pty-single-state-handoff.md
git commit -m "docs(pty): record live terminal handoff implementation"
```

---

## Self-Review Notes

Spec coverage is mapped as follows: stable IDs/token/epoch and single host in Tasks 1, 3, 6; backend-owned replay/checkpoint in Tasks 2, 4, 5; prepare/detach/attach/commit/rollback in Tasks 1, 5, 10; cleanup in Task 11; frontend serializer and view lifecycle in Tasks 8, 9; every move path in Task 10; recursion guard in Task 12; full verification and documentation in Task 13. Every task includes a failing test first and a targeted command. No step leaves error handling, test code, or signatures undefined.
