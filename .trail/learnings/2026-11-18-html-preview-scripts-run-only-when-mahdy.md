---
title: HTML preview scripts run only when no local file was inlined or the folder is trusted
author: Mahdy
date: 2026-11-18
branch: dydev
cost: { minutes: 41, tokens: 411739 }
id: l-8b665111
scope: repo
status: active
src: agent
touches:
  - src-tauri/src/modules/pty/handoff.rs
  - src-tauri/src/modules/pty/mod.rs
  - src-tauri/src/modules/pty/output.rs
  - src-tauri/src/modules/pty/queue.rs
  - src-tauri/src/modules/pty/replay.rs
  - src-tauri/src/modules/pty/session.rs
  - src/app/App.tsx
  - src/lib/tauriBridge.test.ts
  - src/lib/tauriBridge.ts
  - src/main.tsx
---

## Decided
- A preview that inlined local files is served with scripts off until the user trusts the folder (one click, remembered per folder). A script can send out anything it can read, and what it can read is the document plus the inlined local files. A self-contained page has nothing to steal, so its scripts run without asking.
- The backend defaults to the scripts-off policy when the caller does not ask for scripts, so a forgotten flag fails safe.

## Tried and ruled out
- Tightening only `img-src` (dropping `https:`): a script can still leak by navigating its own frame, and no CSP directive blocks that. The app's own `frame-src` allows http/https because other previews need it, so it cannot be narrowed either.

## Watch out
- Scroll restore in the preview relies on an injected script, so it does not work while scripts are off.

In the code: src-tauri/src/modules/fs/preview.rs (CSP, CSP_STATIC, fs_publish_html `scripts`), src/modules/editor/HtmlPreview.tsx, src/modules/editor/lib/htmlPreviewTrust.ts, src/modules/editor/lib/htmlPreview.ts (withScrollBridge)
