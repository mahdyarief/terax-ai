---
title: "Server-terminal tests only run on Linux and Windows, and an exited terminal waits for a slow client"
author: Mahdy
date: 2026-11-24
branch: dydev
cost: { minutes: 57, tokens: 229246 }
id: l-fed18785
scope: repo
status: active
src: agent
touches:
  - ".github/workflows/ci.yml"
  - src-tauri/crates/terax-agent/src/pty.rs
  - src-tauri/src/modules/remote/git_tests.rs
  - src-tauri/src/modules/fs/assets.rs
  - src-tauri/src/modules/fs/file.rs
  - src-tauri/src/modules/pty/paste_image.rs
  - src-tauri/src/modules/remote/fs_tests.rs
  - src-tauri/src/modules/ssh_commands_guard_tests.rs
  - CHANGELOG.md
  - package.json
---

## Decided
- Tests that run the agent as a server on this machine (transport, bootstrap, watch, server terminal and source control) are ignored on macOS with `cfg_attr(target_os = "macos", ignore = ...)`, because the agent supports Linux servers only. On macOS it refuses with "Darwin servers are not supported" and "watching is not available on a macOS server". These tests still run on Linux CI, where they matter, and on Windows.
- Tests that create a folder whose name is not valid UTF-8 are ignored on macOS, because APFS refuses such names (EILSEQ, os error 92). No such sibling can exist there.
- After a terminal's process exits, the agent keeps reading as long as output still comes or the reader is only waiting for the client's credit. It gives up only after `DRAIN` (2 s) of quiet, because a slow client lost the end of a long `cat` when DRAIN was measured by the clock. Closing a terminal ends its window, so the wait cannot hang.
- Each PTY read takes at most the room left in the window, because Linux reads are never whole chunks and the window went up to a chunk over PTY_WINDOW.

## Tried and ruled out
- Blaming FSEvents for the macOS source-control test. That was wrong: the agent refuses to watch on macOS at all.

## Watch out
- CI rust-test runs nextest with `--no-fail-fast`. Before that, each platform showed only its first failure, and the fixes took 4 rounds of CI.
- The test helper `read_until` passes `done` only the newest output (the frame plus 64 KB). Searching all of a 5 MB flood for every frame made the test take 60 s and caused the slow-client loss above.

In the code: src-tauri/crates/terax-agent/src/pty.rs (wait_for_the_last_output, pump), crates/terax-agent/tests/pty.rs (read_until), .github/workflows/ci.yml.
