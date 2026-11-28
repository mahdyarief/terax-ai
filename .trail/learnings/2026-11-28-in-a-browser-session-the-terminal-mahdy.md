---
title: In a browser session the terminal stream requests must carry the device cookie
author: Mahdy
date: 2026-11-28
branch: dydev
cost: { minutes: 14, tokens: 263976 }
id: l-ef87bdde
scope: repo
status: active
src: agent
touches:
  - docs/superpowers/plans/2026-11-28-web-remote-full-app/plan.md
  - src-tauri/src/modules/web_remote/devices.rs
  - src/modules/terminal/lib/pty-bridge.test.ts
  - src/modules/terminal/lib/pty-bridge.ts
  - src/modules/web-remote/publishTabs.ts
---

## Decided
- `pty-bridge` fetches the terax-pty scheme with `credentials: "omit"` on the desktop and `"same-origin"` in the browser runtime (`ptyFetchCredentials`).
- The app in a browser never publishes tabs (`setRemotePublishingEnabled` stays off under `isRemoteRuntime()`). It is a viewer, and the bridge refuses `remote_publish_tabs` anyway.

## Tried and ruled out
- `"omit"` everywhere. Every `/scheme/terax-pty/<session>/poll` then arrives without the `terax_remote` cookie and gets 401 "Pair this device first.", so the full app's terminal shows Retry and takes no input.
- The bridge's live tests did not catch this: they call the routes with curl and set the cookie by hand.

## Watch out
- Any desktop fetch to a custom scheme that the web remote serves under `/scheme/` must send same-origin credentials when `isRemoteRuntime()` is true.
- Check a browser session through the Network panel, not only through curl.

In the code: src/modules/terminal/lib/pty-bridge.ts (`ptyFetchCredentials`), src/modules/web-remote/publishTabs.ts (`setRemotePublishingEnabled`)
