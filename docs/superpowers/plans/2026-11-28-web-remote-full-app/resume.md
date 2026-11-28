# Resume: finish the full-app spike (Phase 0)

You are continuing work another agent started. Read these first, in full, and follow them:
1. `docs/superpowers/plans/2026-11-28-web-remote-full-app/agent-prompt.md`: rules, security, test host and reporting. Its **Hard rules** changed: read them again even if you saw an older copy.
2. `docs/superpowers/plans/2026-11-28-web-remote-full-app/plan.md`: the plan. Do only **Task 0 (the spike)**.

Everything is on **`dydev`**. The user asked for one branch: there is no spike branch any more, so do not create one.

## What went wrong last time
- The previous run built and started a dev build of Terax on Windows, and the user's own running Terax ended.
- From now on:
  - Never stop, kill or restart any `terax.exe` on this machine.
  - Never run `pnpm dev:app` or any dev build here without asking the user first.
  - On Windows, only use checks that need no GUI: `cargo check`, `cargo test`, vitest.
  - Run everything that needs a running app on **aspirexc**.

## Where the spike stands
The spike is committed on `dydev`, and its code builds.
- `src-tauri/src/modules/web_remote/spike.rs` holds the five checks, run by `GET /spike/check`:
  1. window `remote-0`;
  2. invoke through `Webview::on_message`, plus an ACL denial;
  3. `plugin:event|listen` callback relay;
  4. Channel, with a small payload and a 12 KB one;
  5. `pty::protocol::dispatch_for_label`.
- **`/spike/check` has no auth, so it answers only when the process runs with `TERAX_REMOTE_SPIKE=1`** (`spike::enabled()`). Without that variable it is a 404 like any unknown path. Never set it on the `terax-remote` service.
- `remote-relay.html` overrides `__TAURI_INTERNALS__.callbacks.get` and forwards each callback through `spike_relay_push`. It is in the Vite inputs.
- `remote-*` has been added to `capabilities/default.json`, `desktop.json` and `clipboard.json`.
- Tests: `cargo test --lib -- web_remote pty::protocol` gives 44 passed.
- Not done yet: the checks have **not** been run, memory has not been measured, and the plan's "Spike result" section is empty.

## Next steps
1. On aspirexc, `cd ~/terax-ai && git pull` (it is on `dydev`), then build with `/tmp/terax-build.sh` (see the agent prompt).
   - Do **not** install over `~/.local/bin/terax`, and do not restart `terax-remote`.
   - Run the build next to the service, on another port:
     `TERAX_REMOTE_SPIKE=1 xvfb-run -a src-tauri/target/release/terax --remote-serve --bind loopback --port 7690`
   - Do not touch Orca on port 6768.
   - Over SSH, run `curl -s http://127.0.0.1:7690/spike/check` and keep the JSON.
   - If two copies of the app on the same host clash (single instance, shared data directory), stop and report.
   - Stop that second copy when you are done.
2. Measure the relay webview's memory on aspirexc: the RSS of its `WebKitWebProcess` with the relay window open against without it. Skip Windows unless the user gives the go-ahead.
3. Write a "Spike result" section into `plan.md`, then commit and push it to `dydev`. Record:
   - each check passed or failed, with the real output;
   - how Tauri 2.12.2 runs callbacks inside the webview (`callbacks.get` overridden, the channel fetch path for large payloads);
   - the memory figures;
   - the refactor `stream_protocol` needs.
4. **Stop and report to the user in Indonesian:**
   - which checks passed;
   - the memory per relay against the 150 MiB target;
   - what was not verified.

   Do not start Phase 1 until the user says go. If dispatch or the callback relay fails, report it and do not improvise another architecture.

## Watch out
- An `AppHandle` stored in a struct that the tests build (such as `server::Deps`) links the webview runtime into the test binary. On Windows that binary then fails to start with `STATUS_ENTRYPOINT_NOT_FOUND`. Reach the app through a trait object instead, like `TabEvents` or `SpikeRunner`.
- The spike code (`spike.rs`, the `/spike/check` route, `spike_relay_push` and `spike_test_channel`) is removed when Task 3 replaces it with real relay sessions.
