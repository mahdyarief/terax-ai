---
title: "The status bar branch is a live git status consumer, so it has an off switch"
author: Mahdy
date: 2026-11-26
branch: dydev
cost: { minutes: 16, tokens: 73147 }
id: l-b88f3d58
scope: repo
status: active
src: agent
touches:
  - packages/ghostty-core/adapted/UPSTREAM.md
  - packages/ghostty-core/adapted/ghostty-vt-scalar.wasm
  - packages/ghostty-core/adapted/ghostty-vt.wasm
  - packages/ghostty-core/adapted/wasm/src/terax.zig
  - packages/ghostty-core/lib/adapted/TeraxGhostty.ts
  - packages/ghostty-core/lib/types.ts
  - src/modules/tabs/TabBar.tsx
  - src/modules/terminal/ghostty/AdaptedGhostty.test.ts
  - src/modules/terminal/ghostty/core/GhosttySemanticEventRouter.test.ts
  - src/modules/terminal/ghostty/core/GhosttySemanticEventRouter.ts
---

## Decided
- The status bar's git item (issue #9) reads the same SourceControlSummary as the rail badge (the repo the explorer / active context points at), so it adds no git calls of its own except listing branches when its switcher opens.
- Because the item is always on screen, it counts as a live status consumer: while it is shown, `git status` runs on focus and context changes and the repo is watched, even with the sidebar collapsed. Upstream designed status to be lazy, so the item can be turned off (Settings > General > Explorer > "Git branch in the status bar"); off restores the old lazy behaviour.
- Creating a branch from the switcher is `git branch <name> HEAD` followed by `git checkout <name>` (git_branch_create never switches).

## Watch out
- The switcher lists only this worktree's local branches: a branch checked out in another worktree cannot be checked out here.

In the code: src/modules/statusbar/GitBranchPill.tsx, statusbar/lib/gitBranch.ts, source-control/statusConsumers.ts (statusBar consumer), settings/store.ts statusBarGitBranch.
