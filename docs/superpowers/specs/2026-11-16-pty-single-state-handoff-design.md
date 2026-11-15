# Design: PTY Single-State Handoff Across Windows

**Date:** 2026-11-16  
**Status:** Approved design, pending written-spec review  
**Branch:** `dydev`

## Goal

Replace terminal respawn during cross-window moves with one live PTY state that can be attached by exactly one active host window at a time. This applies to every tab movement path: Open Space in new window, Bring Space back to this window, and move tab across windows.

This phase does not add a daemon, multi-writer collaboration, read-only mirrors, or survival across application restart/update.

## Decisions

- Keep `1 Space = 1 host window` from `docs/multi-window-space-plan.md`.
- Every PTY is identified by a stable `ptySessionId` and has one active attachment: `hostWindowLabel`, `attachToken`, and `leaseEpoch`.
- Only the active attachment may receive output or write input. A stale token or stale lease epoch is rejected.
- Backend Rust remains the owner of the child process and output sequence. It becomes independent of WebView lifetime.
- Frontend windows are disposable views. A view detach must never kill the PTY. Explicit terminal close and real application shutdown remain the only kill paths.
- Handoff failure rolls back to the source attachment with a fresh token and epoch. Silent respawn is not allowed.
- If rollback cannot restore the source view, the PTY is closed explicitly and the user sees an error. The system never leaves an unattached live session.
- Functional core logic lives in pure Rust and pure TypeScript modules. Tauri commands and React hooks remain thin shells.

## Why this design

Orca keeps terminal state alive across renderer restarts by separating daemon-hosted sessions from renderer attachments: `createOrAttach`, `getSnapshot`, `takePendingOutput`, and `disconnectDaemon()` rather than `shutdownDaemon()` during update. Terax does not need an external daemon for moves between windows inside one running Tauri process. The useful abstraction is the same: process lifetime and terminal state are independent of the view that currently renders them.

The current Terax implementation ties PTY ownership to `webview.label()`, binds stream tokens and ACK credits to that label, and disposes the Ghostty model when a tab is detached. Simply skipping `disposeSession` would create orphaned PTYs, stale stream tokens, duplicated or missing output, and empty scrollback. This design replaces those assumptions with an explicit attachment protocol.

## Architecture

### Backend

Add a pure attachment state machine, separate from `PtyState` lock handling and Tauri command plumbing. It transitions each session through:

`Attached → Preparing → Detached → Attaching → Attached`

A transition records `handoffId`, `leaseEpoch`, source and target window labels, output checkpoint sequence, and a bounded replay log. The state machine validates every command and returns either a new state or a typed error; it performs no I/O and spawns no process.

`PtyState` keeps the PTY process, writer, output queue, and replay checkpoint globally. `owner` is replaced by an active attachment record. Hub stream registration and ACK validation use `attachToken` and `leaseEpoch`, not a bare label. During `Preparing`, output is buffered at a sequence boundary. During `Detached`/`Attaching`, no view may consume output. After `Attached`, the target receives replay from the checkpoint and then resumes live output.

Tauri commands are thin wrappers:

- `pty_prepare_handoff` freezes a sequence boundary and returns a checkpoint.
- `pty_detach` revokes the source token and releases its view resources without killing the PTY.
- `pty_attach` validates the handoff, installs the target attachment, and returns replay plus a fresh token.
- `pty_commit_handoff` finalizes the lease epoch.
- `pty_rollback_handoff` installs a fresh source token and epoch, then replays output produced during the failed attempt.
- Existing `pty_write`, `pty_resize`, `pty_poll`, `pty_ack`, and `pty_stream_open` validate the active token and epoch.

`destroy_webview`, `reset_webview`, startup cleanup, and main-window destroyed handling must distinguish between detaching a view and closing a session. A transaction in progress is neither killed nor leaked: it is rolled back or explicitly closed after its deadline.

### Terminal state checkpoint

`packages/ghostty-core` currently exposes render, viewport, scrollback, cursor, and mode APIs but no full Wasm `save`/`restore` export. JavaScript objects cannot be transferred between Tauri WebViews because each window has an isolated JS context. Therefore, this phase uses a replayable stream checkpoint, not object transplantation and no Wasm change:

- Backend retains the complete ordered log of raw output bytes and resize events emitted since session creation.
- The log starts at sequence zero. A checkpoint records its end sequence and current terminal dimensions.
- On attach, the target creates a fresh Ghostty model with the creation dimensions, applies the retained resize/output events in order through the checkpoint, then consumes post-checkpoint events before opening the live stream.
- Because the fresh emulator does not reply to terminal queries, replay reconstructs output history and terminal-content state, not an exact live TUI negotiation. Moving an interactive TUI may require its application to redraw; the child process, input stream, and output stream remain live.
- The log has an explicit byte and event limit for the lifetime of a session. If the log would be truncated before a complete reconstruction is possible, `pty_prepare_handoff` fails before detach and no UI state changes. This keeps memory bounded while guaranteeing that every accepted handoff has a valid replay.
- Pending live output after the checkpoint is delivered through the same ordered replay path, preventing gaps and duplicates.

This preserves the live process and its output/scrollback without pretending that a full emulator-state serializer already exists. A later phase may add a native terminal snapshot to preserve exact TUI parser state when replay cost, truncation, or redraw behavior becomes a product problem.

### Frontend

`serialize.ts` adds `ptySessionId`, the checkpoint sequence, and terminal dimensions to serialized terminal leaves. Older state without `ptySessionId` remains a cold terminal and keeps the existing behavior.

`useTabs.ts` gains a distinct handoff/dispose boundary. Closing a tab calls `disposeSession`; moving a tab or Space calls a handoff path that prepares, detaches, serializes, sends `spaces:tabs-moved`, hydrates, attaches, and commits. All move paths use this one path.

`useGhosttyTerminalSession.ts` separates view cleanup from session close. Detach removes surface, writer, input, and local model resources without invoking `pty_close`. A new attach path creates a model from replay, applies pending output, then starts live output. `pty-bridge.ts` and `PtyOutputHub.ts` carry the active token and epoch with every stream, write, resize, ACK, and poll request.

`App.tsx` coordinates the transaction and UI state. It only changes `hostWindowLabel` after backend prepare succeeds, and restores it on rollback. The existing guard that prevents `space-*` windows from opening another Space window remains unchanged.

## Data flow

1. The source window initiates a move with `handoffId`, `ptySessionId`, target window label, and all affected tab IDs.
2. Backend `prepare` freezes a replay checkpoint and validates that the bounded reconstruction log is complete.
3. The source window detaches views and revokes its active token. The PTY keeps running.
4. Serialized tabs carry the stable `ptySessionId` and checkpoint. The target hydrates without allocating a new PTY ID.
5. Backend `attach` installs a fresh token for the target, increments `leaseEpoch`, and returns ordered replay through the current sequence.
6. The target reconstructs the Ghostty model, consumes replay, opens live output, and applies only post-checkpoint output.
7. Backend `commit` finalizes ownership and drops the rollback record.
8. On any failure before commit, backend `rollback` installs a fresh source token and epoch, replays output produced during the attempt, and target views are discarded. No new process is spawned.

## Error handling

- Prepare failure: no detach, no state mutation, no warning dialog change.
- Window creation or persistence failure after prepare: rollback to the source with a fresh token before any target hydration.
- Attach or replay failure: discard target views, rollback the source with a fresh token, and restore buffered output in order.
- Source window disappears before rollback: explicitly close the PTY and surface a failed-move error. Do not leave the session detached indefinitely.
- Commit timeout: rollback if the source is still available; otherwise explicitly close.
- Stale source input after commit: rejected by token and epoch.
- Duplicate command delivery: commands are idempotent by `handoffId`.
- Replay overflow: prepare fails before detach; no partial transfer.
- Explicit user close during a transaction: cancel the handoff and close the PTY after the source view is released.

## Testing

Every production change starts with a failing test. Tests assert real state transitions and observable behavior, not mock call counts.

Rust unit tests cover:

- detach does not kill the child process;
- stale tokens and epochs are rejected for write, resize, poll, ACK, and stream open;
- replay has no gaps or duplicates across the sequence boundary;
- prepare, commit, and rollback are idempotent by `handoffId`;
- rollback installs a fresh source lease and replays buffered output;
- replay overflow fails during prepare without detaching;
- destroyed-window cleanup does not kill a session in a live transaction;
- explicit close and application shutdown still terminate the PTY.

TypeScript unit tests cover:

- serialized terminal leaves retain `ptySessionId` and checkpoint metadata;
- old serialized state remains a cold terminal;
- every cross-window move path calls handoff instead of dispose;
- explicit tab close still calls dispose;
- attach waits for replay completion before live output;
- stale token/epoch errors do not silently spawn a replacement PTY;
- rollback restores the source host and tab state;
- `space-*` windows still cannot open another Space window.

Integration/manual checks run `vim`, `htop`, a long-running process, Open Space in new window, Bring back to this window, move tab, and a forced window-creation failure. The process ID, output history, scrollback, and input capability must survive a successful move. Exact TUI cursor and mode parity is not guaranteed before native emulator snapshots exist; a TUI may need a redraw. A failed move must leave the source usable.

Required checks follow `TERAX.md`: `pnpm lint`, `pnpm check-types`, `pnpm test`, `cargo clippy --all-targets --locked -- -D warnings`, and `cargo test --locked`. Fast iteration uses targeted Vitest and `npm run tauri -- build --debug --no-bundle`; a release installer is not part of this phase's design or planning step.

## Non-goals

- No sidecar daemon, detached OS process, or endpoint adoption.
- No terminal survival through application update/restart.
- No simultaneous read-only mirrors or multi-writer shared terminals.
- No changes to Ghostty Wasm or full native emulator serialization in this phase.
- No broad refactor outside the PTY, tab handoff, Space handoff, and output hub paths.

## Replacement of previous deferred work

`docs/multi-window-space-plan.md` marks "Transfer live PTY / hub rebind" as optional v1-deferred work. This spec intentionally activates that item with a single-host transaction protocol. The older plan's statement that `owner` must remain bound to `webview.label()` is superseded by this design. Its invariant of one host window per Space is retained.
