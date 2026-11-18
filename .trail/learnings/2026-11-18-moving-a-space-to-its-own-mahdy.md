---
title: "Moving a space to its own window must hand PTY ownership to the move, not close it"
author: Mahdy
date: 2026-11-18
branch: dydev
cost: { minutes: 38, tokens: 300726 }
id: l-7fcb2089
scope: repo
status: active
src: agent
touches:
  - src/modules/terminal/ghostty/useGhosttyTerminalSession.ts
  - src/modules/spaces/lib/serialize.ts
  - src/modules/spaces/lib/serialize.test.ts
  - src/modules/tabs/lib/useTabs.ts
  - src/modules/tabs/lib/useTabs.transfer.test.ts
  - src/modules/spaces/lib/windowReady.ts
  - src/modules/spaces/lib/windowReady.test.ts
  - src/modules/terminal/lib/handoffSession.ts
  - src/modules/terminal/lib/handoffSession.test.ts
  - src/modules/spaces/lib/handoff.ts
---

## Decided
- A handoff detach must hand PTY ownership to the transaction: the source's view teardown releases the session (`releaseGhosttySessionForHandoff`) instead of disposing it. Disposing calls `pty_close`, which reaped the PTY mid-move, so the target's `pty_attach` failed and tabs came back cold in main.
- `pty_close` refuses to close a terminal in a handoff phase (Preparing/Detached/Attaching) unless it is given the owning `handoffId`. Only the transaction's abort path passes it.
- Window readiness is forgotten when a space window is destroyed (host-changed reason=window-closed). Otherwise reopening the same space emits the handoff before the new window's listeners exist, and the event is silently dropped.
- Tabs that cannot travel losslessly (private terminals, AI diffs, dirty editors) stay in main, re-homed to another space main hosts. If there is none, the move is refused with a toast. Git diff/history/commit tabs travel, but only in handoffs; they are not persisted.
- A live leaf's `ptyAttachment` is never serialized. `stampHandoffAttachments` sets it fresh and `hydrateTabs` keeps it only with keepPty. A persisted one made later handoffs give cold leaves a dead PTY id.
- A space window lists only its own space and cannot create spaces; main is the hub that shows all of them (user's call).
## Tried and ruled out
- Releasing on a correction with `detachTabsForSpace`: it wiped every tab of that space in the target (including tabs it already had before a single-tab move) and `pty_close`d PTYs the source had just rolled back. The target now records each incoming handoff's tab ids and releases only those.
## Watch out
- The Rust pty tests are `#[cfg(all(test, unix))]`, so the `pty_close` guard tests do not run on Windows.
## In the code
- src/modules/terminal/ghostty/useGhosttyTerminalSession.ts `releaseGhosttySessionForHandoff`; src/modules/terminal/lib/terminalSessionApi.ts `detachSessionView`
- src-tauri/src/modules/pty/mod.rs `close_session`/`close_allowed`
- src/modules/spaces/lib/windowReady.ts `forgetWindowReady`; src/modules/spaces/lib/handoff.ts `visibleSpacesForWindow`, `pickHandoffFallback`
- src/modules/spaces/lib/serialize.ts `isHandoffMovable`, `serializeTabsForHandoff`; src/modules/tabs/lib/useTabs.ts `planHandoffDetach`
- src/app/App.tsx handleOpenSpaceInWindow, reclaim listener, tabs-moved handler (`incomingHandoffsRef`)
