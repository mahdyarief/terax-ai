---
title: "Windows start publishing tabs whenever remote access is switched on, not only at launch"
author: Mahdy
date: 2026-11-28
branch: dydev
cost: { minutes: 9, tokens: 49754 }
id: l-549ff810
scope: repo
status: active
src: agent
touches:
  - src/web/terminal/RemoteTerminal.tsx
  - src/web/terminal/TypingBar.tsx
  - src/web/terminal/composer.test.ts
  - src/web/terminal/composer.ts
  - docs/remote-serve.md
  - docs/superpowers/plans/2026-11-27-web-remote.md
  - src/modules/web-remote/lastOptions.ts
  - src/modules/web-remote/lastOptions.test.ts
  - src/modules/web-remote/RemoteAccessPill.tsx
  - src/modules/web-remote/publishTabs.ts
---

## Decided
Every window follows the terax:remote-access event to switch tab publishing on and off, and usePublishTabs subscribes to store and PTY changes even while remote access is off (schedulePublish is a no-op until it is on). Turning it on publishes immediately.

## Tried and ruled out
Reading remote_access_status once at load. That only worked for --remote-serve: turning remote access on from Settings (or the status bar) later left every window unpublished, so the browser listed no tabs.

## Watch out
The status bar toggle reuses the last bind/port/HTTPS from localStorage key terax.remoteAccess.last (written on each start by Settings or the pill). Malformed values fall back to loopback, never a wider bind.

In the code: src/modules/web-remote/publishTabs.ts (setRemotePublishingEnabled, useRemotePublishingStatus), src/modules/web-remote/lastOptions.ts, src/modules/web-remote/RemoteAccessPill.tsx.
