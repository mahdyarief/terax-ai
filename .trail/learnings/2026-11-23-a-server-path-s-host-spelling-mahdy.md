---
title: "A server path's host spelling is one shared placeholder, so never compare or test server paths through it"
author: Mahdy
date: 2026-11-23
branch: dydev
cost: { minutes: 1, tokens: 1841 }
id: l-155f5995
scope: repo
status: active
src: agent
touches:
  - src-tauri/src/modules/workspace.rs
  - src-tauri/src/modules/remote/fs_tests.rs
---

## Decided
For an SSH workspace, `resolve_path` returns the same `UNREACHABLE` placeholder for every path. Any code that compares, tests or pattern-matches the resolved host path is silently wrong for a server: all paths look equal, none is absolute, no secret pattern matches. Compare and check a server's paths as the server spells them.

## Watch out
Phase 4 Task 4 found three such bugs that tests did not catch until commands ran against the in-process agent: every remote move was a no-op, every paste/drop was refused, and the secret-path policy never fired. Audit any remaining `resolve_path(..)` use (search, watch, assets, ai) the same way.

In the code: `same_spelling` and `is_absolute_in` in src-tauri/src/modules/workspace.rs; `read_policy::readable_in` / `deny_runtime_in`; tests in src-tauri/src/modules/remote/fs_tests.rs. The in-process test server is this Windows machine, so server paths in tests look like `C:/...`.
