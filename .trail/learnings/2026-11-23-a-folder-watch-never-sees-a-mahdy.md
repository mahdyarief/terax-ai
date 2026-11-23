---
title: "A folder watch never sees a git commit; watch git's files by stamp"
author: Mahdy
date: 2026-11-23
branch: dydev
cost: { minutes: 126, tokens: 615850 }
id: l-432c5039
scope: repo
status: active
src: agent
touches:
  - docs/superpowers/plans/2026-11-24-remote-ssh-phase5-git-terminals.md
  - src-tauri/crates/terax-agent/src/watch.rs
  - src-tauri/src/lib.rs
  - src-tauri/src/modules/fs/tree.rs
  - src-tauri/src/modules/fs/watch.rs
  - src-tauri/src/modules/git/commands_review.rs
  - src-tauri/src/modules/git/operations.rs
  - src-tauri/src/modules/git/watch.rs
  - src-tauri/src/modules/remote/git_tests.rs
  - src-tauri/src/modules/remote/git_watch.rs
---

## Decided
Source control on a server watches git's own files (HEAD, index, packed-refs, loose branch refs) as files, by their stamp, beside the repository's folders. The agent watches a folder it normally leaves out (`.git`) only when files inside it are registered in the same set.

## Tried and ruled out
Watching `.git` and `.git/refs/heads` as folders only: nothing was reported for `git add` or `git commit`. The core watcher reports a folder when its listing differs after the debounce, and git rewrites index and refs through `<name>.lock` renamed into place, so the listing ends as it began. Registering the files without their folder also failed: the core checks registered files only inside watched folders (`resolve_changes_with` iterates `set.dir_keys()`), and the agent skipped `.git` as a folder.

## Watch out
Content-only edits to tracked files made outside the app are not reported by folder watches either; the source control panel relies on its focus refresh for those. Editor tabs work because they register the parent folder and the file together.

In the code: `src-tauri/src/modules/git/watch.rs::remote_subscribe_in`, `src-tauri/src/modules/remote/git_watch.rs`, `src-tauri/crates/terax-agent/src/watch.rs` (`Watches::set`), test `remote::git_tests::a_commit_made_on_the_server_reaches_source_control`.
