# Plan: Remote SSH, Phase 2 — protocol and agent

**Date:** 2026-11-23
**Spec:** `docs/superpowers/specs/2026-11-22-remote-ssh-workspace-design.md` (sections 2, 3, Testing, Phases)
**Branch:** `dydev`
**Depends on:** Phase 1 (`2026-11-22-remote-ssh-phase1-workspace-core.md`): the `Workspace` trait, `HostWorkspace`, `read_confined` and the contract suite in `terax-workspace-core`.

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

- [ ] `Envelope`, `Call`, `Reply`, `WireError`; serde derives on the core types listed above; the `ExecSpec` wire type and its UTF-8 check.
- [ ] Tests: every `Call` and `Reply` variant round-trips; every `io::ErrorKind` the core returns and every `ConfinedError` variant survives the trip; a non-UTF-8 exec argument is refused; an unknown message decodes to an error, not a panic.

### Task 3: Handshake

- [ ] `Hello` and the pure `negotiate(local, remote) -> Result<Session, Mismatch>`.
- [ ] Tests: same version passes; a different protocol version, or a missing required capability, fails with a reason.

### Task 4: Agent

- [ ] `terax-agent` crate: `serve(reader, writer)` and `main` (`--stdio`; `--version`). Handshake, `SetRoots`, dispatch to `HostWorkspace::new(WorkspaceEnv::Local)`, worker pool, `Cancel`, `Ping`, exit on EOF.
- [ ] Root checks for every path argument (including both ends of rename, replace and copy, `read_confined`'s roots, and `exec`'s cwd).
- [ ] Unit tests drive `serve` over in-memory pipes: a request before the handshake is refused, a path outside the roots is refused even when it exists, a link inside a root to outside it is refused, a cancelled grep stops early, EOF ends the loop.

### Task 5: Client

- [ ] `RemoteWorkspace::connect(reader, writer, env, roots)`: handshake, `SetRoots`, the call table, the reader thread, a deadline per call, `Ping` for liveness. Implements `Workspace`.
- [ ] Tests over in-memory pipes against a scripted fake agent: replies routed to the right waiter when they arrive out of order; a dropped connection fails waiting and later calls with `ConnectionAborted`; an oversize write fails before anything is sent.

### Task 6: The contract over a real child process

- [ ] Integration test in `terax-agent`: spawn the built binary (`CARGO_BIN_EXE_terax-agent`) with piped stdio, connect `RemoteWorkspace`, run `contract::run` on a temp directory.
- [ ] Plus: confinement through the full stack, killing the agent mid-call fails the call promptly, the agent exits when the client drops its stdin.
- [ ] Runs on Windows here and on the Linux and macOS CI jobs.

### Task 7: Linux binaries (trial for the packaging risk)

- [ ] Install `zig` and `cargo-zigbuild` (asks first). Build `terax-agent` for `x86_64-unknown-linux-musl` and `aarch64-unknown-linux-musl` on Windows; record size and confirm the binaries are static.
- [ ] With the owner's approval, on a real server: copy the agent over, then run the contract suite through `ssh <host> <path>/terax-agent --stdio` as the transport (a manual test harness, not app code). This exercises the real network, the real Linux file system and git before Phase 3 builds the connection.
- [ ] Record the outcome in the spec's packaging section (zigbuild on Windows, or CI artifacts).

### Task 8: Guards and docs

- [ ] Extend the dependency guard to `terax-remote-protocol` and `terax-agent` (no `tauri`, `tokio` or app).
- [ ] TERAX.md: one paragraph on the agent and protocol.
- [ ] A learning for anything non-obvious.

## Risks

- **Frame size versus large files.** A 32 MiB cap means editor saves above that fail until chunked upload (Phase 4). Large reads already go through bounded requests.
- **musl and `grep-*` / `ignore`.** Pure Rust, so they should cross-compile; `notify` is not needed by the agent until Phase 4.
- **Blocking calls with no deadline.** Every client call gets a deadline (a long one for `exec` and `grep`, from their own timeouts), so a stuck agent cannot hang a Tauri command forever.
