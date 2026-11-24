---
title: "Server shell commands tested on Windows run MSYS sh, whose pwd is not the server's path"
author: Mahdy
date: 2026-11-24
branch: dydev
cost: { minutes: 1, tokens: 1024 }
id: l-b1a08e4a
scope: repo
status: active
src: agent
touches:
  - src-tauri/src/modules/shell/mod.rs
  - src-tauri/src/modules/remote/shell_tests.rs
---

## Decided
A server's AI commands run `sh -c` with `sh` taken from the server's PATH, not `/bin/sh`, because `/bin/sh` does not exist on Windows. Otherwise the in-process agent tests fail there. Git for Windows puts MSYS's `sh` on PATH, so the tests can run.

## Watch out
Under MSYS, `pwd` prints `/c/Users/...`. The Windows agent does not know that as a folder, so in those tests a shell session does not follow `cd` and keeps its folder. Check `cwd_after` only on Unix or on a real server; the dypod e2e does. Don't map MSYS paths to "fix" it, because on a server the path is already right and a mapping would corrupt it.

In the code: src-tauri/src/modules/shell/mod.rs (run_blocking, the ssh branch), src-tauri/src/modules/remote/shell_tests.rs, remote/e2e_tests.rs.
