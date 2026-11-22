---
title: Never run cargo fmt on the whole crate here; the tree is not rustfmt-clean
author: Mahdy
date: 2026-11-22
branch: dydev
cost: { minutes: 1, tokens: 1856 }
id: l-735dc9ed
scope: repo
status: active
src: agent
---

## Decided
Format only the lines you wrote. CI runs clippy and tests but no `cargo fmt --check`, and much of the existing Rust (upstream files included) is not rustfmt-clean, so formatting a crate rewrites unrelated code.

## Watch out
`cargo fmt -p terax` reformatted 19 untouched files (pty/agent_detect.rs alone by 228 lines) and also turned the touched file's CRLF line endings into LF. Running rustfmt on a single file is no safer: that file is usually not clean either, so it rewrites old code next to yours.

In the code:
- If it happens: `git restore` the files you did not mean to change (only when they have no uncommitted work of yours), then re-apply your edit to the HEAD version rather than keep the formatted file.
- Check before committing: `git diff --stat` lists only the files you meant to touch.
