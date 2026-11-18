---
title: A cross-window terminal move only works when the handoff id is a plain integer and the source may commit
author: Mahdy
date: 2026-11-18
branch: dydev
cost: { minutes: 41, tokens: 216990 }
id: l-030c99cf
scope: repo
status: active
src: agent
touches:
  - src/lib/tauriBridge.ts
  - src/lib/tauriBridge.test.ts
  - src/main.tsx
  - src/app/App.tsx
  - src/modules/spaces/lib/moveTabsAcrossWindows.ts
  - src/modules/spaces/lib/moveTabsAcrossWindows.test.ts
  - src/modules/terminal/lib/ptyHandoff.ts
  - src/modules/terminal/lib/ptyHandoff.test.ts
  - src-tauri/src/modules/pty/replay.rs
  - src-tauri/src/modules/pty/session.rs
---

## Decided
- Handoff ids are plain decimal integers (`ms * 1000 + seq`), because the backend parses them as `u64`. The old `"<ms>-<counter>"` id made every handoff fail at its first step ("invalid handoff id"), so the PTY move had never actually worked.
- The source window may commit the handoff even though `pty_attach` has already moved the host to the target. The check is host == caller OR (same handoff id and `handoff_from` == caller). Before this, commit always refused.
- Prepare is all or nothing: every terminal is validated before any is marked `Preparing`. A partial prepare stranded PTYs that no window owned and made rollback fail ("source-unavailable").
- Rollback hands back the full retained history, because the source already threw its view away at detach. It also returns the current lease for terminals the failed handoff never prepared.
- The replay log rolls: it evicts the oldest output past 8 MiB / 65,536 events instead of going "incomplete" for good, so a busy terminal stays movable with its newest history.

## Tried and ruled out
- Treating "source-unavailable" as the cause. It only says the rollback also failed, and it used to hide the real first error. That error is now kept in the message, and every handoff step logs a warn on failure.

## Watch out
- The Rust command handlers (`pty_commit_handoff` and friends) have no tests that run on Windows. The pure `handoff.rs` and `replay.rs` tests cover the state machine, not the command-level checks, and that is how the commit bug survived.

In the code: src/modules/terminal/lib/ptyHandoff.ts (nextHandoffId), src-tauri/src/modules/pty/mod.rs (pty_commit_handoff), src-tauri/src/modules/pty/handoff.rs (prepare, rollback), src-tauri/src/modules/pty/replay.rs (ReplayLog), src/modules/spaces/lib/moveTabsAcrossWindows.ts
