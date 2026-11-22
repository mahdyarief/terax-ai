---
title: "Confined file reads check the opened handle, and their three copies differ on purpose"
author: Mahdy
date: 2026-11-22
branch: dydev
cost: { minutes: 106, tokens: 321857 }
id: l-f5be32b4
scope: repo
status: active
src: agent
touches:
  - src-tauri/Cargo.lock
  - src-tauri/crates/terax-workspace-core/Cargo.toml
  - src-tauri/crates/terax-workspace-core/src/contract.rs
  - src-tauri/crates/terax-workspace-core/src/host.rs
  - src-tauri/crates/terax-workspace-core/src/paths.rs
  - src-tauri/crates/terax-workspace-core/src/workspace.rs
  - src-tauri/src/modules/fs/mutate.rs
  - src-tauri/src/modules/fs/tree.rs
  - src-tauri/src/modules/fs/file.rs
  - src-tauri/Cargo.toml
---

## Decided
The project-memory read, asset serving and AI file reads each open the file first and then check the real path of the opened handle against the authorized roots. They were left out of the copy/read migration to the Workspace backend and will move together with the registry (phase 1, task 11), as one backend operation that opens, checks the handle against an authorized root, applies the secret-path policy to that real path and reads with a limit. A remote agent has to run that same check on the server; a closure from the app cannot cross the connection.

## Watch out
The three `opened_path` helpers were not duplicates: on Windows the AI read kept the lossless verbatim path from GetFinalPathNameByHandleW, while asset serving and project memory went through `from_utf16_lossy` and compared the stripped forward-slash spelling. Task 11c merged them into one `Workspace::read_confined` (core `host.rs`), which keeps the lossless path, refuses it (`ConfinedError::Inexact`) when it is not valid Unicode, and only then strips it, so every caller now compares an exact spelling. Do not reintroduce a lossy conversion on that path: a lossy spelling can equal a real sibling's and pass the containment check.

In the code: src-tauri/crates/terax-workspace-core/src/confined.rs (roots, shared secret-path policy), host.rs (`read_confined`, `open_pinned`, `opened_path`); callers in src-tauri/src/modules/fs/ai.rs, assets.rs, project_memory.rs; plan docs/superpowers/plans/2026-11-22-remote-ssh-phase1-workspace-core.md, tasks 6 and 11.
