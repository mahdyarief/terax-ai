# Resume: finish the full-app spike (Phase 0)

You are continuing work another agent started. Read these first, in full, and follow them:
1. `docs/superpowers/plans/2026-11-28-web-remote-full-app-agent-prompt.md`: rules, security, test host and reporting. Read the version on `dydev` (commit `e30b233` or later). Its **Hard rules** changed: read them again even if you saw an older copy.
2. `docs/superpowers/plans/2026-11-28-web-remote-full-app.md`: the plan. Do only **Task 0 (the spike)**.

## What went wrong last time
- The previous run built and started a dev build of Terax on Windows, and the user's own running Terax ended.
- From now on:
  - Never stop, kill or restart any `terax.exe` on this machine.
  - Never run `pnpm dev:app` or any dev build here without asking the user first.
  - On Windows, only use checks that need no GUI: `cargo check`, `cargo test`, vitest.
  - Run everything that needs a running app on **aspirexc**.

## Where the spike stands
All of it is uncommitted on branch **`dydev-spike-full-app`** in `D:\Github\agent-workspace\terax-ai`. Run `git status` and `git diff` to see it.
- `src-tauri/src/modules/web_remote/spike.rs`: the five checks, run by `GET /spike/check`:
  1. window `remote-0`;
  2. invoke through `Webview::on_message`, plus an ACL denial;
  3. `plugin:event|listen` callback relay;
  4. Channel, with a small payload and a 12 KB one;
  5. `pty::protocol::dispatch_for_label`.
- `remote-relay.html`: overrides `__TAURI_INTERNALS__.callbacks.get` and forwards each callback through `spike_relay_push`. The page is added to the Vite inputs.
- `remote-*` has been added to `capabilities/default.json`, `desktop.json` and `clipboard.json`.
- It compiled locally (the debug build from 08:26). The checks have **not** been run, memory has not been measured, and the plan's "Spike result" section is empty.

## Next steps
1. `cargo check` locally to confirm it still builds. Do not launch it.
2. Commit the spike on `dydev-spike-full-app`. **Do not push it** unless the user asks; it would show up as a branch on GitHub.
3. On aspirexc, build the spike branch in a **separate checkout**, for example `~/terax-spike`, so `~/terax-ai` stays on `dydev`.
   - Run it on another port, for example `xvfb-run -a ./terax --remote-serve --bind loopback --port 7690`, so the `terax-remote` service and its terminals keep running.
   - Do not touch Orca on port 6768.
   - Then call `curl -s http://127.0.0.1:7690/spike/check` over SSH and keep the JSON.
   - If two copies of the app on the same host clash (single instance, shared data directory), stop and report. Do not restart `terax-remote` without telling the user first.
4. Measure the relay webview's memory on aspirexc (the RSS of its `WebKitWebProcess` with the relay window open versus without it). Skip Windows unless the user gives the go-ahead.
5. Write a "Spike result" section into the plan on **`dydev`**. Use a worktree, so the spike's uncommitted state never mixes with `dydev`. It records:
   - each check passed or failed, with the real output;
   - how Tauri 2.12.2 runs callbacks inside the webview (`callbacks.get` overridden, the channel fetch path for large payloads);
   - the memory figures;
   - the refactor `stream_protocol` needs.

   Commit and push that to `dydev`.
6. **Stop and report to the user in Indonesian:**
   - which checks passed;
   - the memory per relay against the 150 MiB target;
   - what was not verified.

   Do not start Phase 1 until the user says go. If dispatch or the callback relay fails, report it and do not improvise another architecture.

`/spike/check` has no auth. It must never reach `dydev` or the `terax-remote` service binary.
