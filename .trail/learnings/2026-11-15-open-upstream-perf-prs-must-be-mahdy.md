---
title: Open upstream perf PRs must be judged against the Ghostty-era code
author: Mahdy
date: 2026-11-15
branch: dydev
id: l-16a2b756
scope: repo
status: active
src: agent
---

## Decided

- On the 2026-11-15 sweep of the 230 open upstream PRs, only #1220 (chat LRU eviction, already ported as `eedb0f2`) and #1460 (command-palette workspace scoping, ported as `1be0121` + `5ffc6de`) were worth bringing into `dydev`. #1460 shrinks content-search/recent-file I/O to the active workspace and cancels a queued search when a retry fires.
- Judged performance by mechanism, not by keyword: bundle/startup cost, CPU idle, render/GPU work, memory/leak and cache behavior. PRs that only fix correctness, docs or stale APIs were set aside.

## Tried and ruled out

- #210 (`parking_lot` PTY): conflicts because `src-tauri/src/pty` was rewritten into `hub.rs`/`queue.rs`/`gate.rs` using `PoisonError::into_inner`; would need a new Rust dep + manual port for a marginal uncontended-lock win.
- #723 (renderer hibernation) targets the old xterm renderer; the terminal is now Ghostty WebGPU/WebGL, so the files no longer exist.
- #953 sets `preserveDrawingBuffer: true`, which disables a GPU swap-chain optimization; it is a one-frame desync fix, not a speedup.
- #1389 (terminal render recovery), #1376 (watcher polling), #847 (session restore) and #277 (SSH) all conflict or are big features; #277 even carries an unrelated 5k-line blob.
- #1214 (eager-import tracer tests), #1430 and #1483 are correctness/invariant tests, not runtime gains.

## Watch out

- The fresh `eedb0f2` build already exceeds the `.size-limit.json` `total client JS` budget by ~113 kB (1.62 MB vs 1.5 MB), driven by `streamdown` (~139 kB gz) and `cm-lang-css` (~125 kB gz). Eager startup is fine (251 kB / 540 kB), so this is an all-chunks exposure, not a startup regression. Do not chase it by porting upstream perf PRs; trim/lazy-load those two chunks instead.
