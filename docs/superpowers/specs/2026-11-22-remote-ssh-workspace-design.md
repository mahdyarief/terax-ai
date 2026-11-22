# Design: Remote SSH Workspaces

**Date:** 2026-11-22
**Status:** Draft design, pending review
**Branch:** `dydev`

## Goal

Open a folder on a Linux server over SSH and work in it as if it were local: explorer, editor, previews, search, git, source control, terminals and AI tools all operate on the remote machine. The experience matches VS Code Remote-SSH.

## Non-goals (v1)

- Remote LSP (WSL workspaces refuse LSP today; SSH does the same at first).
- Remote hosts other than Linux (no macOS, Windows or BSD servers).
- Terminals and agents that survive a dropped connection. v1 restarts them on reconnect; see "Later".
- Port forwarding, remote containers, multi-hop UI beyond what `~/.ssh/config` already expresses.

## Decisions

- **Remote agent, not agentless.** Terax installs a small static binary (`terax-agent`) in `~/.terax-server/<version>/` on the server and talks to it over one SSH connection. File operations, search, file watching, git and terminals run on the server, next to the data.
- **System OpenSSH is the transport.** Terax spawns the user's `ssh` (OpenSSH 9.9 on this machine), so `~/.ssh/config`, `ProxyJump`, `ssh-agent`, keys, certificates and `known_hosts` all work unchanged. Terax never implements the SSH protocol and never sees a private key.
- **One connection per host, multiplexed.** Every request, stream and terminal for a host shares the stdio of a single `ssh -T host terax-agent --stdio` process. Windows OpenSSH has no `ControlMaster`, so per-operation `ssh` invocations would re-handshake each time; multiplexing avoids that.
- **Linux x86_64 and aarch64 only**, built as static musl binaries so they run on any distribution.
- **The agent ships inside the installer** and is uploaded over the same SSH connection. Servers without internet access (common inside corporate networks) work.
- **Prompts go through Terax.** Passwords, OTP codes, passphrases and new host keys are answered in a Terax dialog via `SSH_ASKPASS` with `SSH_ASKPASS_REQUIRE=force`. Nothing typed is stored.
- **Functional core, thin shells.** The protocol, framing, install planning and connection state machine are pure and tested without a network. Tauri commands and React stay thin, per TERAX.md.

## Why this design

Today `WorkspaceEnv` is `Local | Wsl`, and WSL works because a WSL path maps to a Windows path: `resolve_path()` turns `/home/x` into `\\wsl.localhost\<distro>\home\x`, and roughly 30 fs commands then use plain `std::fs`. Git and the PTY wrap their processes in `wsl.exe`. SSH has no such mapping, so every file operation must cross the network.

Doing that agentless (SFTP plus `ssh host cmd`) breaks down on exactly the features that make an editor feel local:

- **Search and grep** would pull every file across the network. The agent runs the same ripgrep libraries (`ignore`, `grep-searcher`, `globset`) on the server and streams only matches.
- **File watching** would need polling. The agent runs `notify` (inotify) on the server and pushes events.
- **Git** would pay a connection's latency per command, and dozens of git commands run per refresh. The agent runs git locally on the server.

VS Code reached the same conclusion with its server. Unlike early VS Code Server, `terax-agent` never listens on a TCP port: it speaks only over the stdio of the authenticated SSH session.

## Architecture

```
Terax (Windows/macOS/Linux)                         Linux server
┌──────────────────────────────────────┐   one     ┌─────────────────────────────┐
│ React UI ── invoke ── Tauri commands  │   ssh     │ terax-agent --stdio         │
│                          │            │  process  │  ├ fs (read/write/stat/...) │
│              Workspace trait (core)   │◀════════▶│  ├ search / grep / glob     │
│        ┌────────┬────────┴────────┐   │  framed   │  ├ watch (inotify)          │
│      Local     Wsl           Ssh ───┼──RPC over──▶│  ├ git (process runner)     │
│   (std::fs)  (UNC paths)  (RemoteClient)  stdio   │  └ pty (portable-pty)       │
└──────────────────────────────────────┘           └─────────────────────────────┘
```

### 1. Extract a shared workspace core (prerequisite, no behaviour change)

Every fs module calls `tauri::` directly, so the logic cannot run inside an agent. Move the pure parts into a new crate, `crates/terax-workspace-core`:

- file reads (bounded, head, binary), atomic writes, stat and canonicalize, directory listing, create/rename/move/delete, copy
- search, grep and glob (the ripgrep-based code in `fs/grep.rs`, `fs/search.rs`)
- the watcher wrapper around `notify`
- the process runner behind `git/process.rs` (every git process is built in one place, `build_git_command`)

Git logic itself (diff, review, worktree, checkpoint, parsers) stays in the app: it only needs its process spawning and its direct file access routed through the trait, so the same logic drives a remote repository unchanged.

Behind it sits a synchronous `Workspace` trait in two layers: primitives (metadata, read_dir, bounded read, atomic write, create, rename, remove, copy) and engines that run next to the data (grep, glob, list files, watch, exec). `HostWorkspace` (Local, and WSL as Local plus path mapping) implements it in the core. The trait is sync because today's callers already run inside blocking tasks; the remote backend blocks on its RPC reply the same way. The detailed slicing is in `docs/superpowers/plans/2026-11-22-remote-ssh-phase1-workspace-core.md`. Tauri commands become thin shells: authorize, pick the `Workspace` for the env, call it. This step changes no behaviour, keeps every existing test green, and is worth doing even before SSH ships.

### 2. Protocol (`crates/terax-remote-protocol`)

- **Framing:** length-prefixed frames (u32 big-endian, maximum frame size enforced) carrying MessagePack (`rmp-serde`). Binary file contents travel as bytes, without base64.
- **Messages:** `Request { id, method }`, `Response { id, result | error }`, `StreamData { stream, chunk }`, `StreamCredit { stream, bytes }`, `StreamEnd { stream, reason }`, `Ping`/`Pong`.
- **Handshake:** the first exchange carries protocol version, agent version, OS, arch and the agent's capabilities. A mismatch fails cleanly and triggers reinstall.
- **Streams** (terminal output, watch events, grep results, large reads) use credit-based flow control, mirroring the existing PTY ACK credits, so one busy terminal cannot starve file reads.
- **Methods** mirror the `Workspace` trait one-to-one. Initial surface: the fs commands that touch files (about 35 of the 40 fs/workspace commands; WSL discovery, the native folder dialog and clipboard stay local), the git runner, and `pty.open/write/resize/close`.

All of it is pure: encode and decode, a request table and the credit accounting, tested without I/O.

### 3. Agent (`crates/terax-agent`)

- A single static binary per architecture, built for `x86_64-unknown-linux-musl` and `aarch64-unknown-linux-musl` (expected size 3–5 MB stripped).
- It implements the protocol on stdin/stdout by calling `terax-workspace-core`, the same code the local workspace runs.
- It enforces confinement itself: requests carry the authorized roots, and the agent rejects paths outside them after canonicalizing on the server. This is defence in depth on top of the client's `WorkspaceRegistry` check.
- Terminals use `portable-pty` on the server, multiplexed as streams.
- It exits when stdin closes, so a dropped connection never leaves an orphan process.

### 4. Transport and connection (`src-tauri/src/modules/remote/`)

**Connect sequence:**

1. Spawn `ssh -T -o ServerAliveInterval=15 -o ServerAliveCountMax=3 <host> <bootstrap>` with `SSH_ASKPASS` set to Terax's askpass helper and `SSH_ASKPASS_REQUIRE=force`.
2. A short POSIX `sh` bootstrap prints `uname -sm` and whether `~/.terax-server/<version>/terax-agent` exists with the expected SHA-256.
3. If it is missing, Terax streams the bundled binary for that arch over the same session: `cat > tmp`, verify the SHA-256 on the server, `chmod 700`, then atomically `mv` into place. The directory is created `0700`.
4. `exec` the agent with `--stdio`. Handshake. The connection is up.

**Askpass:** the existing `terax-cli` binary gains an askpass mode. OpenSSH runs it and passes the prompt; it relays the prompt to the running app over the existing control protocol (`terax-control-protocol`, with the endpoint authentication, message limits and timeouts `control.rs` already enforces); the app shows a dialog naming the host; the answer is returned on stdout and never stored or logged. `terax hook` already blocks on a user decision this way (permission prompts, up to 900 s), so askpass reuses a proven path rather than adding a new IPC channel. A new or changed host key appears in that same dialog, with OpenSSH's fingerprint text shown verbatim.

**Connection state machine (pure):** `Idle → Connecting → Installing → Handshaking → Ready → Reconnecting → Ready | Failed`. Each host has one connection shared by all spaces on it. Heartbeats run over `Ping`. On a drop: editors keep their buffers (unsaved text survives), saving is disabled with a visible state, and reconnects retry with backoff. Terminals show "Disconnected" and offer restart.

### 5. Client side of each subsystem

| Subsystem | Today (Local / WSL) | SSH |
|---|---|---|
| fs (read, write, stat, tree, mutate, copy) | `std::fs` on host / UNC path | `RemoteClient` calls the agent |
| search, grep, glob, list files | ripgrep libs over host path | agent runs the same code, streams matches |
| watch | `notify` on host / UNC | agent's inotify, pushed as a stream |
| git (all three command modules) | process, or `wsl.exe git` | agent runs git; the existing parsers stay client-side |
| terminals | `portable-pty` local / `wsl.exe` | agent PTY stream, same `PtyState` attachment model |
| previews (HTML, images, PDF, DOCX, XLSX) | asset grants over local paths | bytes fetched through the agent; HTML preview serves from an in-memory publish |
| LSP | local; refused for WSL | refused (non-goal) |
| AI tools (read, glob, grep, edit, run) | local with deny-list | through `Workspace`; deny-list applies to remote paths; remote command execution always needs approval |

`WorkspaceEnv` gains `Ssh { host: String }` (`host` = an alias from `~/.ssh/config` or `user@host[:port]`). Scope keys become `ssh:<host>`, so settings, the recent list and the explorer root stay per remote.

### 6. UI and UX

- **Connect to Host…** opens from the command palette and the space switcher. It lists hosts parsed from `~/.ssh/config` (`Host` entries without wildcards) plus recent hosts, and accepts free-form `user@host`.
- After connecting, a remote folder picker (served by the agent) opens. The result is a space whose env is `ssh:<host>`.
- The status bar shows the host and state: Connecting, Installing agent, Connected, Reconnecting. Clicking it offers Reconnect, Disconnect and Show log.
- Remote spaces get a distinct avatar badge, so a remote terminal is never mistaken for a local one.
- The connection log (bootstrap and ssh stderr, with secrets redacted) is available from the status bar for troubleshooting.

## Security

- **No secrets at rest.** Passwords, OTPs and passphrases pass from the askpass dialog to `ssh` and are dropped. Keys stay with OpenSSH and `ssh-agent`.
- **Host keys:** never `StrictHostKeyChecking=no`. A new or changed key needs explicit confirmation in the dialog, and a changed key is shown as a warning, not a routine prompt.
- **Agent integrity:** the SHA-256 of the bundled binary is checked on the server before install and at every connect. The install directory is `0700`. The agent runs with the user's own rights and nothing more.
- **No listening sockets.** The agent speaks only over the SSH session's stdio.
- **Confinement on both ends:** the client's `WorkspaceRegistry` (keyed per env) and the agent's own root check.
- **AI:** the secret-path deny-list (`.ssh/`, `.aws/`, …) applies to remote paths. Remote command execution by AI always requires approval. Private terminals stay private.
- **Input validation:** the host string is validated before it reaches `ssh` (no leading `-`, no whitespace or control bytes), so it can never be read as an option.

## Packaging

- CI builds both musl agents (`cargo-zigbuild`) on Linux and attaches them as release artifacts.
- `tauri.conf.json` bundles them as resources. The Windows installer, which is built on the owner's machine, either pulls the CI artifacts or cross-compiles with `cargo-zigbuild` on Windows. This needs a trial run before Phase 2 is planned in detail.
- The agent version is tied to the app version. A newer app installs its own agent beside the old one; old versions are cleaned up after 30 days.

## Testing

- **Pure units:** framing (partial frames, oversize, garbage), protocol round-trips, credit accounting, the connection state machine, install planning (arch detection output → action), host-string validation, `ssh_config` host parsing.
- **Agent integration:** run `terax-agent --stdio` as a local child process over pipes, with no SSH involved. This covers every method against a temp directory and runs on the Linux CI job.
- **Core refactor:** the existing fs, git and pty test suites must pass unchanged after step 1.
- **End-to-end:** a Linux CI job with an `sshd` container covers key auth, password auth via a scripted askpass, first install, agent upgrade, a dropped connection with reconnect, and a changed host key being refused.
- **Manual:** Windows client to a real server (x86_64 and ARM64), including `ProxyJump` and a corporate server without internet access.

## Phases

1. **Workspace core extraction.** No user-visible change. Unblocks everything else.
2. **Protocol and agent**, tested over local pipes.
3. **Transport:** ssh spawn, askpass via `terax-cli`, install, handshake, state machine.
4. **Remote fs, search and watch:** explorer, editor, previews and search work on a remote space.
5. **Remote git and terminals:** source control, git history and the PTY.
6. **AI tools, UX polish, packaging and release.**

Each phase ends green on the full test suite and is reviewable on its own. Phase 1 is the largest refactor; phases 4 and 5 are where users first see value.

## Risks and open questions

- **Scale.** About 115 commands touch `WorkspaceEnv`. Phase 1 must land in small slices (fs, then search, then git) to stay reviewable.
- **Windows packaging** of Linux binaries, while Windows installers are built locally (see the dy release learning). Needs a decision after a `cargo-zigbuild` trial.
- **Askpass on Windows:** `SSH_ASKPASS_REQUIRE=force` is supported by OpenSSH 8.4+ (9.9 here). Older client versions need a fallback message asking the user to set up key auth.
- **HTML preview** currently serves local files through the `terax-doc` scheme; remote assets need an in-memory publish path. Exact design is deferred to Phase 4.
- **Terminal survival across drops.** v1 restarts terminals after a reconnect. A per-user agent daemon on a Unix socket, which `--stdio` would attach to, could keep them alive. Deferred.

## Later

- Terminals and running tasks that survive disconnects (daemonized agent).
- Port forwarding, so a dev server on the remote opens in a preview tab.
- Remote LSP.
- macOS servers.
