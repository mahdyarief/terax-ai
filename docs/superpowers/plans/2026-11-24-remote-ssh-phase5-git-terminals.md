# Plan: Remote SSH, Phase 5 — git and terminals on a server

**Date:** 2026-11-24
**Spec:** `docs/superpowers/specs/2026-11-22-remote-ssh-workspace-design.md` (sections 2, 3, 5; Phases step 5)
**Branch:** `dydev` (until the SSH feature ships as 0.9.5-dy)
**Depends on:** Phase 4 (`2026-11-23-remote-ssh-phase4-remote-fs.md`): remote spaces, the fs commands, `SetWatch`/`Changed`, the command guard (`modules/ssh_commands_guard_tests.rs`).

## Goal

In a server's space, source control (status, diff, stage, commit, branches, fetch/pull/push), git history and the explorer's git state work, run by git on the server; terminals open on the server in the space's folder, with shell integration (cwd tracking, prompt marks), resize, input and exit, at local speed for ordinary use and without starving file calls when one floods. A dropped connection ends the server's terminals with a clear message and a restart.

## What exists (from the survey, 2026-11-24)

- **Git runs through `backend_for(ws).exec`** (`git/process.rs::run_git_full`); the agent runs it in a root-confined cwd. Every git command still fails on SSH because `git/utils.rs::canonical_dir` canonicalizes `resolve_path(..)` (the placeholder) and keeps a host `local_path`; `operations.rs` authorizes with `registry.is_authorized(&local_path)`; `plumbing::lock_for_repo` is keyed by `local_path`; `ensure_git_available` caches a disconnect as "git not installed" for 60 s.
- **Host-only git areas:** watch (`git/watch.rs`, walks and stats on the host), worktree add/remove/backlinks, checkpoints, task worktrees, task state, project config.
- **Terminals:** the app's PTY stack (`modules/pty`: `session::spawn_with` over portable-pty traits, reader pipeline, replay, hub, handoff) is free of the platform below `MasterPty`/`SlavePty`/`Child`. The protocol has no stream. `portable-pty 0.9` builds for both musl agents (trial 2026-11-24, `pnpm build:agent`).

## Decisions

- **Git: make the repo resolution env-aware, then reuse everything.** `ResolvedGitDirectory` gets the workspace spelling as the identity; `local_path` becomes `Option<PathBuf>` (None on SSH) and only host-only code may read it. Authorization uses `registry.covers(env, ws_path)`. Repo locks key on `(env scope, ws_path)`. An exec that fails with `NotConnected` is reported as not connected, never cached as "not installed".
- **Git watch on a server uses the remote watch.** `git_watch` for SSH registers the repo root, `.git`, `.git/refs/heads` and `.git/refs/remotes` with `RemoteWatches` and emits `terax:git-changed` on any delivery for them (coarser than the local classifier; the panel refreshes, which is what it needs). The frontend watches for every env that `git_watch` accepts.
- **Explorer git state:** `gitignored` on SSH comes from one `git check-ignore --stdin -z` exec per listing (only for directories inside a repo), so the tree dims ignored entries as locally.
- **Host-only git features refuse on a server, by name:** worktree add/remove, checkpoints, task worktrees and project config return "not available for a workspace on an SSH host" (v1). They are listed as `Refused` in the guard. Worktree list and prune work (exec only), with `entry_exists` through the backend.
- **Terminals: a PTY stream in the protocol, portable-pty on the agent.** New calls `PtyOpen { id, spec }` (cwd in workspace spelling, size, env), `PtyWrite { id, bytes }`, `PtyResize { id, size }`, `PtyClose { id }`, `PtyCredit { id, bytes }`; pushed `Envelope::PtyData { id, bytes }` and `PtyExit { id, code }`. `PROTOCOL_VERSION` 4. The agent reads a PTY only while the client's unacknowledged bytes for it are under a window (1 MiB), in chunks of at most 64 KiB, so one flood cannot fill the shared output and delay file replies; the client credits bytes as the app's session reader consumes them, so the client's read loop only hands data off and never blocks.
- **The app side plugs in under `Session`.** A `RemotePty` implements portable-pty's `MasterPty`, `Child` and `ChildKiller` over the stream; `spawn_with` runs unchanged (reader pipeline, replay, hub, handoff all reused). Foreground-process checks report no local pid for it.
- **The shell and its integration are set up on the server by the agent:** the login shell from the passwd entry (`$SHELL`, else `/bin/sh`), the same integration scripts (compiled into the agent from `src/modules/pty/scripts/`) written under the server's `~/.cache/terax/shell-integration`, the env the app sets locally (TERM, COLORTERM, TERAX_TERMINAL, TERAX_BLOCKS; no control endpoint), a UTF-8 locale when the server has none set.
- **A drop ends the server's terminals** (v1, per spec): each gets `PtyExit` with a reason ("the connection to <host> ended"), shown in the terminal's existing failure overlay with Retry, which opens a new one once the host is ready again.
- **OSC 7 on a server is the server's path:** the frontend's MSYS mapping (`/c/x` → `C:/x`) is skipped in a server's space.

## Tasks

### Task 1: Git on a server

- [x] `canonical_dir` / `ResolvedGitDirectory` env-aware (no `resolve_path` on SSH), authorization by `covers`, locks by workspace path, `ensure_git_available` not caching a disconnect.
- [x] `worktree list`'s `entry_exists` through the backend; host-only commands refuse by name on SSH.
- [x] Tests (`remote/git_tests.rs`, a real repo served by the in-process agent): status, diff, stage/unstage, commit, log, show, branches and checkout, discard, apply hunks, upstream info; a path outside the folder refused; the refusals' messages. Guard entries move to `Works`/`Refused`.

Done: `ResolvedGitDirectory::local` is `None` on a server and `local_path()` refuses there, so every host-only path (worktree add/remove, checkpoint create/restore, tasks) says so instead of using the placeholder; repo resolution authorizes with `covers`, the repo lock keys on the workspace path, and a server that cannot be reached is no longer cached as "git not installed". `remote/git_tests.rs` (5 tests) runs status, diff, stage/unstage, commit, log, commit files and diffs, branches (create, rename, checkout, upstream), discard, review summary/file/apply hunks, merge base, a remote with fetch/push/pull, worktree list/prune and checkpoint list/prune on the in-process agent, and checks the refusals. Guard: 30 git commands `Works`, 11 `Refused`; `git_watch` waits for Task 2.

### Task 2: Git state live on a server

- [x] `git_watch` for SSH through `RemoteWatches`, emitting `terax:git-changed`; released with the subscription; resumed with the rest on `ready`.
- [x] Explorer `gitignored` on SSH via `git check-ignore`.
- [x] Frontend: source control watches on SSH too.
- [x] Tests: a commit made on the server (outside the app) reaches the subscriber; ignored entries flagged in a listing.

Done, with a finding: the core's folder watch reports a folder only when its listing differs after the debounce, and git rewrites `index` and refs through `<name>.lock` renamed into place, so the listing ends as it began and a folder watch never sees a commit. Git's own files (`HEAD`, `index`, `packed-refs`, each loose branch ref) are watched as files, by their stamp; the agent watches a folder it normally leaves out (`.git`) when files in it are registered, since the core checks files only inside watched folders. Content-only edits to tracked files made outside the app are seen on the panel's focus refresh (folders report listings). `remote/git_watch.rs` classifies a change as head, index, refs or worktree; tests: the pure classifier and subscription counts, and a file added and a commit made on the in-process agent's server reaching the subscriber. Explorer dimming on a server comes from one `git check-ignore -z --stdin` per listing. Also fixed Task 1's local refusal message, which had started naming the workspace spelling instead of the host path (`tests/git_operations.rs`).

### Task 3: The PTY stream in the protocol and the agent

- [x] Messages and `PROTOCOL_VERSION` 4; client: per-PTY routing of `PtyData`/`PtyExit` from the read loop (hand-off only), `pty_open/write/resize/close/credit`.
- [x] Agent: PTY sessions on portable-pty (a thread per PTY, not a worker), credit window, chunking, exit codes, kill on `PtyClose` and when the session ends; shell discovery, integration scripts and env.
- [x] Tests over pipes: echo through a shell, resize, exit code, a 4 MiB flood while file calls keep answering promptly, close kills, session end kills.

Done. `PROTOCOL_VERSION` 4 and capability `pty.v1`. Only `PtyOpen` is a request (answered `PtyOpened { program }`); keystrokes, resize, close and credit are one-way envelopes, so typing never waits for a round trip and an id that has just ended is ignored. The client picks the id and registers its channel before asking, so no output is missed. Agent (`crates/terax-agent/src/pty.rs`): three threads per terminal (keys, reader, waiter); the reader sends at most 64 KiB a frame and stops while 1 MiB is uncredited; the waiter drops the master when the process ends (which ends ConPTY's reader), waits up to 2 s for the reader's last output, then sends `PtyExit` under the same lock data is sent with, so nothing follows it. Login shell from `$SHELL` (else `/bin/sh`) with the app's integration scripts written under the server's `~/.cache/terax/shell-integration`. Found on dypod: an agent built on Windows carries the scripts with CRLF and bash refuses them; the agent writes them with Unix line ends. Tests: the window with a fake endless reader, the shell setup per shell, six terminal tests over pipes through the real client (command, cwd, env and exit code; keys and resize; a 5 MB flood beside file calls; close; refused cwd; session end kills), and `over_ssh.rs::a_terminal_runs_on_the_server_over_ssh` on dypod (bash with prompt marks and OSC 7, `stty size` after a resize, a 2.7 MB flood with a stat beside it in 24 ms, `exit 4`).

### Task 4: Terminals in a server's space

- [ ] `RemotePty` (portable-pty traits over the client); `pty_open` for SSH: cwd authorized by `covers` in server spelling, `spawn_with` with the remote system; no local pid.
- [ ] A drop exits every server terminal with the reason; Retry after reconnect.
- [ ] Frontend: OSC 7 without the MSYS mapping on SSH; the first tab of a server space opens on the server.
- [ ] Tests: a terminal through the app's session on the in-process agent (output, input, resize, exit, a cut connection).

### Task 5: End to end, guards and docs

- [ ] On dypod (every login must succeed): git status/commit/log in a folder, a terminal running `pwd`, `tput cols` after a resize, a flood with the explorer still answering, a drop and a new terminal.
- [ ] Guard: git and pty entries updated; TERAX.md; a learning for anything non-obvious; this plan marked complete.

## Risks

- **musl PTY at runtime:** the build works; opening a PTY on the server is proven only by Task 3's over-ssh run.
- **Latency of keystrokes:** each write is one frame (about 30 ms to dypod round trip, one way is half). Acceptable; no batching of input.
- **Terminals do not survive a drop** (v1). A daemonized agent is in the spec's "Later".
