---
title: Handoff PTY needs env adoption and one tab-restore owner
author: Mahdy
date: 2026-11-17
branch: dydev
id: l-2f62f509
scope: repo
status: active
src: agent
touches:
  - package.json
  - src-tauri/Cargo.lock
  - src-tauri/Cargo.toml
  - src-tauri/src/lib.rs
  - src-tauri/tauri.conf.json
  - src-tauri/tauri.windows.conf.json
  - src/app/App.tsx
  - src/app/hooks/useWorkspaceSwitcher.ts
  - src/modules/editor/EditorStack.tsx
  - src/modules/spaces/lib/crossWindowSync.ts
---

## Decided
- A window that boots with no hosted space adopts that space's workspace env when the first handoff activates it; PTY spawn/attach then sees WSL vs local cwd instead of the default env.
- Tabs have exactly one restore owner per scenario: live handoffs restore via SPACE_TABS_MOVED_EVENT, and only a closed space window restores from disk via SPACE_HOST_CHANGED_EVENT tagged reason=window-closed. This prevents duplicate tabs and attaching PTYs already detached for handoff.
- Workspace env adoption is guarded by a monotonic sequence token so a slow WSL home lookup from an older space cannot authorize after a newer handoff wins.

## Tried and ruled out
- Restoring from disk on every host-changed broadcast: raced with the live tabs-moved handoff, appended duplicate tabs, and tried to attach PTYs the source had detached.
- Using activeSpaceEnv/findActiveSpace for the missing-env lookup: both fall back to local or the first space, which is the exact wrong-env bug being fixed.
- Removing first-activation env adoption: broken for the window-closed path where only setActive runs and no tabs-moved follows.

## Watch out
- Reason is optional on HostChangedPayload to avoid a coordinated emitter change; an untagged payload is treated as handoff. All current emitters tag explicitly (Rust Destroyed hook uses window-closed, emitHostChanged defaults to handoff).
- Queued handoff replays are only consumed on leaf mount; a rollback/correction detaches without consuming them, so that path can leak scrollback until a cleanup API is added.

## In the code
- src/app/App.tsx: first-activation adopt, host-changed restoreFromDisk gate, tabs-moved meta/env resolution before pty_attach.
- src/modules/spaces/lib/crossWindowSync.ts: HOST_CHANGED_REASONS, HostChangedPayload.reason, idempotent applyHostChanged.
- src/modules/spaces/lib/useSpaces.ts: emitSafely SSOT, emitHostChanged.
- src/app/hooks/useWorkspaceSwitcher.ts: adoptSeqRef generation guard.
- src-tauri/src/lib.rs: Destroyed hook tags reason=window-closed.
