# Plan: Remote SSH, Phase 6 — AI tools, UX polish, packaging

Spec: `docs/superpowers/specs/2026-11-22-remote-ssh-workspace-design.md`, phase 6. Branch `dydev`; each task is verified (cargo test, clippy, vitest, tsc, lint at baseline), committed and pushed on its own. No release, no version bump, no CI in this phase: the 0.9.5-dy bump comes after the owner's walkthrough.

What is left after Phase 5 is what the guard (`ssh_commands_guard_tests.rs`) still lists as `PHASE6`, plus the rough edges a user meets in a server's space.

### Task 1: AI file tools on a server

- [x] `ai_glob` and `ai_grep` on an SSH workspace authorize the root with `authorized_in` (the server's spelling) and run the workspace's own `glob`/`grep` (the agent walks, `.gitignore` honoured), then drop every hit the secret-path policy refuses (`confined::refusal`), so a `.env` or `server.key` on the server never reaches the model. Local and WSL keep their walk.
- [x] `ai_read_file` and `ai_read_dir` already go through `authorized_in` and the backend: tests against the in-process agent prove it, including a refused secret file.
- [x] Guard: the four commands are `Works(<test>)`.

Done. `fs::ai::run_glob`/`run_grep`/`run_read_dir` take the registry; on a server, `run_read_dir` canonicalizes only links (a plain name in a canonical folder is canonical; every canonicalize is a round trip). Test: `remote::search_tests::the_ai_tools_search_and_read_on_the_server_without_its_secrets`.

### Task 2: AI shell tools on a server

- [x] `shell_run_command` on a server runs `sh -c <command>` through `backend_for(ws).exec` in the authorized cwd (`covers`), with the same timeout and output caps, so the approval flow and the result shape stay as they are.
- [x] `shell_session_open`/`run` keep the session's cwd in the server's spelling: the POSIX sentinel wrapper, the cwd checked with the workspace's `metadata` instead of this machine's `is_dir`.
- [x] `shell_bg_spawn` on a server is refused with a clear message (a background process needs streamed output, which the protocol does not carry yet); the guard lists it as `Refused`.
- [x] Tests against the in-process agent; guard entries updated.

Done. `shell::run_command_in` and `authorize_server_cwd` (a server has no default folder, so a cwd is required). `sh` comes from the server's `PATH`, so the in-process tests on Windows run MSYS's `sh`, whose `pwd` spells `/c/...`: the session's `cd` is checked on Unix and on dypod. The guard has no pending commands left, so its `Pending` variant is gone. Tests: `remote::shell_tests`.

### Task 3: UX polish

- [x] A terminal opened while its server is still connecting waits for `ready` instead of failing, and a disconnected terminal's Retry is offered as soon as the server is back.
- [x] Saving while the server is not connected says so ("<host> is not connected") instead of a raw error, and the buffer stays dirty.
- [x] The space switcher marks a server's space with its host.

Done. `whenServerReady(host)` (`remote/connections.ts`) waits without starting an attempt; `openPty` awaits it for an SSH workspace, so a restored tab or Retry after a drop starts once the server is back, and an idle or failed host fails with its reason. A failed save (any workspace; it used to reject unseen) toasts once per file, `saveFailure` naming a disconnected host, and returns false with the buffer dirty. `SpaceAvatar` carries a server badge for an SSH space (its name already ends in `(host)`).

### Task 4: Packaging

- [ ] The bundled agents (`binaries/terax-agent-*-linux-musl`) are resources of the unsigned build config; a missing agent says how to build it. The release workflow's `build:agent` step is noted for the CI pass later.

### Task 5: On a real server, docs

- [ ] dypod e2e: `ai_glob`, `ai_grep` (a secret file left out), `ai_read_file`, `shell_run_command`, a shell session's `cd`.
- [ ] TERAX.md, the guard has no `PHASE6`, plan marked complete.
