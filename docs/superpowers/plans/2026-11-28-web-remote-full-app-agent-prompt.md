# Task: run the full Terax app in a browser over the web remote

You are working in the Terax repo: a Tauri 2 desktop terminal with a Rust backend in `src-tauri/`, a React/TS frontend in `src/`, and the Ghostty WASM renderer in `packages/ghostty-core`.
- Working copy: `D:\Github\agent-workspace\terax-ai`.
- Fork: `mahdyarief/terax-ai`.
- Branch: **`dydev`**.

## What to build
Follow **`docs/superpowers/plans/2026-11-28-web-remote-full-app.md`** exactly. Read all of it before starting, especially "Why this can reuse the whole app", "Decisions" and the feature matrix.

Also read the plan it builds on, `docs/superpowers/plans/2026-11-27-web-remote.md`, which is done. It covers the listener, pairing, per-device grants, the tab mirror, typing, tab actions, `--remote-serve` and the status bar switch.

Goal: open `/app/` in a browser and use the unchanged desktop app bundle, backed by Tauri's own command dispatcher through a hidden relay webview per session. The existing mirror at `/web-index.html` stays as the light, phone-first view.

## Phase 0 is a gate
Do **Task 0 (the spike) first, on a throwaway branch** (for example `dydev-spike-full-app`).

Then **stop and report to the user**:
- which of the five checks passed;
- how Tauri 2.12.2 runs callbacks inside a webview;
- the relay webview's memory on aspirexc (and on Windows if you can).

Write the findings into a "Spike result" section of the plan on `dydev`.

Continue to Phase 1 only after the user says go. If dispatch through `Webview::on_message` or the callback relay does not work, do not improvise a different architecture. Report it; the fallback (per-feature HTTP endpoints) needs a new plan.

After the go, work through Tasks 1 to 22 in order. If the code contradicts the plan, stop and say so; do not change a "Decision" on your own.

Ask before:
- adding a dependency the plan does not name;
- widening the command policy, making the `app` grant default on, or loosening the secret rule;
- weakening any existing security rule: bind default, pairing, grants, Host/Origin checks, `X-Terax-Remote`, CSP;
- widening Tauri capabilities beyond adding `remote-*` with the same permissions `main` has.

## Security rules that must hold
- `app.invoke_key()` stays in Rust. It never appears in a response, a log line, the injected runtime or the relay page's URL.
- Deny by default. A command with no class in `policy.rs` is refused, and the guard test fails on any command in `generate_handler!` without a class.
- `Native` commands are never forwarded.
- `Secret` commands need HTTPS (`tailscale serve`) or loopback, and `secrets_get_all` is never forwarded.
- Prove from the browser's network log that no API key reaches the browser before AI is enabled remotely (Task 18).
- The `app` grant defaults to false. Old device files load with `app: false`. Revoking a device, or removing `app`, closes its relay sessions.
- The audit log records command name, device and time, never arguments.
- The port proxy (Task 19) only reaches ports Terax itself reported, on `127.0.0.1`, and never an arbitrary host.

## How to work
- **One task = one commit.** Write the test first where the task adds behaviour (RED, then GREEN).
- After each finished task, **commit and push to `dydev` without asking**. The spike branch is the exception: push it only if the user asks.
- Commit messages end with:
  `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`
- Tick the task's checkboxes in the plan file in the same commit, and update the plan's **Status** line when a phase is done.
- **Never** merge to `main`, tag, create a release or open a PR; ask first. Do not trigger GitHub Actions and do not push to `main`.
- Run `trail check` before each commit.
- After a task that settles a non-obvious decision or rules an approach out, write a learning:
  - command: `trail note --title "<decision in plain words>"`;
  - text on stdin, with the sections `## Decided`, `## Tried and ruled out` and `## Watch out`, then an `In the code:` line;
  - commit it with the change.

## Hard rules
- Package manager: **pnpm only** (`pnpm exec …`). Never npm, npx or yarn.
- Never touch the installed `D:\Terax\terax.exe`. To run the app, use `pnpm dev:app`.
- Formatting:
  - Never run `cargo fmt` on the crate. `rustfmt --edition 2021 <file>` is fine on new files only.
  - Never run `biome format --write` on existing files. To check one, compare biome's `lint/…` and `assist/source/organizeImports` findings against HEAD (`git stash`, check, `git stash pop`) and fix only what you introduced. New files may be formatted.
  - Working copies are CRLF, so biome reports "format" on almost every file. That is noise.
- Never use built-in WebSearch or WebFetch (see CLAUDE.md). Use the MCP web-access tools if present; otherwise work from the repo and from the Tauri source in `~/.cargo/registry/src/*/tauri-2.12.2/`.
- Any Tauri command taking a `WorkspaceEnv` must be listed in `src-tauri/src/modules/ssh_commands_guard_tests.rs` with a remote test (pattern: `remote/live_server_tests.rs`).
- Do not remove or weaken anything that uses askpass or passphrases in the SSH or git code. Task 17 must route those prompts to the browser session that started the operation.
- The desktop build must not change behaviour. Remote-only code paths check `isRemoteRuntime()`, and the injected runtime is only served under `/app/`.

## Editing pitfalls seen in this repo
- If you edit files with Python:
  - read and write bytes;
  - keep the file's line endings (`nl = "\r\n" if "\r\n" in raw else "\n"`) and use `encoding="utf-8"`, because cp1252 corrupted characters before;
  - assert that each replaced pattern occurs exactly once.
- Do not write regexes or escapes (`\b`, `\n`, `\u0000`) through a bash heredoc into Python; they turned into real control bytes twice. Use the Edit tool for those lines.
- Timings:
  - `pnpm exec tsc --noEmit -p .` takes a few minutes;
  - the full `pnpm exec vitest run` takes about 10 minutes (last run: 364 files, 5076 tests);
  - `cargo test --lib` from `src-tauri/` takes about 7 minutes once built (last run: 1113 passed).
- **Never run the full cargo suite and the full vitest suite at the same time.** Under that load about 11 Rust tests and `src/app/dist-budget.test.ts` fail spuriously.
  - Known flakies that pass alone: `dist-budget`, `AdaptedGhosttyGpu`, and the git tests `the_upstream_is_the_ref_the_fetch_refspec_maps_not_origin_slash_branch` and `a_new_checkpoint_replaces_the_restore_as_the_baseline`.
- Render tests use `renderToStaticMarkup`, where zustand stores show their initial state, and `@testing-library/react` is not installed. Test presentational components with props and put logic in pure functions with unit tests (see `src/web/terminal/composer.ts`).
- The `fastmcp-browser` screenshot tool returned stale frames for the web client, and canvas content cannot be read from a snapshot. Verify through the API with `curl` (stream SSE, decode the base64 `data` events) and use the DOM digest for layout. Say what you could not see.
- Remote-access state reaches every window through the `terax:remote-access` event. Code that depends on it must follow that event, not read the status once.

## Code worth reading first
- Tauri internals (version 2.12.2, from `Cargo.lock`):
  - `~/.cargo/registry/src/*/tauri-2.12.2/src/webview/mod.rs`: `InvokeRequest` around line 135 and `on_message` at 2005, which has the ACL and `is_local` logic;
  - `src/app.rs:1147`: `invoke_key`;
  - the IPC/channel modules under `src/ipc/`;
  - the injected JS that defines `__TAURI_INTERNALS__` (`runCallback`, `transformCallback`, channel fetch).
- Web remote (done):
  - `src-tauri/src/modules/web_remote/` (`mod.rs`, `server.rs`, `http_rules.rs`, `devices.rs`, `stream.rs`, `tabs.rs`, `sse.rs`, `https.rs`);
  - `src/web/` (mirror client, `importGuard.test.ts`);
  - `src/modules/web-remote/` (`publishTabs.ts`, `RemoteAccessPill.tsx`, `lastOptions.ts`);
  - `src/settings/sections/RemoteSection.tsx`.
- Schemes:
  - `src-tauri/src/modules/pty/protocol.rs` (`stream_protocol`) and `pty/gate.rs` (per-label tokens);
  - `fs/assets.rs` and `fs/preview.rs`;
  - registrations at `lib.rs:293`.
- Window labels: `lib.rs` (`main`, `settings`, `space-*` helpers, the main-destroyed teardown around line 367), and `capabilities/default.json`, `desktop.json` and `clipboard.json`.
- Channel users: `src/modules/ai/lib/proxyFetch.ts` and `src/modules/lsp/lib/transport.ts`.
- Secrets: `src-tauri/src/modules/secrets.rs` and the `net` proxy commands.
- Handoff: `src/modules/terminal/lib/handoffSession.ts`, and `takeHandoffReplay` and `attach: { ptyId, replay }` in `src/modules/terminal/ghostty/useGhosttyTerminalSession.ts`.
- Settings window: `src/modules/settings/openSettingsWindow.ts` and `src/settings/SettingsApp.tsx`.
- Vite multi-page inputs: `vite.config.ts`, `rollupOptions.input`, around line 189.

## Test host
- **`aspirexc`:** Ubuntu 26.04 x86_64, headless with Xvfb, 12 cores, 14 GiB.
  - Tailscale IP `100.84.56.62`.
  - SSH alias **`dy-aspire`** (user `dy`, key auth, passwordless sudo).
  - Always use `ssh -o BatchMode=yes` and never try password logins. A separate host, `dypod`, has fail2ban, so never test failed SSH logins against it.
- Terax already runs there:
  - systemd user unit `terax-remote`;
  - command `xvfb-run -a … ~/.local/bin/terax --remote-serve --bind tailscale --port 7681`;
  - checkout `~/terax-ai` on `dydev`.
- Orca runs on port 6768. Do not touch it.
- Build there:
  - pull `dydev`, then run `/tmp/terax-build.sh`, which does `pnpm install --frozen-lockfile` and `pnpm exec tauri build --no-bundle`, logging to `~/tmp/terax-build.log`;
  - if `/tmp` was cleared, recreate the script with those two commands;
  - install with `systemctl --user stop terax-remote; install -m755 src-tauri/target/release/terax ~/.local/bin/terax; systemctl --user start terax-remote`.
  - `pnpm build:agent` fails there (`cargo-zigbuild` is not installed) and is not needed.
- Pairing on a headless host:
  - `--remote-serve` prints a pairing URL to stderr (`journalctl --user -u terax-remote | grep "pair a browser"`), valid 10 minutes and once;
  - grants are edited in `~/.local/share/app.crynta.terax/remote-devices.json` while the service is stopped;
  - the Lenovo laptop's Chrome is already paired with `input` and `tabs`; add `app` there when Task 2 lands.
- Restarting the service ends its terminals. Tell the user before you restart it while they may be using it.

## Reporting
Report to the user in **Indonesian**, concisely. After the spike and at the end of each phase, say:
- what was done;
- how it was verified, with real test output (say if a step was skipped or something failed);
- what was not tested manually.

Keep the user's email address private; never send it to an external service.
