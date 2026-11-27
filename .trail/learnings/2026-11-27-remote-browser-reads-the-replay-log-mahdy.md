---
title: "Remote browser reads the replay log, never the desktop stream"
author: Mahdy
date: 2026-11-27
branch: dydev
id: l-bfef35af
scope: repo
status: active
src: agent
touches:
  - src-tauri/src/lib.rs
  - src-tauri/src/modules/web_remote/mod.rs
  - src-tauri/src/modules/web_remote/server.rs
---

Each web viewer subscribes to a PTY's replay log (snapshot, then replay_after) instead of the desktop's credit stream, so a browser that falls behind is rebuilt from a snapshot and can never slow a terminal or take output away from the desktop. Browser input goes through Session::write_remote, which appends to the same ordered writer queue without taking the window's attachment lease, because typing from a phone must not detach the desktop. Tab actions are never run in the HTTP layer: Rust emits remote:tab-action to the window hosting the space or tab, so new and close go through that window's own code and the published registry simply follows its next report.
