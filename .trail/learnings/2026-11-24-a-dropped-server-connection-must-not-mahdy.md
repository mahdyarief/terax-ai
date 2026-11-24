---
title: "A dropped server connection must not look like a shell exit, or the pane closes"
author: Mahdy
date: 2026-11-24
branch: dydev
cost: { minutes: 30, tokens: 148735 }
id: l-22926e63
scope: repo
status: active
src: agent
touches:
  - src-tauri/src/modules/pty/remote.rs
  - src/modules/terminal/lib/osc-handlers.test.ts
  - src/modules/terminal/lib/pty-bridge.test.ts
  - src/modules/terminal/lib/pty-bridge.ts
  - src/modules/terminal/ghostty/useGhosttyTerminalSession.ts
---

## Decided
A server terminal whose connection ended exits with a code no process uses (-1001). The frontend keeps the pane for it and shows "Terminal disconnected" with Retry, instead of treating it as the shell's own exit.

## Tried and ruled out
Reporting a drop as an ordinary exit (code 1): `App.tsx::handleLeafExit` closes the pane on any exit, and closes the window when it was the last pane of the last tab, so a network blip would have closed every server terminal, and possibly the app.

## Watch out
The terminal's own output also gets "[the connection to <host> ended]" before the exit, so the reason survives in the scrollback. A close the app asked for ("the terminal was closed") is not a drop and exits normally.

In the code: `src-tauri/src/modules/pty/remote.rs` (`CONNECTION_ENDED_EXIT`), `src/modules/terminal/lib/pty-bridge.ts` (`PTY_EXIT_CONNECTION_ENDED`, `afterPtyExit`), `src/modules/terminal/ghostty/useGhosttyTerminalSession.ts` (the `onExit` handler).
