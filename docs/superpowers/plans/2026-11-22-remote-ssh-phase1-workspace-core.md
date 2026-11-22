# Remote SSH — Phase 1: Workspace Core Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Put every file-system, search, watch and process operation that depends on `WorkspaceEnv` behind one `Workspace` abstraction, implemented today by a host backend (Local and WSL), with its engines in a Tauri-free crate that the Phase 2 agent can link. No user-visible change.

**Architecture:** A new crate, `crates/terax-workspace-core`, owns `WorkspaceEnv`, path mapping, a sync `Workspace` trait in two layers (primitives and engines), the `HostWorkspace` backend, and the engines (search/grep/glob/index, the watch planner and portable backend, the bounded file readers). Tauri commands keep their names and payloads and become thin shells: authorize, pick the backend for the env, call it. Git logic stays in the app; only its process spawning and its direct file access route through the trait.

**Tech Stack:** Rust 2021 / Tauri v2, `ignore`, `grep-regex`, `grep-searcher`, `globset`, `notify` 8, cargo workspace (`crates/*`).

## Global Constraints

- Approved spec: `docs/superpowers/specs/2026-11-22-remote-ssh-workspace-design.md` (Phase 1 of 6).
- **No behaviour change.** Command names, argument shapes, result shapes, error strings and events stay identical. The frontend is not touched in this phase.
- The trait is **synchronous**. Today's code is sync inside `fs::blocking` / `spawn_blocking`, and the remote backend (Phase 3) will block on an RPC reply from the same blocking threads. An async trait would force a rewrite of every caller for no gain.
- Dispatch is `&dyn Workspace` (or `Arc<dyn Workspace>`). The dynamic call is negligible next to a syscall.
- `terax-workspace-core` must not depend on `tauri`, `tokio`, or any app module. Enforced by a test (Task 12).
- Every production change keeps the existing tests green, and each task adds a failing test first where it introduces new behaviour (the contract suite, the guard tests).
- Rust checks per task: `cargo clippy --all-targets --locked -- -D warnings` and `cargo nextest run --locked` (fallback `cargo test --locked`), run from `src-tauri/`. Frontend checks at the end: `pnpm lint`, `pnpm check-types`, `pnpm test`.
- The Rust pty tests are `#[cfg(all(test, unix))]` and do not run on Windows. The core's contract suite must run on Windows and Linux.
- Land in small slices. Each task below is one reviewable commit (or a few) that leaves `main`-quality code.

---

## The abstraction

```rust
// crates/terax-workspace-core/src/workspace.rs (sketch, final names may change)

pub trait Workspace: Send + Sync {
    fn env(&self) -> &WorkspaceEnv;

    // ── primitives ───────────────────────────────────────────────
    fn metadata(&self, path: &str) -> io::Result<Meta>;           // follows links
    fn symlink_metadata(&self, path: &str) -> io::Result<Meta>;
    fn canonicalize(&self, path: &str) -> io::Result<String>;
    fn read_dir(&self, path: &str) -> io::Result<Vec<DirEntry>>;
    fn read(&self, path: &str, limit: ReadLimit) -> io::Result<ReadOutcome>;   // bounded / head / range
    fn write_atomic(&self, path: &str, bytes: &[u8]) -> io::Result<()>;
    fn create_file(&self, path: &str) -> io::Result<()>;          // fails if it exists
    fn create_dir_all(&self, path: &str) -> io::Result<()>;
    fn rename(&self, from: &str, to: &str) -> io::Result<()>;
    fn remove_file(&self, path: &str) -> io::Result<()>;
    fn remove_dir_all(&self, path: &str) -> io::Result<()>;
    fn copy_file(&self, from: &str, to: &str) -> io::Result<u64>;

    // ── engines (run next to the data) ───────────────────────────
    fn grep(&self, req: GrepRequest, sink: &mut dyn GrepSink) -> Result<GrepSummary, WsError>;
    fn glob(&self, req: GlobRequest) -> Result<Vec<String>, WsError>;
    fn list_files(&self, req: ListRequest) -> Result<Vec<String>, WsError>;
    fn watch(&self, roots: WatchSet, sink: Box<dyn WatchSink>) -> Result<WatchHandle, WsError>;
    fn exec(&self, spec: ExecSpec) -> Result<ExecOutput, WsError>;   // git and other helpers
}
```

- Paths in the trait are **workspace paths** (what the UI shows: `/home/x/...` in WSL, `C:\...` locally). `HostWorkspace` maps them with today's `resolve_path`. The remote backend will pass them through unchanged.
- `Meta` and `DirEntry` are plain structs (kind, size, mtime millis, readonly, is_symlink), not `std::fs::Metadata`, so they can cross the Phase 2 wire.
- `ExecSpec` carries program, args, cwd, env additions, stdin bytes, output cap and timeout. It is the generalization of today's `build_git_command` + `run_git_with`.
- `backend_for(env: &WorkspaceEnv) -> Arc<dyn Workspace>` lives in `src-tauri` (it will later look up live SSH connections). In Phase 1 it always returns a `HostWorkspace`.

## File Structure

| Path | Role |
|---|---|
| `src-tauri/crates/terax-workspace-core/Cargo.toml` | New crate; deps: `serde`, `ignore`, `grep-*`, `globset`, `notify` (cfg linux/windows), `thiserror` |
| `…/src/env.rs` | `WorkspaceEnv`, scope helpers, WSL path mapping and distro-name validation (moved from `modules/workspace.rs`) |
| `…/src/workspace.rs` | The `Workspace` trait, `Meta`, `DirEntry`, `ReadLimit`, `ReadOutcome`, `ExecSpec`, `WsError` |
| `…/src/host.rs` | `HostWorkspace`: primitives on `std::fs` + `resolve_path`, engines by calling the modules below |
| `…/src/read.rs` | Bounded reads, line-safe heads, atomic writes (moved from `fs/file.rs`) |
| `…/src/search/` | Grep, glob, list-files and the file index engine (moved from `fs/grep.rs`, `fs/search.rs`, `fs/index.rs`) |
| `…/src/watch/` | Watch planner (`plan`) and the portable `notify` backend (moved from `fs/watch/`); macOS backend stays app-side behind the same sink |
| `…/src/exec.rs` | Process runner with cap and timeout, Local and WSL spawn (generalized from `git/process.rs`) |
| `…/tests/contract.rs` | Contract suite every `Workspace` backend must pass; runs against `HostWorkspace` on a temp dir now, against the agent in Phase 2 |
| `src-tauri/src/modules/workspace.rs` | Keeps `WorkspaceRegistry` and the commands; re-exports `WorkspaceEnv`; gains `backend_for` |
| `src-tauri/src/modules/fs/*.rs`, `git/*.rs` | Call the trait instead of `std::fs` / `Command` |

---

## Tasks

### Task 1: Scaffold the crate and move `WorkspaceEnv`

- [ ] Add `crates/terax-workspace-core` to `[workspace] members` and `default-members` in `src-tauri/Cargo.toml`; depend on it from the app crate.
- [ ] Move `WorkspaceEnv`, `from_option`, `is_wsl`, `resolve_path`, `wsl_path_to_unc`, `wsl_path_to_host`, `wsl_drvfs_to_windows`, `validate_wsl_distro_name`, `is_safe_distro_name` and their tests into `env.rs`. Keep `serde` attributes byte-identical (the IPC shape must not change).
- [ ] Re-export from `modules/workspace.rs` (`pub use terax_workspace_core::env::*;`) so no caller changes in this task.
- [ ] Add a serde round-trip test pinning the JSON for `Local` and `Wsl { distro }`, which is what the frontend sends.
- [ ] Verify: clippy, nextest, `pnpm tauri dev` starts.

### Task 2: Define the trait and the contract suite (red)

- [ ] Write `workspace.rs` with the trait and data types above.
- [ ] Write `tests/contract.rs` as a generic `fn run_contract(make: impl Fn(&Path) -> Box<dyn Workspace>)` covering, for every primitive: success on a temp tree, the error kind for a missing path, refusal to overwrite in `create_file`, atomic replacement in `write_atomic` (no partial file visible), `read` limits (exact limit, one byte over, head never splitting UTF-8), devices/FIFOs refused where today's code refuses them (Unix only), symlink vs. target metadata.
- [ ] The suite fails to compile/run until Task 3 provides a backend.

### Task 3: `HostWorkspace` primitives (green)

- [ ] Implement the primitives on `std::fs` through `resolve_path`, porting the exact semantics of today's helpers: `read_file_sync`, `read_bounded`, `head_of`, `write_atomic`, `stat_path`, `canonical_wsl_path` (move these into `read.rs` / `host.rs`).
- [ ] Run the contract suite against `HostWorkspace` on Windows and Linux CI.
- [ ] Add `backend_for` in `src-tauri/src/modules/workspace.rs`.

### Task 4: Migrate `fs/file.rs`

- [ ] `fs_read_binary`, `fs_read_bytes`, `fs_read_head`, `fs_read_file`, `fs_write_file`, `fs_canonicalize`, `fs_stat` call `backend_for(&env)`; their bodies shrink to authorize → call → map result.
- [ ] Existing `file.rs` tests keep passing; tests of moved helpers move with them.

### Task 5: Migrate `fs/tree.rs`, `fs/mutate.rs`, `fs/supersede.rs`

- [ ] `fs_read_dir`, `list_subdirs`, `fs_create_file`, `fs_create_dir`, `fs_rename`, `fs_move`, `fs_delete`, `fs_delete_batch` go through the primitives.
- [ ] Watch for Windows-specific behaviour in `mutate.rs` (case-only renames, read-only attribute, trash vs. delete). Whatever is host-only stays in `HostWorkspace` behind the primitive, not in the command.

### Task 6: Move the copy engine (`fs_copy`) behind `Workspace::import`

- [x] `fs_copy` sources are always paths on this machine (OS clipboard paste, desktop drop) and the destination is a workspace directory, so the copy is an engine, not a composition of primitives: `Workspace::import(source, dest_dir, rename_on_conflict)`. The host backend keeps today's staged, no-replace publish; the remote backend will implement it as an upload.
- [x] Destination authorization stays host-side until Task 11.
- **Moved to Task 11:** `fs_read_project_memory`, `asset_grant`/asset serving, `ai_read_file`, `ai_read_dir`. Each opens a file and then checks the *opened handle's* real path (`/proc/self/fd`, `F_GETPATH`, `GetFinalPathNameByHandleW`) against the registry, so they are confinement checks rather than plain reads. Their three `opened_path` copies also differ on purpose (the AI path keeps the lossless verbatim Windows path; the others compare the stripped spelling), so merging them is a security-semantics change that belongs with the registry redesign, where a remote agent must run the same check on the server.
- `read_policy.rs`, `clipboard.rs`, `preview.rs` and `local_file.rs` stay host-only.

### Task 7a: Move the grep and glob engines

- [x] The `ignore`/`grep-searcher` walk of `fs/grep.rs` moved into the core (`grep.rs`) with serializable `GrepRequest` (regex or smart-case literal), `GlobRequest` and their responses, so they can cross the Phase 2 wire unchanged. Results are returned whole (capped), not streamed: today's commands return whole responses too, and a streaming sink can be added when the remote palette needs one.
- [x] `Workspace::{grep, glob}`; `fs_grep`, `fs_grep_interactive` and `fs_glob` are thin shells. Cancellation stays app-side (`Supersede` tickets) and reaches the engine as a `cancel` closure.
- [x] Contract suite gains engine cases: `.ignore` respected outside a git repo, hidden and binary files skipped, globs filter, result caps report truncation, cancellation, bad and empty patterns refused. Mutation-checked: dropping the ignore rule fails the suite.
- `ai_glob` / `ai_grep` walk only authorized paths through the registry and move with Task 11.

### Task 7b: Move the file-index walk

- [x] `FileIndexState` (cache, warmers, invalidation from the watcher and from mutations) stays app-side, keyed by `(env, workspace root)`; only the walk that fills it is `Workspace::list_files(root, show_hidden, max_depth, cancel)`. `Packed` and `FileIndex` moved to the core and are serializable.
- [x] `fs_search`, `fs_warm_index`, `fs_list_files` keep ranking (nucleo) app-side over the cached list.
- [x] Contract suite gains a walk case (pruned and ignored directories, hidden entries, depth cap, directory flags, cancellation). Mutation-checked.
- The invalidation prefix is still computed on the canonical host path, because watcher events and mutation notes report host paths (UNC for WSL). Task 8 decides the event path space; the remote backend needs events in workspace paths. The root checks in `search.rs` (`search_root`) also still stat the host path and move with it.

### Task 8: Move the watcher engine into the core

- [x] `plan.rs` (planner), `portable.rs` (`notify`) and the engine (`Watcher`: debounce, listing re-reads, file stamps, `Sink`, `Backend`) moved into the core's `watch/`. The app supplies two small traits: `ChangeNotes` (the file-finder index) and `Reporter` (internal taps and the `fs:changed` emit, which is the only Tauri part).
- [x] The watcher is a service, not a per-call operation, so it is not a `Workspace` method: a remote workspace will run this same engine in the agent and stream its reports. `FsWatchState`, the commands and path authorization stay in the app.
- [x] The macOS FSEvents backend stays in the app and plugs into the core's `Backend`/`Sink`, re-exported under their old paths; `macos.rs` changed by one line (its test fixture's `Sink` field). It cannot be compiled on Windows, so it is verified by the macOS CI job only.
- [x] The 37 engine tests moved with it; an end-to-end test drives the real `notify` backend (create a file, expect one debounced report and the change note).
- Paths stay canonical host paths. The remote backend will report workspace paths from the agent; how the app tells the two apart is decided with Task 11's per-env registry.

### Task 9: Generalize the process runner and route git through it

- [x] The bounded runner (`proc/capture.rs`) and `hide_console` moved into the core's `exec` module, re-exported under their old app paths (the shell tool keeps using them). `build_git_command` became the generic `exec::build_command(env, program, cwd, args)`: direct for local, `wsl.exe -d … --cd … --exec` for WSL.
- [x] `Workspace::exec(ExecSpec)` runs a program with bounded output and a deadline; `run_git*` keep their signatures and build an `ExecSpec { program: "git", .. }`. The call sites do not change.
- [x] Git hardening stays in the app: `core.fsmonitor=` and `--no-ext-diff`/`--no-textconv` injection, the safety environment, stripping inherited repository redirects (`env_remove`, applied before `env`), the WSL environment-override refusal and the unsafe-distro `GitError`.
- [x] `ensure_git_available` goes through `run_git`, so through `exec` too. Parsers and all higher-level git logic stay in the app.
- Arguments are `OsString`; the Phase 2 wire format decides how non-UTF-8 arguments cross (expected: refuse them for remote workspaces).

### Order: Task 11 before Task 10

Git holds `repo.local_path`, the registry's canonical *host* path (`\\wsl.localhost\<distro>\...` for WSL), and derives every path from it. The `Workspace` primitives take *workspace* paths, and `HostWorkspace` maps a Linux path into the distro, so a UNC path cannot be passed through. Routing git's file access first would only swap `std::fs` for a host-path shim and redo all of it after the registry changes. So the registry goes first (11a), then each module moves to workspace paths and the backend together (11b with 10).

### Task 11: Key `WorkspaceRegistry` by workspace

- [x] **11a, registry internals.** A grant is a `Location { env, path }`: the canonical path in the workspace's own `/`-separated spelling (`C:/Users/x` local on Windows, `/home/x` in a distro or on a remote host). Containment is component-wise (`/a/b` is within `/a`, `/a/bc` is not) and only within the same workspace (distro names compare case-insensitively), so a root in one workspace never covers the same text in another. The host-path methods (`authorize`, `is_authorized`, `authorize_derived`, `revoke_derived`) stay as adapters through `Location::of_host`, which reads a `\\wsl.localhost\` / `\\wsl$\` path as that distro and anything else as local: on local and WSL roots this is the same coverage as before, and no caller changes. Tests: host spellings map to the right workspace; containment never crosses workspaces (mutation-checked).
- [ ] **11b, callers to workspace paths**, module by module; delete each host adapter once nothing uses it.
  - [x] API: `registry.canonicalize_in(ws, path)` (through the backend, same 1 s cache), `registry.covers(env, canonical)` and `within(env, path, root)`. `Location::in_workspace` reads `/mnt/<drive>/...` inside a distro as that local drive (drvfs), because that is where `wsl_path_to_host` sends it and where the host-path adapters file it: grants made either way name the same location (tested on Windows against `canonical_wsl_path`).
  - [x] fs mutations (`resolve_authorized_root`, `authorize_mutation_entry`) and `copy_destination` authorize canonical workspace paths through the backend. Errors now print the workspace path.
  - [ ] Git, with Task 10 (foundation done).
  - Staying on the host adapters until their remote counterpart exists: `workspace_authorize` and `workspace_current_dir` (they return the host spelling the frontend keys on), spawn cwd for shell/pty/lsp/control (a process spawns on the host; the remote shell runs in the agent), and the fs watcher's `prepare_dirs` (the watcher engine runs on host paths; the remote one runs in the agent).
- [ ] **11c, confined reads.** One backend operation that opens a file, checks the opened handle's real path lies inside an authorized root, applies the secret-path policy to that real path, and reads with a limit. Port `fs_read_project_memory`, asset serving and `ai_read_file`/`ai_read_dir` to it, keeping each caller's error messages. The agent implements the same check on the server.

### Task 10: Route git's direct file access through the primitives (with 11b)

`ResolvedGitDirectory` carries the env and the workspace path; every `std::fs` call on a repo path goes through `backend_for(env)`, and authorization through the `*_in` registry methods. One commit per module, smallest blast radius first (production sites; tests stay on the host file system):

- [x] Foundation: `ResolvedGitDirectory` gains `ws_path` (canonical workspace path, through the backend) and `backend()`; `local_path` stays until the last module moves. Repository authorization (`authorized_repo_root`, `plumbing::repo_root`) uses `registry.covers`. `resolve_within_repo` takes the backend and workspace paths, with `relative_to` in place of host `strip_prefix`; its callers in operations, plumbing (`relative_spec`) and checkpoint (submodule check) move with it, and `read_text_file` reads through the backend.
  - A rooted `rel` (`/etc/passwd`) is now refused up front: `PathBuf::join` used to replace the root with it (landing outside, so refused), but joining text would read it as a path inside the repository. Caught by `file_diff_rejects_paths_outside_the_repository`.
  - Mutation testing showed nothing covered the containment check itself (a link inside the repository pointing out of it); `resolve_within_repo_rejects_a_link_out_of_the_repository` now does, for an existing and a not-yet-existing target (symlink on Unix, junction on Windows).
- [x] `review.rs` (untracked reads, the two occupancy checks), `project_config::read_confined` and `utils::canonical_dir`'s directory check go through the backend. New primitive `Workspace::read_link` (anything that is not a link fails with `InvalidInput` on every OS; in the contract suite). A new test covers `read_confined`'s containment check (`.terax` linked out of the repository), which nothing exercised.
  - Staying on the host: `task_state.rs` (the task store is app state under `~/.terax`, and `lock_operation` takes an OS file lock, for which there is no primitive; a remote lock belongs to the agent), and `project_config`'s worktree placement (`worktree_directory`, `validate_location`), which moves with `worktree.rs` because it shares `create_dirs_confined` and the notion of a home directory, which is the server's for a remote workspace.
- [x] **Local-only features stay on the host** (decided 2026-11-22). Checkpoints, task worktrees, creating and removing worktrees and the git state watcher already refused WSL; moving them behind the backend would need primitives with no non-host user to test them against (`hard_link` for publishing the worktree id, setting a file's mtime for the racy-git index copy, permissions, an OS file lock), and the watcher runs on the host engine. Their guards now require `WorkspaceEnv::is_local()` instead of refusing WSL, so any other kind of workspace (a remote host in Phase 2) is refused by default. Remote support for these is designed separately once the agent exists. The two WSL-specific checks stay `is_wsl()`: `wsl.exe` not forwarding the environment (`process.rs`) and the WSL spelling of `git_path` (`utils.rs`).
  - Host-only, so on Task 12's allowlist: `checkpoint.rs`, `task_worktree.rs`, `worktree.rs` (with `project_config`'s worktree placement), `task_state.rs`, `git/watch.rs`, and `plumbing.rs`'s `git_dir` / `common_dir` (host paths for those modules).
  - Phase 2: the refusal messages and the watcher's `"wsl-unsupported"` reason name WSL; give the remote case its own wording (check what the frontend keys on first).

Each step: the module's existing tests (`*_tests.rs`, `hardening_tests.rs`, `edge_tests.rs`) stay green unchanged.

### Task 12: Lock the invariants

- [ ] Guard test (Rust, reads source files): no `std::fs::`, `File::open`, `File::create` or `Command::new` in `src-tauri/src/modules/fs/*.rs` and `src-tauri/src/modules/git/*.rs` outside an explicit allowlist (host-only files such as `clipboard.rs`, `local_file.rs`, and test modules), with a reason string per allowlisted file. Same idea as `compositingBudget.test.ts`.
- [ ] Guard test: `terax-workspace-core`'s `Cargo.toml` has no `tauri`, `tokio` or app dependency.
- [ ] Update `TERAX.md` (architecture section) with one paragraph: workspace operations go through `Workspace`; never call `std::fs` on a workspace path in a command.

### Task 13: Full verification and handoff

- [ ] Rust: clippy and nextest on Windows; CI green on Linux and macOS.
- [ ] Frontend: `pnpm lint`, `pnpm check-types`, `pnpm test` (unchanged code, should be green).
- [ ] Manual smoke test in `pnpm tauri dev` with an isolated identifier: open a local folder and a WSL folder; explorer, editor save, rename/delete, copy-paste, search, grep, file watch (edit a file outside Terax), git status/diff/commit, worktree create/remove, checkpoint.
- [ ] Measure: cold `fs_read_dir` of a large folder and a full `fs_search` before and after, to confirm the trait adds no measurable cost.
- [ ] Save a learning on anything non-obvious found during the migration.

## Order, size and risk

- Tasks 1–3 are the foundation (small). Tasks 4–8 are the fs side; Task 9 is small and high value; Task 10 is the largest single piece; Tasks 11–13 close the phase.
- Highest-risk areas: Windows-specific semantics in `mutate.rs`/`copy.rs`, the watcher's debounce timing, and `checkpoint.rs`/`worktree.rs`, which write into `.git`. Each has a dense existing test suite; keep those tests untouched as the safety net.
- Not in Phase 1: PTY spawning (Phase 5), LSP (non-goal), the protocol and agent (Phase 2), any frontend change.
