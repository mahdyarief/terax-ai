# Plan: Remote SSH, Phase 3 — transport and connection

**Date:** 2026-11-23
**Spec:** `docs/superpowers/specs/2026-11-22-remote-ssh-workspace-design.md` (section 4, Security, Phases)
**Branch:** `dydev` (all work stays here until the SSH feature ships as 0.9.5-dy)
**Depends on:** Phase 2 (`2026-11-23-remote-ssh-phase2-protocol-agent.md`): `terax-agent`, `RemoteWorkspace`, the static Linux builds.

## Goal

From the app, connect to a host named in `~/.ssh/config` (or `user@host`), install the bundled agent on it if needed, and hold a live `RemoteWorkspace` for that host: prompts (password, passphrase, OTP, host key) answered in a Terax dialog, a visible connection state, reconnect after a drop, and a log for troubleshooting. `WorkspaceEnv` gains `Ssh { host }`, and `backend_for` returns the host's connection. Nothing opens a remote folder yet; that is Phase 4. Until then no existing command can be pointed at a remote path, and none may treat one as a local path.

## Decisions for this phase

- **The variant arrives now, refused everywhere but the trait.** `WorkspaceEnv::Ssh { host }` deserializes from `{ kind: "ssh", host }`. `backend_for` gives it the host's `RemoteWorkspace`; every code path that maps an env to host paths or host processes (`resolve_path`, `Location::of_host`, `build_command`, `HostWorkspace`, the PTY, LSP, the watcher, checkpoints) refuses it with an explicit error. `is_local()` is already false for it, so the host-only git features refuse it without change. A guard test fails if `HostWorkspace` can be built for an `Ssh` env.
- **One `ssh` process per host, its stdio the whole session.** `ssh -T -o ServerAliveInterval=15 -o ServerAliveCountMax=3 -o ConnectTimeout=20 <host> <bootstrap>`, using the first `ssh` on `PATH` (on Windows normally `C:\Windows\System32\OpenSSH\ssh.exe`, 9.5 here; Git's is 9.9), whose version goes in the log. It must be OpenSSH 8.4 or newer ( for `SSH_ASKPASS_REQUIRE=force`; `ssh -V` is checked and an older client gets a message asking for key authentication). The host string is validated first (no leading `-`, no whitespace or control bytes, at most 255 bytes), so it can never be read as an option. Never `StrictHostKeyChecking=no`.
- **Bootstrap, install and exec in the same session**, so a password is asked once. A short POSIX `sh` script reports `uname -sm`, the SHA-256 of `~/.terax-server/<version>/terax-agent` if present, and whether `sha256sum` exists; it then reads one line from stdin: `run`, or `install <bytes>` followed by exactly that many bytes (read with `dd`/`head -c`), which it writes to a temporary file in the `0700` directory, verifies, `chmod 700`s and `mv`s into place. Then it `exec`s the agent, and the same pipes carry the protocol. Agent directories of other versions untouched for 30 days are removed. The decision (run, install, unsupported OS or arch) is a pure function of the report.
- **The agent ships as a resource.** `scripts/build-agent.mjs` builds both musl agents with `cargo zigbuild` into `src-tauri/binaries/terax-agent-<platform>` (`x86_64-linux-musl`, `aarch64-linux-musl`) (gitignored, as the CLI is); `tauri.conf.json` bundles them as resources. The app hashes the bundled file at runtime (`sha2`), so there is no hash to keep in sync. In a debug build it falls back to `target/<cargo triple>/release/terax-agent`.
- **Askpass through `terax-cli` and the control server.** The app starts `ssh` with `SSH_ASKPASS=<bundled terax-cli>`, `SSH_ASKPASS_REQUIRE=force`, `TERAX_SSH_ASKPASS=1`, the control endpoint (`TERAX_CONTROL_ADDR`/`TOKEN`) and a connection id. With `TERAX_SSH_ASKPASS=1`, `terax-cli` treats its argument as OpenSSH's prompt, sends a new control method, `askpass`, and prints the answer. The app shows a dialog naming the host; a new host key shows OpenSSH's text verbatim, and a changed key is a warning that cannot be accepted from the dialog (OpenSSH refuses it anyway). Answers are never stored or logged; the method waits up to 300 s for the user, as `hook` waits for a decision.
- **Connection state is a pure machine.** `Idle → Connecting → Installing → Handshaking → Ready → Reconnecting → Ready | Failed`, with backoff (1, 2, 5, 10, 30 s, then every 30 s until the user stops it) and a heartbeat (`ping` every 15 s, three misses drop the connection). The shell around it owns the `ssh` child and the `RemoteWorkspace`; a drop fails in-flight calls with `ConnectionAborted` (Phase 2) and starts a reconnect. Reconnects never prompt on their own: a prompt during an automatic reconnect needs the user to click Reconnect.
- **Connections live in one app-wide registry** (`modules/remote/`), keyed by host, because `backend_for` is called from everywhere without app state. A space on a disconnected host gets `NotConnected` errors, never a silent local fallback.
- **Roots.** A connection starts with no roots (the agent refuses every path); `RemoteWorkspace` gains `set_roots`, which Phase 4 calls as remote folders are authorized.
- **Minimal UI.** A command palette entry "Connect to SSH Host…" (hosts from `~/.ssh/config` without wildcards, recent hosts, free text), the askpass dialog, and a status bar item per connected host (state; Reconnect, Disconnect, Show log). Visual polish and the space switcher entry are Phase 6.
- **The log** keeps the last 500 lines of `ssh`'s stderr and the bootstrap report per host, with prompt answers never passing through it.

## Tasks

### Task 1: `WorkspaceEnv::Ssh` and refusal everywhere else

- [x] Add the variant and host validation (pure, in the core): `ssh_host(&str) -> Result<SshHost, ...>`. Serde `{ kind: "ssh", host }`; the frontend type gains it.
- [x] Audit every match on `WorkspaceEnv` and every host-path or host-process entry point; each refuses `Ssh` with `Unsupported` naming the operation. `backend_for(Ssh)` asks the remote registry (Task 6; until then `NotConnected`).
- [x] Tests: deserialization and validation (leading `-`, spaces, control bytes, empty, too long); each refused entry point; a guard that `HostWorkspace::new` refuses an `Ssh` env.
  - `SshHost` (core `env.rs`) is a checked newtype, validated on deserialization too, so an invalid host cannot exist: 1 to 255 bytes, no leading `-`, no whitespace, control bytes, quotes, backticks or `$ ; | &`.
  - `resolve_path` was not changed to return a `Result` (about 37 callers, which Phase 4 moves to the trait anyway). Instead an `Ssh` path maps to `env::UNREACHABLE`, a string holding a NUL byte, which every std file-system call and `current_dir` refuses with `InvalidInput` on Windows and Unix; a test proves it for metadata, `read_dir`, `create_dir_all`, write, canonicalize and starting a process, joined or not. Explicit refusals (`env::not_on_this_machine()`, `Unsupported`) sit where code would otherwise reach this machine without a path: `build_command`, the PTY's `build_command`, the shell's `build_oneshot_command`, `authorize_spawn_cwd` and `authorize_user_spawn_cwd` (refused even with no cwd: they returned `Ok(None)`, and the caller then spawned with no cwd at all, on this machine), LSP, and the two `HostWorkspace` matches.
  - Found on the way: `build_command` ran any non-WSL env as a local process with the raw cwd; the PTY on Unix ignored the env entirely; a shell session with no cwd started in this machine's home. All now refuse `Ssh`.
  - `HostWorkspace::new` does not refuse (it returns `Self`); instead a test checks every operation of a `HostWorkspace` built for `Ssh` fails and leaves a real local directory of the same name untouched. `backend_for(Ssh)` returns `unavailable::Unavailable` (every operation `NotConnected`, naming the host) until Task 6.
  - Messages that said "not supported in WSL workspaces yet" for local-only git features now say "only supported in local workspaces". Git path display treats SSH paths as Linux paths.
  - Frontend: `{ kind: "ssh", host }`, scope key `ssh:<host>` (it fell through to `local`, which would have mixed a server's settings and recents with this machine's), `sameWorkspaceEnv`, `workspaceEnvLabel` ("SSH: host"), and no local home directory for a server (switcher, terminal links).
  - Mutation-checked: 8 mutants (the unreachable path, `build_command`, two host checks, the spawn gate, the PTY, the shell, `backend_for`) all fail a test.

### Task 2: Bootstrap and install planning (pure)

- [x] The bootstrap script text, the report parser, and `plan(report, bundled) -> Run | Install { platform } | Unsupported(reason)`. Old-version cleanup in the script.
- [x] Tests: report parsing (x86_64, aarch64, other arch, other OS, missing `sha256sum`, hash match and mismatch, garbage); on Unix, run the script under `sh` against a temp `HOME`, with `cat` standing in for the agent: install, reuse, a truncated upload refused, a corrupted upload refused and nothing left in place, a stale version removed.
  - `terax-remote-protocol::bootstrap`: `remote_command(version)` gives `sh -c '<script>'`, one line with no quote, backslash or newline inside, so any login shell (bash, zsh, fish, csh) hands it to `sh` unchanged; the version is checked (`[A-Za-z0-9._+-]`, at most 64, no leading `.`) because it is spliced into the script. `parse_report` and `plan` are pure; `install_line(len, sha256)`.
  - The exchange: the script prints `TERAX-BOOTSTRAP 1`, `os=`, `arch=`, `hasher=`, `sha256=`, `END`; reads one line (`run`, or `install <bytes> <sha256>`); for install it reads exactly that many bytes into a temp file in the version directory (`head -c`, falling back to `dd bs=1` where `head` has no `-c`), checks the hash, `chmod 700` and `mv` into place, or removes the temp file and prints `ERROR <reason>`; then prints `READY` and `exec`s the agent. The client writes nothing more until `READY`, so the script never reads protocol bytes. Any other instruction ends the session with nothing run.
  - The hash is checked on the server, so a short or altered upload never replaces the installed agent. `umask 077`, and `~/.terax-server` and the version directory are forced to 700 even when they already existed looser.
  - Cleanup: each connect touches its version directory; sibling directories with an mtime over 30 days are removed.
  - `plan` refuses non-Linux, architectures other than x86_64/amd64/aarch64/arm64, a server without `sha256sum`, and a build without the agent for that triple, each with a reason for the user.
  - Tests: 7 pure unit tests; `tests/bootstrap_sh.rs` (Unix only) runs the real command under every login shell present, with a stand-in agent script: install then reuse, modes, a truncated and an altered upload (old agent kept, no temp left), a bad size and an unknown instruction, the `dd` fallback, stale-version cleanup. Run on dypod (Ubuntu 24.04; `sh`, `bash`, `dash`): 6 passed. Mutation-checked on dypod: 7 mutants of the script (hash check, temp removal, chmod, cleanup, size check, unknown instruction, `dd` fallback) all fail a test.

### Task 3: Bundling the agent

- [x] `scripts/build-agent.mjs` (`pnpm build:agent`), run by `build:nosign` before `tauri build`; `binaries/terax-agent-*` gitignored; `tauri.conf.json` resources; runtime lookup with the debug fallback; SHA-256 of the bundled file.
- [x] Tests: lookup order; the hash of a known file. Manual: an installer built here contains both agents.
  - Names: the owner asked for `x86_64-linux-musl` and `aarch64-linux-musl` rather than Rust's `x86_64-unknown-linux-musl`. Those short platform names are used everywhere of ours (file names, resources, `bootstrap::plan`, `remote_agent`); only `cargo --target` and cargo's output directory use the full triple, through `cargo_target()` in Rust and `cargoTarget()` in the script.
  - `scripts/build-agent.mjs` (`pnpm build:agent`, `TERAX_AGENT_PLATFORMS` for a subset) checks for `cargo-zigbuild` (its own `--version`: `cargo zigbuild --version` starts a build), the rustup targets and zig (on `PATH`, or the `ziglang` Python package's), builds with `--release --locked`, checks the ELF magic and copies the result. About 2.5 min for both here.
  - The resources are in `tauri.nosign.conf.json`, not `tauri.conf.json`: tauri-build refuses a resource that does not exist, and a plain `cargo build`/`cargo test` (and CI) must not need the agents. `build:nosign` runs `build:agent` first. Checked with `TAURI_CONFIG` set to that file: both agents land in `agents/`, and a missing one stops the build with "resource path ... doesn't exist". `release-dy.yml` also passes this config, so the macOS and Linux release jobs must run `pnpm build:agent` (with zig and cargo-zigbuild) before 0.9.5-dy; that is part of the deferred CI work.
  - App side: `modules/remote_agent.rs`. Lookup order: `<resource dir>/agents/terax-agent-<platform>`, then in a debug build only `src-tauri/binaries/` and `src-tauri/target/<cargo triple>/release/`. `AgentBundle` hashes on demand and rehashes when size or mtime changes. `sha2` is a direct dependency; `terax-remote-protocol` is a dev-dependency until Task 4 uses it.
  - Tests: 5 (order, empty file and directory refused, SHA-256 test vectors, rehash, platform names agree with `bootstrap::plan`); 3 mutants killed. The prepared x86_64 agent ran on dypod (`terax-agent 0.9.4-dy`) and its `sha256sum` there equals the app's hash. An installer has not been built yet (about 30 min); Task 8 does that.

### Task 4: Transport

- [ ] Spawn `ssh` with the validated host, options and askpass environment; collect stderr into the log; run the bootstrap exchange over stdin/stdout (send `run` or `install` plus the bytes), then hand the same pipes to `RemoteWorkspace::connect`. The transport program is a parameter, so tests can use `sh -c` in place of `ssh`.
- [ ] `ssh -V` parsing (pure): 8.4+ required.
- [ ] Tests (Unix, `sh` as the transport, the real agent binary): a first connect installs and handshakes; a second reuses; a bootstrap that dies mid-install fails cleanly; stderr lines reach the log.

### Task 5: Askpass

- [ ] `terax-cli`: with `TERAX_SSH_ASKPASS=1`, send `askpass { connection, prompt }` and print the answer (exit 1 when cancelled, which OpenSSH reads as "no answer").
- [ ] Control server: the `askpass` method, routed to the frontend like `hook`, with a 300 s wait; classify the prompt (secret, yes/no, new host key, changed host key) in a pure function.
- [ ] Frontend: the dialog (host, prompt text, a masked field or yes/no), cancel on Escape.
- [ ] Tests: CLI argument and env handling; the classifier on real OpenSSH prompt texts; the control method's routing and timeout; the answer never appears in a log line.

### Task 6: Connections and state

- [ ] The pure state machine with backoff and heartbeat; the registry (`connect`, `disconnect`, `get`, `state`, `log`); `RemoteWorkspace::set_roots`; Tauri commands `remote_connect`, `remote_disconnect`, `remote_state`, `remote_log`; a `remote://state` event.
- [ ] Tests: every transition, including a drop during `Installing` and a disconnect during `Reconnecting`; backoff timing with a fake clock; a heartbeat miss drops the connection; `backend_for` after a disconnect returns `NotConnected`.

### Task 7: Connect UI

- [ ] `~/.ssh/config` host parsing (pure: `Host` lines, wildcards and negations skipped, `Include` followed one level); the palette entry and its host list with recents; the status bar item and its menu; Show log.
- [ ] Tests: the config parser; the components' states (connecting, installing, ready, failed with the reason).

### Task 8: End to end, guards and docs

- [ ] With the owner's approval, from a dev build to `dypod`: a first install, a reconnect that reuses, a version change that installs beside the old one, a killed `ssh` that reconnects, a new host key prompt (with a throwaway `UserKnownHostsFile`), and a password prompt if a password host is available.
- [ ] Guards: no `std::fs`/`Command::new` on an `Ssh` path (the Phase 1 guard covers new modules); `modules/remote` keeps answers out of logs.
- [ ] TERAX.md (connection model, askpass), a learning for anything non-obvious, and this plan marked complete.

## Risks

- **Prompt classification** relies on OpenSSH's wording; unknown prompts fall back to a plain secret or yes/no field with the text shown verbatim.
- **Reading an exact byte count in `sh`** differs between coreutils and busybox; Task 2 tests both `head -c` and a `dd` fallback.
- ~~**Windows OpenSSH and stdin.**~~ Checked while planning: piping the agent plus every byte value (including `
`, `0x1a` and NUL) through both `C:\Windows\System32\OpenSSH\ssh.exe` (9.5p2) and Git's `ssh.exe` (9.9p1) to `sha256sum` on `dypod` gives the same hash, so the upload can share the session.
