# Plan: Remote SSH, Phase 4 — remote files, search and watch

**Date:** 2026-11-23
**Spec:** `docs/superpowers/specs/2026-11-22-remote-ssh-workspace-design.md` (sections 5 and 6, Security, Phases)
**Branch:** `dydev` (all work stays here until the SSH feature ships as 0.9.5-dy)
**Depends on:** Phase 3 (`2026-11-23-remote-ssh-phase3-transport.md`): a live `RemoteWorkspace` per host from `remote::registry`, `backend_for(Ssh)`, the askpass dialog, the connect UI.

## Goal

After connecting to a host, the user picks a folder on the server and gets a space whose env is `ssh:<host>`. In it the explorer, the editor (open, save, conflicts), file operations (create, rename, delete, copy, drag in), file search, grep and glob, live updates when files change on the server, and previews (images, PDF, DOCX, XLSX, HTML) all work, served by the agent. Git, terminals and LSP still refuse a remote space with a clear message (Phase 5 brings git and terminals; LSP is a non-goal), and AI tools refuse it until Phase 6.

## What exists and what is missing

- **Through the backend already** (Phase 1): `fs_read_dir`'s listing, `list_subdirs`, file read and write (`fs/file.rs`), mutations (`fs/mutate.rs`), copy, project memory, `fs_grep` / `fs_glob` (`fs/grep.rs`), and the agent serves all of them, including `Grep`, `Glob` and `ListFiles`.
- **Blocked for `Ssh`:**
  - Authorization: `WorkspaceRegistry::authorize` canonicalizes with `std::fs` on a host path, so no remote root can be authorized, and the agent's roots are never set.
  - The home directory: `resolveEnvHome` throws for `ssh`; nothing asks the server for `$HOME`.
  - A folder picker: `workspace_pick_folder` is this machine's native dialog.
  - File search (`fs/search.rs`: `fs_search`, `fs_warm_index`, `fs_list_files`) builds its index over `resolve_path(root)`, a host path.
  - Watching (`fs/watch.rs`) registers host paths; the protocol has no way to push an event.
  - The secret-path read policy (`read_policy::readable`) and the explorer's gitignore decorations take host paths.
  - Previews: asset grants check the policy on a host path; the HTML preview (`terax-doc`) serves files from this machine.

## Decisions for this phase

- **Authorization is per env, with the agent's roots following it.** For an `Ssh` env, `WorkspaceRegistry::authorize` canonicalizes through `backend_for(env).canonicalize` (on the server) and stores `Location { env: Ssh, path }`; covering works as for WSL (`covers` already compares env and path). Every change to a host's authorized roots (authorize, derived, revoke) recomputes that host's list and hands it to `remote::registry::set_roots`, which keeps it for reconnects (Phase 3). The client check stays first; the agent's check is defence in depth.
- **Browsing before authorizing.** The picker must list directories before any root is granted, but the agent refuses every path outside its roots. A new protocol call `BrowseDirs { path }` returns the canonical path and its subdirectory names only (no file names, no contents, no metadata beyond "is a directory"), outside the roots, like a folder dialog on this machine. `Home` returns the agent user's canonical `$HOME`. Both are new `Call`s; `PROTOCOL_VERSION` rises (agent and app ship together and the agent is hash-checked, so there is no mixed-version case).
- **A remote space opens after the user picks a folder.** Connect, then the picker (served by `BrowseDirs`, starting at `Home`, with a typed path), then a space with env `ssh:<host>` and that root. The space's home is the server's `$HOME`. Restoring a remote space at startup connects right away, as a user-started attempt (it may prompt; the user opened it before; owner's decision, 2026-11-23). If that attempt fails, or the connection is down for any other reason, the explorer shows the state, the reason and a Connect button as the fallback, instead of errors.
- **The file index runs on the agent.** `fs_search` / `fs_list_files` / `fs_warm_index` for `Ssh` use `Workspace::list_files` (the agent walks with the same `ignore` rules and returns the list); fuzzy ranking stays in the app, unchanged. The index cache is keyed by env, so a reconnect (a new session) rebuilds it.
- **Watching is a push stream.** New calls `Watch { id, paths }` and `Unwatch { id }`; the agent runs the core's watch engine (`terax_workspace_core::watch`, inotify) on its side, debounced as locally, and sends `Envelope::Changed { id, paths }`. The client routes it to the app's existing `fs:changed` event, so the frontend needs no change. Watches are kept per host in the registry and re-registered after a reconnect, followed by one `fs:changed` for every watched directory (anything may have changed while down).
- **Policies on remote paths.** The secret-path refusals (`confined::refusal`, already in the core and enforced by the agent for confined reads) apply to remote paths by their workspace spelling; `read_policy::readable` gains an env-aware entry point instead of resolving a host path. The explorer's gitignore decorations are skipped for `Ssh` until Phase 5 brings git.
- **Previews fetch bytes through the agent.** Asset grants for an `Ssh` path read with `Workspace::read` (ranged reads for media, the existing caps). The HTML preview publishes the page in memory and resolves its relative assets through the backend on request, confined to the page's directory tree and the authorized root (the spec's "in-memory publish path").
- **Refusals stay explicit.** Git, terminals, LSP, checkpoints, worktrees and AI tools refuse an `Ssh` space with a message naming the phase, never a host fallback; the guards from Phases 1 and 3 keep watching for host calls.

## Tasks

Task 2 runs before Task 1: authorizing a remote folder canonicalizes it on the server, and the agent refuses `Canonicalize` outside its roots, so authorization takes the canonical path from `BrowseDirs` (a remote root is always a directory).

### Task 1: Remote authorization and roots

- [x] `WorkspaceRegistry::authorize_in(env, path)`: local and WSL as today, `Ssh` through the backend's `canonicalize`; `workspace_authorize` uses it. The host's root list goes to `remote::registry::set_roots` on every change (authorize, `authorize_derived`, revoke).
- [x] Tests: with the in-process agent from Phase 3's registry tests, authorizing a remote folder lets the agent read inside it and nowhere else; revoking takes it away; a reconnect keeps it; authorizing while disconnected fails with `NotConnected`.
- [x] A shared test helper (`remote::testing`) that installs one app-wide registry with an in-process host serving a temp directory, since `registry::global` is set once per process.

Done. `WorkspaceRegistry::authorize_in` (used by `workspace_authorize`): local and WSL unchanged; `Ssh` takes the canonical path from `remote::registry::canonical_dir` (`BrowseDirs`, so only a directory), stores `Location { env: Ssh, path }`, and sends every root of that host to `set_roots`, which the registry keeps for reconnects. There is no revocation of plain roots in the app today (only derived worktree grants, which are local), so a host's list only grows for the app's lifetime. `remote/testing.rs` holds the in-process connector (its cut switch now per host) and `global_registry()` / `connected(host)` for tests that go through `backend_for`. Tests (`remote/authorize_tests.rs`, 4): resolved on the server and confining the agent, covered per env and per host, a second root added, roots back after a reconnect, a file refused (`NotADirectory`), a host never connected refused (`NotConnected`). Mutation: dropping the `set_roots` push fails three of them.

### Task 2: `Home` and `BrowseDirs`

- [x] Protocol calls and replies, the agent's handlers (`BrowseDirs` outside the roots, directory names only), the client methods, `PROTOCOL_VERSION` raised.
- [x] Tauri commands `remote_home(host)` and `remote_browse_dirs(host, path)`.
- [x] Tests: round trips over pipes; `BrowseDirs` never returns file names or anything for a file path; the agent's contract suite still passes; the over-SSH test runs both on dypod.

Done. `Call::Home` (the agent's `Options::home`, `$HOME` by default, canonicalized; a clear `NotFound` when unset) and `Call::BrowseDirs` (`Reply::Browse { path, dirs }`: canonical path, sorted subdirectory names, links to directories included, a file refused with `NotADirectory`), both outside the roots; `PROTOCOL_VERSION` 2. Client `RemoteWorkspace::home` / `browse_dirs`; commands `remote_home`, `remote_browse_dirs` (off the main thread). Tests: round trips, two agent tests (`tests/serve.rs`), one through the registry (browse before any root, files still refused). On dypod through the e2e test: home `/home/ubuntu`, its directories listed. The bundled agents were rebuilt (`pnpm build:agent`) for protocol 2.

### Task 3: Open a remote folder

- [x] Frontend: after a connect from the palette, a remote folder picker (`RemoteFolderPicker`: the current path, its subdirectories, up, a typed path, Open), then a space with env `ssh:<host>` and that root; `resolveEnvHome` asks `remote_home`; restoring a remote space connects first; the explorer shows the connection state and Connect while not ready.
- [x] Tests: the picker's states (loading, a listing, an error, a typed path); the space created with the right env and root; `resolveEnvHome` for `ssh`.

Done (frontend only).
- Picking a host in "Connect to SSH Host..." opens `RemoteFolderPicker`, which calls `ensureConnected` (a user-started attempt; resolves on `ready`, rejects with ssh's reason when this attempt fails, ignoring a `failed` left from an earlier one), then lists `remote_home` through `remote_browse_dirs`: the path, `..`, the folders, a typed path with Go, Open. Open calls the handler the app registers (`setFolderHandler`): `workspace_authorize` with the `ssh` env, a new space named `<folder> (<host>)` with that root and env, the env set before the first tab so it never starts on this machine.
- `resolveEnvHome` for `ssh` connects, then asks `remote_home`; a restored remote space therefore connects right away. If that fails, `RemoteGate` (around the explorer) shows the host, the state, the reason and Connect; on every new connection (`readyEpochs`) it authorizes the space's root again and remounts the explorer, so it reloads.
- The first tab of a remote space is a terminal, which refuses with "not available for a workspace on an SSH host" until Phase 5 (a space must have a tab).
- Tests: 15 in `remoteFolder.test.ts` (paths, `ensureConnected`'s outcomes, ready epochs, the picker's and the gate's states) and 3 for `resolveEnvHome`. Not yet seen in the running app (Task 8).

### Task 4: Files, editor and file operations on a remote space

- [x] Audit every command the explorer and the editor call for an `Ssh` env and fix what still takes a host path: the read policy (env-aware), the explorer's gitignore decorations (skipped), copy and drag-in from this machine (`import` through the backend), project memory, save conflicts (stamps from the agent).
- [x] Tests: the app's own command functions (`read_dir_entries`, `subdirs`, read, write, create, rename, delete, copy, import) against the in-process host, including a secret path refused and a path outside the root refused by both ends.

Done. What the audit found and fixed:
- `fs_move` compared host spellings to spot a move onto itself; every server path has the same one (`UNREACHABLE`), so every remote move was a silent no-op. Now `same_spelling` compares a server's paths as spelled.
- `fs_copy` checked `resolve_path(..).is_absolute()`, false for every server path, so every paste or drop was refused. Now `is_absolute_in` (`/...`, or `C:/...` on a Windows server).
- Copy and drag-in from this machine: the client's `import` is now an upload (`import::upload_item`, generic over `Workspace`): staged in `.terax-copy-*` in the destination, published with the no-replace `rename`, "name copy" on conflict, the staging removed whatever happens; files over `MAX_UPLOAD_FILE` (30 MiB, one frame) and symbolic links are refused.
- The secret-path policy checked a server path's host spelling, a placeholder no pattern matches; `read_policy::readable_in` now checks the server's spelling (AI tools and asset grants use it); `deny_runtime_in` skips this app's directory on a server (project memory).
- Gitignore decorations are skipped for `ssh` explicitly.
- Save conflicts need nothing: the agent's stamps are the file's mtime, and an edit made on the server moves them (tested).
- Tests: 4 in `remote/fs_tests.rs` (listing, subdirs, read, save, a change on the server, create, rename, move, conflict and replace, delete, upload with a copy name and no staging left, and refusals outside the folder), 3 for `upload_item` against a local workspace, 1 for the policy on a server path.

### Task 5: Search, grep and glob

- [x] `fs_search`, `fs_list_files`, `fs_warm_index` for `Ssh` through `Workspace::list_files`; the index cache keyed by env; `fs_grep`, `fs_grep_interactive`, `fs_glob` checked on a remote space.
- [x] Tests: fuzzy search finds a file in the in-process host; ignored files are left out; a reconnect rebuilds the index; a cancelled search stops.

Done.
- The finder's commands checked the root with the host's `is_dir()` (always false for a server); `search_root` now asks the workspace. A hit's path on a server is the root joined in the server's spelling.
- The index cache was invalidated by host spellings, one placeholder for every server path. Changes on a server are now spelled `ssh:<host>:<path>` (`remote_change_path`), for the cache's prefixes and for our own mutations (`note_changed_in`, `note_dir_changed_in`, used by every file command).
- Changes made on the server while nothing watches are not seen until a reconnect: on each `ready` the app drops that host's cache (`forget_host`). Task 6 makes this live.
- `fs_grep`, `fs_grep_interactive` and `fs_glob` already went through the backend; tested.
- Tests: 4 in `remote/search_tests.rs` (fuzzy search, gitignored and pruned folders left out, our own change seen at once, a server-side change seen after a reconnect, a cancelled search, a missing root, grep and glob).

### Task 6: Watching

- [ ] Protocol `Watch` / `Unwatch` / `Changed`; the agent's watcher (the core engine on inotify); the client's event routing; `fs_watch_add` / `fs_watch_remove` for `Ssh`; re-registration and one catch-up `fs:changed` after a reconnect.
- [ ] Tests: over pipes, a file created in a watched directory reaches the callback, debounced; `Unwatch` stops it; a reconnect re-registers; a watch the server refuses (the inotify limit) is reported in the connection log and does not end the session.

### Task 7: Previews

- [ ] Asset grants for `Ssh` paths read through the backend (ranged); the HTML preview's in-memory publish with relative assets fetched through the backend, confined.
- [ ] Tests: an image and a PDF grant on the in-process host; an HTML page with a relative stylesheet; `../` outside the page's tree and a secret path refused.

### Task 8: End to end, guards and docs

- [ ] On dypod (the owner's server; every login must succeed, see the fail2ban learning): open a folder, edit and save, a conflict when the file changed on the server, create, rename and delete, file search and grep, a change made over a separate `ssh` showing up through the watch, an image preview, and a drop and reconnect with the explorer recovering. Driven through the app's command functions in an ignored test, plus a GUI walkthrough by the owner (a dev build shares the installed Terax's control descriptor).
- [ ] Guards: the host-call guard covers every fs module for `Ssh`; a test that every Tauri command taking a `WorkspaceEnv` either works on `Ssh` or refuses with a message (no host path built from a remote one).
- [ ] TERAX.md (remote spaces: authorization, browsing, search, watch, previews), a learning for anything non-obvious, and this plan marked complete.

## Risks

- **Large trees.** `ListFiles` returns the whole list in one frame. The core's index caps a walk at 50,000 entries and 16 MiB of packed paths, under the 32 MiB frame limit, so a bigger tree is truncated (as locally), not failed.
- **inotify limits** (`max_user_watches`) on the server: a refused watch leaves that directory without live updates; Task 6 decides how the explorer shows it (refresh still works).
- **Latency.** Each explorer expansion is a round trip (about 30 ms to dypod). Acceptable for v1; batching `read_dir` with metadata is a later optimisation.
- **HTML previews** that load assets by absolute URL or from the network are not affected; only relative assets go through the agent.
