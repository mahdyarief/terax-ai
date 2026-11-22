---
title: Authorization must compare exact path spellings; to_canon and to_string_lossy can alias two real paths
author: Mahdy
date: 2026-11-22
branch: dydev
cost: { minutes: 1, tokens: 4685 }
id: l-e6fcf2ed
scope: repo
status: active
src: agent
touches:
  - src-tauri/src/modules/workspace.rs
  - src-tauri/crates/terax-workspace-core/src/host.rs
---

## Decided
A path that is not valid Unicode is never authorized. `Location::of_host` returns `None` for it, the registry neither covers it nor accepts it as a root, and `HostWorkspace::canonicalize` fails with `InvalidData` instead of returning a lossy spelling. Every authorization input in this app is a string (frontend, git output), so such a path only shows up after resolving a link, and refusing it there is safe.

## Tried and ruled out
Keying grants by `to_canon` strings (the first cut of phase 1 task 11a). `to_canon` calls `to_string_lossy`, so `repo\xff` on Unix, or a name with an unpaired surrogate on Windows, spells as `repo\u{FFFD}`, the same text as a real sibling with that name. A grant on one then covered the other. `PathBuf::starts_with`, used before, compared exactly.

## Watch out
Any security comparison done on strings must refuse a path whose `to_str()` is `None` before it converts the path. The linux-only `invalid_utf8_sibling_cannot_alias_an_authorized_root` test in project_memory.rs protects only that reader's own lossless check, not the registry, so it did not catch this. Windows can make such names: `OsString::from_wide(&[.., 0xD800])`, which NTFS accepts.

In the code:
- src-tauri/src/modules/workspace.rs: `Location::of_host`, `inexact()`; tests `a_non_unicode_sibling_cannot_alias_an_authorized_root` and `a_path_that_cannot_be_spelled_exactly_is_never_covered`.
- src-tauri/crates/terax-workspace-core/src/host.rs: `canonicalize`; unix test `a_canonical_path_that_is_not_unicode_is_refused`.
