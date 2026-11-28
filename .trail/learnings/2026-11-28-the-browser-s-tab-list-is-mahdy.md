---
title: "The browser's tab list is republished when a pane's terminal process starts or ends"
author: Mahdy
date: 2026-11-28
branch: dydev
cost: { minutes: 47, tokens: 168392 }
id: l-18fae54e
scope: repo
status: active
src: agent
touches:
  - docs/remote-serve.md
  - src-tauri/src/lib.rs
  - src-tauri/src/modules/web_remote/mod.rs
  - docs/superpowers/plans/2026-11-27-web-remote.md
  - src-tauri/src/modules/web_remote/https.rs
  - src/settings/sections/RemoteSection.tsx
  - src/modules/settings/openSettingsWindow.ts
  - src/modules/statusbar/StatusBar.tsx
  - src/modules/web-remote/RemoteAccessPill.tsx
  - src/settings/SettingsApp.tsx
---

## Decided
The remote tab list is republished whenever a pane gains or loses its PTY, through a small notifier in the terminal module (notifyPtyChange / subscribePtyChanges).

## Tried and ruled out
Relying on tab, agent-activity and space changes to trigger a publish. A tab is created before its PTY opens, so the first publish has ptyId null; on a headless --remote-serve host nothing else changes afterwards and the browser showed "This tab has no live terminal yet" forever. On a desktop it usually healed by accident (some other store change).

## Watch out
Any new place that sets or clears a Ghostty session's pty must call notifyPtyChange, or the browser keeps a stale pane-to-PTY map. The terminal module must not import web-remote; the notifier lives in terminal/lib for that reason.

In the code: src/modules/terminal/lib/ptyChanges.ts, src/modules/terminal/ghostty/useGhosttyTerminalSession.ts (after session.pty = pty and in onExit), src/modules/web-remote/publishTabs.ts usePublishTabs.
