# Plan: Remote SSH, Phase 2 — protocol and agent

**Date:** 2026-11-23
**Spec:** `docs/superpowers/specs/2026-11-22-remote-ssh-workspace-design.md` (sections 2, 3, Testing, Phases)
**Branch:** `dydev`
**Depends on:** Phase 1 (`2026-11-22-remote-ssh-phase1-workspace-core.md`): the `Workspace` trait, `HostWorkspace`, `read_confined` and the contract suite in `terax-workspace-core`.

**Status (2026-11-23): complete.** Every task is done and committed on `dydev` (`36e80e2` to the Task 8 commit). Left open, none of them a known defect:

- CI has not run Phase 2 on Linux or macOS (CI runs only for `main`; the owner deferred CI for `dydev` until every phase is done). The agent and protocol pass clippy for Linux (gnu and musl), and the contract passed on a real Linux server over SSH, which covers the Linux paths end to end.
- The aarch64 agent builds and is static but has not run on ARM hardware.
- The agent reports the crates' version (`[workspace.package]`, 0.9.3), not the app's (0.9.4-dy). Phase 3 decides which version the app compares before it reinstalls an agent.
- `RemoteWorkspace::import` (uploading from this machine) is `Unsupported`.

## Goal

A `terax-agent --stdio` binary that serves the `Workspace` trait over its stdin and stdout, and a client, `RemoteWorkspace`, that implements the same trait by calling it. The existing contract suite (`contract::run`) passes against `RemoteWorkspace` talking to a local agent child process over pipes, with no SSH involved. Nothing in the app changes yet; Phase 3 replaces the local pipes with an `ssh` process.

## Decisions for this phase

- **Crates.** `crates/terax-remote-protocol`: framing, messages, handshake (pure) and the client (`RemoteWorkspace`, sync, std threads, generic over any `Read`/`Write` pair). `crates/terax-agent`: the server loop as a library plus the `terax-agent` binary. Neither depends on Tauri, Tokio or the app (guarded, as the core is).
- **Wire format.** Frames are a u32 big-endian length plus a MessagePack body (`rmp-serde`). Maximum frame 32 MiB, enforced on both sides before allocating. Bytes travel as MessagePack binary, never base64.
- **Messages.** `Hello` (each side, first), `Request { id, call }`, `Response { id, result }`, `Cancel { id }`, `Ping`/`Pong`. `Call` has one variant per `Workspace` method, so the protocol mirrors the trait one-to-one and a new trait method fails to compile until it is carried.
- **Wire types.** Core types that already derive serde (grep and glob requests and responses) travel as they are. The rest (`Meta`, `DirEntry`, `ReadRequest`/`ReadOutcome`, `WriteOutcome`, `ConfinedRead`/`ConfinedOutcome`, `Captured`, the file index) gain serde derives in the core. `ExecSpec` has its own wire type: arguments and environment are strings, and a non-UTF-8 argument is refused on the client with `InvalidInput` (the Phase 1 note on exec arguments). Errors travel as `{ kind, message }`, mapped back to the same `io::ErrorKind`, and `ConfinedError` keeps its variants.
- **Handshake.** Protocol version, agent version, OS, arch and capabilities. A protocol mismatch fails cleanly with an error Phase 3 turns into "reinstall the agent".
- **Confinement on the agent.** The client sends the authorized roots (`SetRoots`) after the handshake. The agent checks every path argument against them after canonicalizing it on the server (for a path that does not exist yet, its nearest existing ancestor), and `exec`'s working directory too. This is defence in depth; the client's `WorkspaceRegistry` still decides first.
- **Concurrency.** The agent reads frames on one thread and runs each request on a bounded worker pool, so a slow grep does not hold up a `stat`; replies are written under one lock. `Cancel` sets the flag that `grep` and `list_files` already poll. The client keeps a table of waiting calls, one reader thread routes replies, and losing the connection fails every waiting call at once with `ConnectionAborted`.
- **Lifetime.** The agent exits when stdin closes, so a dropped connection never leaves an orphan.
- **Not in this phase.**
  - Streams and credit flow control: their first users are the watcher (Phase 4) and terminals (Phase 5); grep, glob and the file list fit in bounded replies for now.
  - `import` (copy local files into the workspace): for a remote workspace this is an upload from the client, so `RemoteWorkspace::import` returns `Unsupported` until Phase 4 adds chunked upload.
  - Writes larger than a frame return a clear error until the same chunked upload exists.
  - `WorkspaceEnv::Ssh`: the client takes its `WorkspaceEnv` from the caller; the variant arrives in Phase 3, where the app first needs it.

## Tasks

Each task: tests first, `cargo clippy --workspace --all-targets -- -D warnings`, the full suite green, one commit, then ask before the next.

### Task 1: Framing

- [x] `terax-remote-protocol` crate in the workspace; `frame::encode` and an incremental `FrameReader` that accepts bytes in any split and yields whole frames.
- [x] Tests: partial frames, several frames in one read, a zero-length frame, an oversize length refused before any allocation, a truncated stream at EOF reported as such; a proptest that any split of a stream of frames decodes to the same frames.
  - Also a blocking `read_frame`/`write_frame` pair (the agent and client read a pipe directly); the body grows with what arrives instead of trusting the prefix. Mutation-checked: dropping either size check, the truncation check, the buffer compaction or the header offset each fails a test.

### Task 2: Messages and wire types

- [x] `Envelope`, `Call`, `Reply`, `WireError`; serde derives on the core types listed above; the `ExecSpec` wire type and its UTF-8 check.
- [x] Tests: every `Call` and `Reply` variant round-trips; every `io::ErrorKind` the core returns and every `ConfinedError` variant survives the trip; a non-UTF-8 exec argument is refused; an unknown message decodes to an error, not a panic.
  - Byte fields (`ReadOutcome`, `ConfinedOutcome`, `Captured`, a write's body, exec stdin) use `serde_bytes`, so they travel as MessagePack binary; a test checks a 1 MiB read encodes to about 1 MiB.
  - `ConfinedError::Refused` now holds a `String` (it held a `&'static str`, which cannot be rebuilt from the wire).
  - A file index from the wire is validated as it is decoded (`Packed` deserializes through `PackedParts`): offsets must be in order, in bounds and on character boundaries, or `get` could panic in the app.
  - `io::ErrorKind` crosses by name (31 kinds, a superset of the ten the app checks); an unknown name decodes as `Other` and keeps its text.
  - Mutation-checked: bytes as arrays, the character-boundary check, one error kind and the `Refused` mapping each fail a test.

### Task 3: Handshake

- [x] `Hello` and the pure `negotiate(local, remote) -> Result<Session, Mismatch>`.
- [x] Tests: same version passes; a different protocol version, or a missing required capability, fails with a reason.
  - `PROTOCOL_VERSION = 1` and the capability `workspace.v1`. The session keeps the peer's `Hello` and the capabilities both sides offer; a different agent version is left to Phase 3 (reinstall), not refused here. Mutation-checked: the protocol check, the required-capability check and the intersection each fail a test.

### Task 4: Agent

- [x] `terax-agent` crate: `serve(reader, writer)` and `main` (`--stdio`; `--version`). Handshake, `SetRoots`, dispatch to `HostWorkspace::new(WorkspaceEnv::Local)`, worker pool, `Cancel`, `Ping`, exit on EOF.
- [x] Root checks for every path argument (including both ends of rename, replace and copy, `read_confined`'s roots, and `exec`'s cwd).
- [x] Unit tests drive `serve` over in-memory pipes: a request before the handshake is refused, a path outside the roots is refused even when it exists, a link inside a root to outside it is refused, a cancelled grep stops early, EOF ends the loop.
  - Serving is the default with no arguments (no `--stdio` flag); `--version` prints `terax-agent <version>`, anything else exits 2. The version is the crate's (`[workspace.package]`, today 0.9.3); Phase 3 must decide which version the app compares.
  - Two checks per path (`roots.rs`): `follow` (where it leads: reads, listings, search roots, exec cwd, `create_dir_all`) and `entry` (where the entry sits: remove, rename, replace, `symlink_metadata`, `read_link`). Writes (`write_atomic`, `create_file`, the destination of a copy) need both, so a file link pointing out cannot be written through. Paths must be absolute with no empty components; a path that does not exist yet is judged by its nearest existing ancestor. `.` and `..` are resolved by the OS inside that ancestor (as the operation will resolve them) and refused in the names after it, including an entry named `..` (Task 6 found the contract needs `canonicalize("dir/..")`; refusing only unresolved dots also closes `root/..` reading as inside `root`). The walks do not follow links, so a search only checks its root.
  - `exec` runs with the SSH user's rights, as a shell would; only its starting directory is confined, and it defaults to the first root.
  - A request id already in flight is refused; a reply over the frame limit becomes an error reply; a message that cannot be decoded, or a second `Hello`, ends the session.
  - Tests: 9 unit (roots, cancel, the read loop) and 14 over pipes. Mutation-checked: 14 mutants (each path check, the write-through check, exec's cwd, cancel, the handshake, duplicate ids, the parser) all fail a test.

### Task 5: Client

- [x] `RemoteWorkspace::connect(reader, writer, env, roots)`: handshake, `SetRoots`, the call table, the reader thread, a deadline per call, `Ping` for liveness. Implements `Workspace`.
- [x] Tests over in-memory pipes against a scripted fake agent: replies routed to the right waiter when they arrive out of order; a dropped connection fails waiting and later calls with `ConnectionAborted`; an oversize write fails before anything is sent.
  - In `terax-remote-protocol::client`. The reader thread holds only the waiters, never the writer, so dropping the `RemoteWorkspace` closes the agent's input and the agent exits.
  - Deadlines: 15 s to connect (Hello and `SetRoots`), 60 s per file operation, 10 min per walk, and an `exec`'s own timeout plus 60 s. A call past its deadline fails with `TimedOut` and sends `Cancel`; a late reply is dropped. A walk polls its caller's cancel flag every 50 ms and sends `Cancel` once.
  - Errors: the agent's errors come back as the same `io::ErrorKind` and text; `read_confined` keeps the `ConfinedError` variant; a refusal by the agent's roots is `PermissionDenied` (and `Outside` for a confined read). `list_files` returns `None` on failure as well as on cancel, since the trait has no error to give.
  - `import` (uploading from this machine) returns `Unsupported`; a later phase adds it.
  - Tests: 11 against a scripted fake agent. Mutation-checked: 8 mutants fail a test (the closed check, both `Cancel` sends, the size check, waking waiters on close, routing by id, the required capability, the confined error mapping); a ninth survived because its branch was redundant, and the branch was removed.

### Task 6: The contract over a real child process

- [x] Integration test in `terax-agent`: spawn the built binary (`CARGO_BIN_EXE_terax-agent`) with piped stdio, connect `RemoteWorkspace`, run `contract::run` on a temp directory.
- [x] Plus: confinement through the full stack, killing the agent mid-call fails the call promptly, the agent exits when the client drops its stdin.
  - `crates/terax-agent/tests/contract.rs`: the whole `contract::run` passes through `RemoteWorkspace` and the child process, plus the roots holding through the stack, killing the agent during an `exec` (fails with `ConnectionAborted` within a second), the agent exiting when the client is dropped, and `--version`.
  - Found and fixed on the way: (1) on Windows the agent's stdin and stdout handles were inheritable, so a command run by `exec` kept the stream open after the agent died and the client noticed only when that command ended; the agent now clears `HANDLE_FLAG_INHERIT` on them at start (Unix needs nothing: the child's 0-2 are replaced, the rest are close-on-exec). (2) A grep or listing cancelled before it starts is now answered by the client without a round trip, as the host does; the agent would otherwise finish a small search before the `Cancel` arrived.
  - Mutation-checked: the unresolved-dot check, the inherit fix and both pre-cancel checks each fail a test.
- [ ] Runs on Windows here and on the Linux and macOS CI jobs.

### Task 7: Linux binaries (trial for the packaging risk)

- [x] Install `zig` and `cargo-zigbuild` (asks first). Build `terax-agent` for `x86_64-unknown-linux-musl` and `aarch64-unknown-linux-musl` on Windows; record size and confirm the binaries are static.
- [x] With the owner's approval, on a real server: copy the agent over, then run the contract suite through `ssh <host> <path>/terax-agent --stdio` as the transport (a manual test harness, not app code). This exercises the real network, the real Linux file system and git before Phase 3 builds the connection.
- [x] Record the outcome in the spec's packaging section (zigbuild on Windows, or CI artifacts).
  - Outcome: zigbuild on Windows works (2.7 and 2.5 MiB, static); the contract passes on the owner's server `dypod` (Ubuntu 24.04, x86_64) over SSH in 3.6 s. Details in the spec. The core now calls `SYS_renameat2` directly, since zig's musl lacks the wrapper.
  - The agent is served with no arguments, so the transport is `ssh -T <host> <path>/terax-agent` (the plan's `--stdio` was dropped in Task 4). Running the harness from Git Bash needs `MSYS2_ENV_CONV_EXCL=TERAX_AGENT_SSH_PATH`, or Git Bash rewrites the remote path into a Windows one.
  - The test agent stays at `~/.cache/terax-agent-test/terax-agent` on the server for reruns.

### Task 8: Guards and docs

- [x] Extend the dependency guard to `terax-remote-protocol` and `terax-agent` (no `tauri`, `tokio` or app).
- [x] TERAX.md: one paragraph on the agent and protocol.
- [x] A learning for anything non-obvious.
  - The guard (`crates/terax-workspace-core/tests/dependencies.rs`) now reads all three manifests; the three crates may depend on each other and on nothing named `tauri*`, `tokio*` or another `terax*`. Probed: adding `tokio` to the agent's manifest fails it.
  - TERAX.md gains "Remote agent and protocol". Learnings: `2026-11-23-the-remote-agent-must-hide-its-mahdy.md` (Task 6) and `2026-11-23-linux-agents-are-built-and-tested-mahdy.md` (building on Windows, the harness).

## Risks

- **Frame size versus large files.** A 32 MiB cap means editor saves above that fail until chunked upload (Phase 4). Large reads already go through bounded requests.
- **musl and `grep-*` / `ignore`.** Pure Rust, so they should cross-compile; `notify` is not needed by the agent until Phase 4.
- **Blocking calls with no deadline.** Every client call gets a deadline (a long one for `exec` and `grep`, from their own timeouts), so a stuck agent cannot hang a Tauri command forever.
