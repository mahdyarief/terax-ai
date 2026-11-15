---
title: "Upstream chat LRU fix needed test adaptation, not a verbatim apply"
author: Mahdy
date: 2026-11-15
branch: dydev
id: l-5781ed44
scope: repo
status: active
src: agent
touches:
  - src/modules/ai/store/chatStore.ts
---

## Decided

- Ported the chat LRU eviction fix from upstream PR #1220, but adapted its
  tests: the PR was written against an older `chatStore` API that still had
  `hasKeyForModel` and `stop` exports, both since removed from upstream `main`.
- Debounce coverage from that PR is redundant here: this codebase replaced
  plain debouncing with a scheduled persistence model (`persistSchedule.ts`)
  and already has `chatStore.persist.test.ts` plus `persistSchedule.test.ts`.

## Tried and ruled out

- Applying PR #1220 verbatim: `tsc --noEmit` failed on the two removed exports
  and the test expected `saveMessages(id, messages)` while this codebase calls
  `saveMessages(id, messages, { durable })`.

## Watch out

- Other open upstream perf PRs are stale too: #210 (`parking_lot`) conflicts
  because the PTY module was rewritten into `hub.rs`/`queue.rs`/`gate.rs`;
  #723 targets the old xterm renderer, now Ghostty WebGPU/WebGL; #953 sets
  `preserveDrawingBuffer: true`, which disables a GPU optimization and is a
  frame-sync fix rather than a speedup.
