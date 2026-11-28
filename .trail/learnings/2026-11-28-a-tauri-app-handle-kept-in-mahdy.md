---
title: A Tauri app handle kept in a struct the tests build makes the Windows test binary fail to start
author: Mahdy
date: 2026-11-28
branch: dydev
cost: { minutes: 38, tokens: 194854 }
id: l-0296fdac
scope: repo
status: active
src: agent
touches:
  - docs/superpowers/plans/2026-11-27-web-remote/agent-prompt.md
  - docs/superpowers/plans/2026-11-27-web-remote/plan.md
  - docs/superpowers/plans/2026-11-28-web-remote-full-app/agent-prompt.md
  - docs/superpowers/plans/2026-11-28-web-remote-full-app/plan.md
  - docs/superpowers/plans/2026-11-28-web-remote-full-app/resume.md
  - docs/superpowers/plans/2026-11-28-web-remote-full-app-agent-prompt.md
  - docs/superpowers/plans/2026-11-28-web-remote-full-app.md
  - src-tauri/src/modules/web_remote/server.rs
  - src-tauri/src/modules/web_remote/spike.rs
---

## Decided
- `server::Deps` reaches the app only through trait objects (`TabEvents`, `SpikeRunner`), never an `AppHandle` field.

## Tried and ruled out
- Putting `#[cfg(not(test))]` on the spike's webview functions and on the route made no difference.
- Reverting the capabilities, the `generate_handler!` lines, the `pty` changes or `remote-relay.html` made no difference either.
- The cause was the `Option<tauri::AppHandle>` field itself. The server tests build `Deps`, so its drop glue links the wry/WebView2 runtime into the test exe.

## Watch out
- Symptom: `cargo test --lib` builds fine, then the test exe exits with `0xc0000139` (STATUS_ENTRYPOINT_NOT_FOUND) before any test runs. The test binary has no app manifest on Windows.
- Bisect by stashing one group of files at a time against a passing HEAD (`git stash push -u`).

In the code: `src-tauri/src/modules/web_remote/server.rs` (`Deps::spike`), `src-tauri/src/modules/web_remote/spike.rs` (`SpikeRunner`, `AppSpike`)
