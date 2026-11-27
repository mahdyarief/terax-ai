---
title: "A trusted folder's HTML preview also runs its own script files and CDN libraries"
author: Mahdy
date: 2026-11-27
branch: dydev
cost: { minutes: 37, tokens: 294756 }
id: l-0bbd5dad
scope: repo
status: active
src: agent
touches:
  - src/app/App.tsx
  - src/modules/preview/PreviewAddressBar.tsx
  - src/modules/preview/lib/portPresets.ts
  - src/modules/statusbar/DevServersPill.test.tsx
  - src/modules/statusbar/DevServersPill.tsx
  - src/modules/statusbar/StatusBar.tsx
  - src/modules/terminal/ghostty/useGhosttyTerminalSession.ts
  - src/modules/terminal/lib/devServers.test.ts
  - src/modules/terminal/lib/devServers.ts
  - src/modules/editor/lib/inlineHtmlAssets.ts
---

## Decided
- The HTML file preview has three policies, not two: scripts off (local files inlined, folder not trusted), inline scripts only (nothing local inlined), and trusted. Only the trusted one adds `https:` and `data:` to `script-src`, so CDN libraries (Tailwind browser, chart libraries) and the folder's own `<script src>` files run there.
- Local `<script src>` files are inlined as `data:` URLs, not as inline `<script>` bodies, so `defer`, `async` and the order of scripts behave as in a browser. Only `.js`, `.mjs` and `.cjs` inside the preview boundary are inlined; the `?v=` query is dropped.
- A page that loads a CDN script but is not trusted shows the same "Run scripts for this folder" bar, with a message about libraries from the web.
- Why this is acceptable: trusting a folder already lets its scripts read the inlined files and send them out (frame navigation, image URLs). Running the libraries the page names adds no new reach, and still needs the user's click.

## Tried and ruled out
- Before this, local `<script src>` files were never inlined. They resolved against the `terax-doc` scheme and the CSP blocked them, so a page's own JS never ran, even when trusted. The fix is to inline them; asking the user to start a local server instead was what we wanted to avoid.

## Watch out
- `connect-src` stays `'none'` even when trusted, so a page that loads its data with `fetch` still needs the Preview tab with a local server.
- The frame keeps an opaque origin (no `allow-same-origin`), so `localStorage` throws. Pages must wrap it in try/catch.

In the code: src-tauri/src/modules/fs/preview.rs (CSP_TRUSTED, policy(), fs_publish_html `trusted`), src/modules/editor/lib/inlineHtmlAssets.ts (step 5, hasRemoteScripts), src/modules/editor/HtmlPreview.tsx (BlockedScripts).
