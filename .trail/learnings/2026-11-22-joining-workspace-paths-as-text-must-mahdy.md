---
title: "Joining workspace paths as text must refuse rooted input; PathBuf::join used to do that implicitly"
author: Mahdy
date: 2026-11-22
branch: dydev
cost: { minutes: 51, tokens: 123912 }
id: l-2f0b88f1
scope: repo
status: active
src: agent
touches:
  - docs/superpowers/plans/2026-11-22-remote-ssh-phase1-workspace-core.md
  - src-tauri/src/modules/fs/copy.rs
  - src-tauri/src/modules/fs/mutate.rs
  - src-tauri/src/modules/workspace.rs
  - src-tauri/src/modules/git/utils.rs
---

## Decided
When a path check moves from host `PathBuf` to workspace path strings (`terax_workspace_core::paths::join`), refuse a rooted relative part (`/x`, `\x`) before joining. `PathBuf::join` with an absolute argument *replaces* the base, so `/etc/passwd` used to land outside the repository and be refused by the containment check; joining text turns it into `<root>//etc/passwd`, inside the root, and it would be accepted.

## Watch out
Nothing tested the containment check of `resolve_within_repo` itself: mutating it to always pass left all 325 git tests green. A link inside the repository that points out of it is the case that exercises it; that test exists now. When you rewrite a guard, mutate it and check that a test fails.

In the code:
- `src-tauri/src/modules/git/utils.rs`: `resolve_within_repo` (the rooted check, and the existing- and deleted-path branches), tests `resolve_within_repo_rejects_rooted_paths` and `resolve_within_repo_rejects_a_link_out_of_the_repository` (Unix symlink; on Windows a `mklink /J` junction, which needs no symlink privilege).
- Caught by `review_tests::file_diff_rejects_paths_outside_the_repository`.
