# Plan: Remote SSH, Phase 3 — transport and connection

**Status:** complete (2026-11-23), except the GUI walkthrough left to the owner (Task 8).

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

- [x] Spawn `ssh` with the validated host, options and askpass environment; collect stderr into the log; run the bootstrap exchange over stdin/stdout (send `run` or `install` plus the bytes), then hand the same pipes to `RemoteWorkspace::connect`. The transport program is a parameter, so tests can use `sh -c` in place of `ssh`.
- [x] `ssh -V` parsing (pure): 8.4+ required.
- [x] Tests (Unix, `sh` as the transport, the real agent binary): a first connect installs and handshakes; a second reuses; a bootstrap that dies mid-install fails cleanly; stderr lines reach the log.
  - Placed in `terax-remote-protocol::transport` (std only), not the app: its tests need the real agent binary, which only the agent crate's tests can name (`crates/terax-agent/tests/transport.rs`). The app adds askpass (Task 5) and the registry (Task 6) around it. `Transport { program, args, env }` takes the bootstrap command as its last argument; `Transport::ssh` gives `-T`, the keep-alives, `ConnectTimeout=20` and `--` before the host. On Windows the process gets `CREATE_NO_WINDOW`.
  - `connect(transport, env, bundled, log, on_stage, options)` returns `Connected { workspace, process, report, installed }` or a `ConnectError`: `Spawn`, `Ended { stderr }` (ssh's last stderr line, such as "Permission denied (publickey)."), `Bootstrap`, `Unsupported`, `TimedOut` (a watchdog kills the process after `bootstrap_timeout`, 600 s by default, which covers a 300 s prompt), `Handshake`. Stages: `Connecting`, `Installing(bytes)`, `Handshaking`. The workspace starts with no roots.
  - Lines the server's shell prints before `TERAX-BOOTSTRAP 1` (startup files that echo) are skipped and logged as `server: ...`, up to 100. stderr goes to the `Log` as `stderr: ...` (500 lines kept). On `Ended`, the client waits up to 2 s for stderr to close before stopping the process: a mutant that skipped the wait survived the first tests, and showed the stop came first and could cut ssh's last message; a test now prints it after stdout has closed.
  - The bootstrap now also removes upload temp files older than a day, left by a session cut off mid-upload (a recent one may be another upload in progress).
  - `check_openssh_version` needs 8.4+, with a message pointing to key authentication for older clients; parses `OpenSSH_for_Windows_9.5p2` and `OpenSSH_9.9p1`.
  - Tests: 4 units; 9 transport tests on Unix (install then reuse, noise and stderr, killed mid-upload leaves no agent, a refused login, late last words, no bundled agent, wrong output, timeout, missing program), run on dypod; `bootstrap_sh` (now 7) on dypod. Mutation: 8 mutants killed.
  - Over real SSH from Windows (`over_ssh.rs`, ignored, throwaway version removed before and after): Git's OpenSSH 9.9 installs in 3.0 s and reconnects in 1.3 s; Windows' OpenSSH 9.5 in 2.8 s and 1.0 s; ping about 50 ms; the installed agent's hash matches and its mode is 700.

### Task 5: Askpass

- [x] `terax-cli`: with `TERAX_SSH_ASKPASS=1`, send `askpass { connection, prompt }` and print the answer (exit 1 when cancelled, which OpenSSH reads as "no answer").
- [x] Control server: the `askpass` method, routed to the frontend like `hook`, with a 300 s wait; classify the prompt (secret, yes/no, new host key, changed host key) in a pure function.
- [x] Frontend: the dialog (host, prompt text, a masked field or yes/no), cancel on Escape.
- [x] Tests: CLI argument and env handling; the classifier on real OpenSSH prompt texts; the control method's routing and timeout; the answer never appears in a log line.
  - Credentials: `ControlState::askpass_grant(host)` makes a token for one `ssh` process and gives its environment (`SSH_ASKPASS=<bundled terax-cli>`, `SSH_ASKPASS_REQUIRE=force`, `TERAX_SSH_ASKPASS=1`, `TERAX_CONTROL_ADDR`, `TERAX_CONTROL_TOKEN`). The `askpass` method accepts only such a token (not the app's, not a terminal's), and such a token is good for nothing else; the host comes from the grant, never from the caller. Dropping the grant revokes the token and ends any question still open. `askpass` is not in `METHODS`, the catalog terminals may call. `None` without a bundled CLI (Task 6 decides what then).
  - The CLI reads only the endpoint in its environment (never the descriptor file), sends the prompt and `SSH_ASKPASS_PROMPT`, prints the answer to stdout and nothing else; `confirm` answers by exit status, `none` (a notice) succeeds at once; a dismissed dialog or any failure exits 1 with nothing on stdout.
  - `classify` (control protocol, pure): `secret`, `yes_no { host_key }`, `changed_host_key` (shown with a warning and only a Close button), `confirm`, `notice`, from OpenSSH 9.x texts. `AskpassAnswer`'s `Debug` redacts the answer; the server and bridge log no params or results.
  - `forward_to_frontend`'s wait became `await_frontend(state, request, timeout, emit)`, so routing and timeouts are tested without Tauri; askpass waits `ASKPASS_WAIT` (300 s).
  - Frontend: `modules/remote` (a queue store, `AskpassDialog`: OpenSSH's text verbatim in monospace, a masked field, Escape cancels; the body is keyed by question so nothing typed outlives it). Notices become a toast.
  - Tests: protocol 6, CLI 6, server 7, frontend 3. Mutation: 9 mutants killed; the token check first survived because the test tried wrong tokens before any grant existed.
  - Real OpenSSH (`crates/terax-cli/tests/askpass_over_ssh.rs`, ignored): Windows' OpenSSH 9.5 ran `terax-cli` as askpass with no console, and dypod's new host key reached the endpoint verbatim, classified `yes_no { host_key: true }`; "yes" was accepted. Then the passphrase of a throwaway key arrived as `secret` ("Enter passphrase for key '...\throwaway': ") and the login succeeded. A throwaway key's passphrase is asked only once the server accepts the key, so the test authorizes it for its run (restricted to `true`, removed on drop; checked gone afterwards). Earlier runs that answered wrong got this machine banned by dypod's fail2ban for a while; the test now never fails a login.

### Task 6: Connections and state

- [x] The pure state machine with backoff and heartbeat; the registry (`connect`, `disconnect`, `get`, `state`, `log`); `RemoteWorkspace::set_roots`; Tauri commands `remote_connect`, `remote_disconnect`, `remote_state`, `remote_log`; a `remote://state` event.
- [x] Tests: every transition, including a drop during `Installing` and a disconnect during `Reconnecting`; backoff timing with a fake clock; a heartbeat miss drops the connection; `backend_for` after a disconnect returns `NotConnected`.

Done. `modules/remote/`: `machine.rs` (pure; time comes in with events), `registry.rs` (a driver thread per host owns the machine, the live link, the heartbeat), `connector.rs` (`SshConnector`), `commands.rs` (the four commands, `remote://state`, installed in `setup`). Decisions:
- A first connect that fails stays `failed` with the reason; only a drop after `ready` reconnects on its own (1, 2, 5, 10, 30 s, then every 30 s).
- Automatic reconnects never prompt: no askpass grant, `BatchMode=yes`. Only an attempt the user starts gets a grant, dropped as soon as the attempt ends, which ends any open dialog.
- A refused login, a host key failure, an unsupported server or a missing ssh is permanent: retrying stops. Retried every 30 s, a refused login would get the client banned (dypod's fail2ban did).
- A drop is seen as the stream ending (`RemoteWorkspace::is_closed`, polled every second) or three missed 15 s heartbeats.
- The roots are kept per host and put back on every connection before it is announced `ready`; `set_roots` while down keeps them for the next one.
- Tests: 11 machine tests on a fake clock, 2 on the failure classes, 5 with the real agent in-process and a switch that cuts the stream (`backend_for` is `NotConnected` after a disconnect).

### Task 7: Connect UI

- [x] `~/.ssh/config` host parsing (pure: `Host` lines, wildcards and negations skipped, `Include` followed one level); the palette entry and its host list with recents; the status bar item and its menu; Show log.
- [x] Tests: the config parser; the components' states (connecting, installing, ready, failed with the reason).

Done.
- Rust `modules/remote/ssh_config.rs`: literal `Host` names only (patterns and `!` skipped, names the transport would refuse left out), `Include` followed one level (`~/`, relative to `~/.ssh`, `*`/`?` in the file name, sorted). Command `remote_ssh_hosts`.
- Frontend `modules/remote`: `connections.ts` (the `remote://state` parser, a store per host, recents in `localStorage`, newest first, 8 at most; the picker's rows: recents, then the config's, then a typed `user@host` checked by the same rule as the app), `ConnectSshDialog` (palette entry "Connect to SSH Host..."), `RemoteStatusItems` (one item per host that is not idle: a dot, the host, the state; menu Reconnect or Connect now, Disconnect, Show log), `RemoteLogDialog`, and `RemoteConnections` (listens, loads the states a new window missed, hosts both dialogs; mounted in `App`).
- Tests: Rust 6 for the parser; frontend 9 (events, the host rule, the picker's order, recents, each state's face including the failure reason, the log).
- Not yet seen in a running app: that is Task 8.

### Task 8: End to end, guards and docs

- [x] With the owner's approval, from a dev build to `dypod`: a first install, a reconnect that reuses, a version change that installs beside the old one, a killed `ssh` that reconnects, a new host key prompt (with a throwaway `UserKnownHostsFile`), and a password prompt if a password host is available.
- [x] Guards: no `std::fs`/`Command::new` on an `Ssh` path (the Phase 1 guard covers new modules); `modules/remote` keeps answers out of logs.
- [x] TERAX.md (connection model, askpass), a learning for anything non-obvious, and this plan marked complete.

Done, with one part left to the owner.
- `src/modules/remote/e2e_tests.rs` (ignored; `TERAX_AGENT_SSH_HOST=dypod`) drives the app's own `Registry` and `SshConnector` with Windows' OpenSSH 9.5 and `terax-cli` as askpass (a stand-in control endpoint answers). On dypod: the new host key asked through `terax-cli` (a throwaway `UserKnownHostsFile`) and accepted; a first install, 2.1 to 2.6 s; disconnect and connect again reusing the agent, 0.7 to 1.0 s, no new question; the agent killed on the server, then an automatic reconnect without prompting, 1.6 s, with the roots put back; another version installed beside the first. The run removes its `~/.terax-server/0.0.0-e2e-*` directories.
- That run found a bug: a drop was explained by the last stderr line in the log, which was an earlier session's "Permanently added ..." warning. `registry::drop_reason` now reads only what ssh said since the session connected.
- No password host was available (dypod takes keys only); a key's passphrase through `terax-cli` was checked in Task 5.
- Not done: clicking through the GUI. A dev build shares the installed Terax's control descriptor (`%LOCALAPPDATA%\terax\control.json`) and would break the owner's running copy, so the palette, status bar and dialogs are left for the owner to try (see the learning).
- Guards: `workspace_guard_tests.rs` now scans `remote/` (only `ssh_config.rs` reads this machine's files, the user's ssh config); `remote/answers_guard_tests.rs` fails if a log call, a connection log line or `println!` on the askpass path (`control.rs`, `terax-cli`'s and the protocol's `askpass.rs`, `modules/remote`) formats an answer, a response or a result. Checked by planting `log::debug!("{:?}", request.params)` in `askpass_request`: the guard failed.
- TERAX.md: an "SSH connections" section.

## Risks

- **Prompt classification** relies on OpenSSH's wording; unknown prompts fall back to a plain secret or yes/no field with the text shown verbatim.
- **Reading an exact byte count in `sh`** differs between coreutils and busybox; Task 2 tests both `head -c` and a `dd` fallback.
- ~~**Windows OpenSSH and stdin.**~~ Checked while planning: piping the agent plus every byte value (including `
`, `0x1a` and NUL) through both `C:\Windows\System32\OpenSSH\ssh.exe` (9.5p2) and Git's `ssh.exe` (9.9p1) to `sha256sum` on `dypod` gives the same hash, so the upload can share the session.
