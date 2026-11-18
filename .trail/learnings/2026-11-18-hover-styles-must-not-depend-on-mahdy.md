---
title: Hover styles must not depend on the device reporting that it can hover
author: Mahdy
date: 2026-11-18
branch: dydev
cost: { minutes: 18, tokens: 107750 }
id: l-ce5269ae
scope: repo
status: active
src: agent
touches:
  - src-tauri/src/lib.rs
  - src-tauri/src/modules/fs/file.rs
  - src-tauri/src/modules/fs/preview.rs
  - src/modules/editor/CsvHeadPreview.tsx
  - src/modules/editor/CsvPreview.tsx
  - src/modules/editor/DocxPreview.tsx
  - src/modules/editor/EditorPane.tsx
  - src/modules/editor/HtmlPreview.tsx
  - src/modules/editor/ImagePreview.tsx
  - src/modules/editor/SpreadsheetPreview.tsx
---

## Decided
- Hover styles apply unconditionally. Tailwind v4 wraps every `hover:` and `group-hover:` rule in `@media (hover: hover)`, and WebView2 on a touchscreen Windows laptop reports `hover: none`, `any-hover: none` and `pointer: coarse` even while a mouse or touchpad is in use. Measured in the running app: 112 of 121 hover rules were inside that media query and so never applied.
- A control must never be reachable by hover alone. Each one hidden with `opacity-0` and shown on group hover is also shown on keyboard focus, and the active tab always shows its close button, because a keyboard user never hovers.
- The Spaces list keeps its row actions always visible: the device that hit this bug is a touchscreen, where hover-revealed actions are awkward even when hover works.

## Tried and ruled out
- Switching the gate to `any-hover`: it is also false on this device, so it would change nothing.
- Removing hover gating row by row (as the Spaces list did earlier): it hides the cause and leaves every other hover-only control unreachable.

## Watch out
- With the override, a hover style can stay "stuck" after a tap on a touchscreen until the next tap elsewhere. Accepted for a desktop app.
- The Source Control discard button used `data-[focused=true]:` on a child that never carries that attribute; it has to be `group-data-[focused=true]:`.

In the code:
- `src/styles/globals.css`: `@custom-variant hover (&:hover);`
- `src/app/hover-only-controls.test.ts` fails when a class string has `opacity-0` and `group-hover:opacity-*` without a focus counterpart, or when the override is removed.
- To measure on a device: `matchMedia('(hover: hover)').matches` in the app's webview.
