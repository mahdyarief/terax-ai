# Web Remote: the full Terax app in a browser

> **For agentic workers:** implement task by task. Each task is one commit on `dydev`, test first where it adds behaviour. Steps use checkbox (`- [ ]`) syntax for tracking. Phase 0 is a spike whose result decides whether Phases 1–5 go ahead as written.

**Status (2026-11-28):** Phase 0 (spike) verified on aspirexc; awaiting go-ahead for Phase 1. Builds on `../2026-11-27-web-remote/plan.md` (done: listener, pairing, grants, tab mirror, typing, tab actions, `--remote-serve`, status bar switch).

## Goal

From a browser, use everything the desktop app offers: spaces and workspaces (local, WSL, SSH), terminal and blocks, the file explorer, the editor with LSP, git and source control, history, AI chat and agents, previews, the command palette and Settings. The current mirror client stays as the light view for phones and for watching desktop tabs.

## Why this can reuse the whole app instead of porting it

The frontend reaches the backend only through `@tauri-apps/*`:

- 104 files import it;
- about 159 distinct `invoke` commands (168 registered in `lib.rs`);
- events: `fs:changed`, `fs:file-written`, `terax:open-file`, `terax:window-presentation`, `terax:settings-tab`, `terax:remote-access`, `terax:agent-signal`, `terax:control-request`, `remote:tab-action`;
- two `Channel` users: `ai/lib/proxyFetch.ts` and `lsp/lib/transport.ts`;
- three URI schemes: `terax-pty` (terminal stream), `asset` and `terax-doc` (previews), reached through `convertFileSrc` and `fetch`;
- plugins: store, opener, window/webviewWindow, path, os, app, notification, process, updater, autostart.

Every `@tauri-apps/api` call ends in `window.__TAURI_INTERNALS__`:
- `invoke`;
- `transformCallback`;
- `convertFileSrc`;
- `metadata` (the current window and webview label).

A browser page that provides its own `__TAURI_INTERNALS__` therefore runs the unchanged app bundle. This is the approach Orca's `web-index.html` and the wmux shim take.

On the Rust side, Tauri 2.12.2 (the version in `Cargo.lock`) lets the server run any command as if a webview had sent it:

- `tauri::webview::InvokeRequest` is a public struct: `cmd`, `callback`, `error`, `url`, `body`, `headers`, `invoke_key`.
- `Webview::on_message(request, responder)` is public (`tauri-2.12.2/src/webview/mod.rs:2005`). It applies the same ACL as a real IPC call, using the webview's label and the request URL's origin.
- `AppHandle::invoke_key()` is public (`src/app.rs:1147`). Its documentation warns never to expose the key to third-party scripts, so it stays in Rust and is never sent to the browser.

So the server does not re-implement 159 commands. It forwards an allowlisted subset to Tauri's own dispatcher and returns the response.

## Decisions

1. **One hidden relay webview per browser app session**, labelled `remote-<n>`.
   - It loads a small local page, `remote-relay.html`, and is the `Webview` that `on_message` dispatches against.
   - Commands, the pty gate (`pty/gate.rs` authorizes by label), the tab registry and space ownership then see a real window with its own label, so per-window logic needs no changes.
   - When Tauri delivers event callbacks or `Channel` messages to that webview, the relay page forwards them to Rust. Rust sends them to the browser over SSE, keyed by callback id. This covers events and channels generically, including events targeted at one window.
   - Sessions are capped (default 3) and closed after a 60 s disconnect grace or when the device is revoked.
2. **Deny-by-default command policy.** Every registered command is classified in `web_remote/policy.rs` as one of:
   - `read`;
   - `write`;
   - `control` (spawns processes, writes files, changes git);
   - `secret` (reads API keys);
   - `native` (meaningless in a browser: vibrancy, window presentation, updater, autostart, app menu).

   A guard test fails when a command in `generate_handler!` has no class, in the same way as `ssh_commands_guard_tests.rs`.
3. **A new grant, `app`**, next to `input` and `tabs`. It opens the full app, which can run anything the user can, so it is off by default and Settings marks it plainly.
   - `secret` commands are refused over the network unless the session is HTTPS (`tailscale serve`) or loopback. Even then, `secrets_get_all` is never forwarded. AI calls that need a key go through `net` proxy commands that keep the key in Rust; Task 13 checks that this holds.
4. **Same bundle, injected runtime.** `/app/` serves the normal `index.html` with one extra script, `/app/remote-runtime.js`, injected before the app's own scripts.
   - It is built from `src/web/runtime/`, defines `__TAURI_INTERNALS__` and sets `window.__TERAX_REMOTE__ = true`.
   - The desktop build does not change. Code that must behave differently checks `isRemoteRuntime()`.
5. **URI schemes become HTTP routes:** `/scheme/terax-pty/…`, `/scheme/asset/…` and `/scheme/terax-doc/…`.
   - They call the same Rust handlers with the session's relay label, so pty, asset and preview grants keep working.
   - `convertFileSrc` in the shim maps to these routes.
6. **The two clients stay separate.**
   - `web-index.html` stays the mirror: light, phone first, watches desktop tabs.
   - `/app/` is the full app as its own window. It hosts its own spaces, sees desktop tabs in a "Desktop" section built on the mirror (Phase 4), and can take a terminal over through the existing handoff (`takeHandoffReplay`, `attach: { ptyId, replay }`).
7. **Native-only features are hidden, not faked:**
   - vibrancy, window presentation, always-on-top;
   - the updater, autostart and the app menu;
   - opening a path in the host's file manager.

   Some get a browser equivalent:
   - `opener` URLs open in a new browser tab;
   - notifications use the Web Notifications API;
   - the clipboard uses `navigator.clipboard`, which needs HTTPS or localhost; otherwise a paste dialog.
8. **The settings window becomes an in-page route** (`/app/#settings/<tab>`), because the browser has no second Tauri window. `openSettingsWindow` gets a remote branch.

## Feature matrix

| Area | Desktop mechanism | In the browser | Phase |
|---|---|---|---|
| Terminal / blocks | `terax-pty` scheme, Ghostty WASM | `/scheme/terax-pty` route; renderer already runs in browsers (mirror proves it) | 2 |
| Tabs, spaces, workspaces (local/WSL/SSH) | invoke + per-window state | relay label = its own window | 2 |
| Explorer, file ops | `fs` commands, `fs:changed` event | invoke + relayed events | 3 |
| Editor, LSP | `fs` + `lsp` Channel transport | invoke + relayed Channel | 3 |
| Previews (markdown, docx, html, sheets, images) | `asset`, `terax-doc` schemes | `/scheme/*` routes | 3 |
| Git, source control, history | `git`, `history` commands | invoke | 3 |
| AI chat, agents launcher, agent signals | `net` proxy Channel, `secrets`, `terax:agent-signal` | relayed Channel; secret policy | 4 |
| Live server, dev-server URLs | host `localhost:<port>` | `/proxy/<port>/` reverse proxy on the remote server (allowlisted ports) | 4 |
| Command palette, shortcuts | DOM + invoke | works; browser-reserved keys remapped (Ctrl+T/W/N, Ctrl+Tab) | 2 |
| Settings | separate window | in-page route | 2 |
| Store-backed preferences | `plugin:store` | invoke (ACL for `remote-*`) | 1 |
| Window controls, vibrancy, updater, autostart, menu | native | hidden | 2 |
| Desktop tabs | — | "Desktop" section using the mirror; take over via handoff | 4 |

## Phase 0: spike (decides the rest)

### Task 0: prove dispatch, callbacks, channels and schemes
The spike lives on `dydev` (the user wants a single branch), behind `TERAX_REMOTE_SPIKE=1` because its route has no auth. Task 3 replaces it. The findings go into this plan's "Spike result" section.
- [x] Create a hidden window `remote-0` loading a minimal `remote-relay.html`. Add `remote-*` to `capabilities/default.json`, `desktop.json` and `clipboard.json`.
- [x] From an HTTP handler, run `pty_list` (or any read command) through `Webview::on_message` with `app.invoke_key()` and the webview's own URL. Confirm the JSON response reaches the HTTP response through `OwnedInvokeResponder`, and that ACL denial works for a command the capability lacks.
- [x] Register `plugin:event|listen` through the same path with a callback id. Confirm that `app.emit("fs:changed", …)` runs that callback id inside the relay page, and that the relay can forward it, by overriding `__TAURI_INTERNALS__.runCallback` or whatever 2.12.2 uses; record which.
- [x] Do the same for a `Channel` argument (the LSP transport's shape), including a payload large enough to take Tauri's fetch path for channel data.
- [x] Call `pty::stream_protocol`'s core from an HTTP route with the relay label. If it is tied to `UriSchemeContext`, note the refactor needed.
- [ ] Measure the relay webview's memory on aspirexc (WebKitGTK) and on Windows (WebView2). (aspirexc measured; Windows skipped per `resume.md`.)

### Spike result

Verified on `aspirexc` (Ubuntu 26.04 x86_64, WebKitGTK 4.1 under `xvfb-run`) with `TERAX_REMOTE_SPIKE=1 src-tauri/target/release/terax --remote-serve --bind loopback --port 7690` and `GET /spike/check`:

1. **Checks (5/5 passed, `"all_passed": true`):**
   - `check1_window`: `passed: true`, `url: "tauri://localhost/remote-relay.html"`.
   - `check2_invoke_and_acl`: `passed: true`. `pty_list_shells` returned `Ok((true, Array [...]))` (9 shells), and `plugin:window|set_always_on_top` returned `Ok((false, "Command plugin:window|set_always_on_top not allowed by ACL"))`.
   - `check3_event_relay`: `passed: true`. `plugin:event|listen` returned `Ok((true, Number(46)))`, and `app.emit("fs:changed", {"spike":"event-ok"})` delivered `{"event":"fs:changed","id":46,"payload":{"spike":"event-ok"}}` to `spike_relay_push`.
   - `check4_channel`: `passed: true`. `spike_test_channel` delivered both small (`small_hits: 2`, including `{ end: true }`) and 12 KB (`large_hits: 2`, `large_len: 12000`) payloads through the relay.
   - `check5_pty_scheme`: `passed: true`. `pty::protocol::dispatch_for_label` for `"remote-0"` with `POST /ack` (empty body) returned `Ok(Ok((400, ...)))` (`"request is shorter than its header"`), proving admission and label routing without `UriSchemeContext`.

   Raw `/spike/check` JSON:
   ```json
   {"all_passed":true,"check1_window":{"passed":true,"url":"tauri://localhost/remote-relay.html"},"check2_invoke_and_acl":{"acl_denied":"Ok((false, String(\"Command plugin:window|set_always_on_top not allowed by ACL\")))","passed":true,"pty_list_shells":"Ok((true, Array [Object {\"integrated\": Bool(true), \"name\": String(\"bash\"), \"path\": String(\"/bin/bash\")}, Object {\"integrated\": Bool(false), \"name\": String(\"sh\"), \"path\": String(\"/bin/sh\")}, Object {\"integrated\": Bool(false), \"name\": String(\"sh\"), \"path\": String(\"/usr/bin/sh\")}, Object {\"integrated\": Bool(true), \"name\": String(\"bash\"), \"path\": String(\"/usr/bin/bash\")}, Object {\"integrated\": Bool(false), \"name\": String(\"rbash\"), \"path\": String(\"/bin/rbash\")}, Object {\"integrated\": Bool(false), \"name\": String(\"rbash\"), \"path\": String(\"/usr/bin/rbash\")}, Object {\"integrated\": Bool(false), \"name\": String(\"dash\"), \"path\": String(\"/usr/bin/dash\")}, Object {\"integrated\": Bool(false), \"name\": String(\"screen\"), \"path\": String(\"/usr/bin/screen\")}, Object {\"integrated\": Bool(false), \"name\": String(\"tmux\"), \"path\": String(\"/usr/bin/tmux\")}]))"},"check3_event_relay":{"hits":[{"event":"fs:changed","id":46,"payload":{"spike":"event-ok"}}],"listen":"Ok((true, Number(46)))","passed":true},"check4_channel":{"large_hits":2,"large_len":12000,"passed":true,"small_hits":2},"check5_pty_scheme":{"passed":true,"result":"Ok(Ok((400, [114, 101, 113, 117, 101, 115, 116, 32, 105, 115, 32, 115, 104, 111, 114, 116, 101, 114, 32, 116, 104, 97, 110, 32, 105, 116, 115, 32, 104, 101, 97, 100, 101, 114])))"}}
   ```

2. **How Tauri 2.12.2 runs callbacks and large `Channel` payloads:**
   - In `tauri-2.12.2/scripts/core.js`, `window.__TAURI_INTERNALS__.runCallback` is defined with `Object.defineProperty(..., { value: runCallback })` (non-writable, non-configurable), so it cannot be overwritten directly.
   - `runCallback(id, data)` looks up `callbacks.get(id)` on the `callbacks` `Map` (`Object.defineProperty(..., { value: callbacks })`). Overriding `callbacks.get` on that `Map` instance (as `remote-relay.html` does) intercepts any callback id not registered locally in the relay page and forwards `(id, payload)` via `invoke("spike_relay_push", ...)`.
   - In `tauri-2.12.2/src/ipc/channel.rs`, small `Channel` messages (`< MAX_JSON_DIRECT_EXECUTE_THRESHOLD` / `MAX_RAW_DIRECT_EXECUTE_THRESHOLD`) are evaluated directly via `webview.eval("window.__TAURI_INTERNALS__.runCallback(id, { message: ..., index: ... })")`. Larger payloads (such as the 12 KB test) are queued in `ChannelDataIpcQueue`, and `webview.eval` runs `window.__TAURI_INTERNALS__.invoke('plugin:core|__FETCH_CHANNEL_DATA__', null, { headers: { 'Tauri-Channel-Data-Id': '<data_id>' } }).then((response) => window.__TAURI_INTERNALS__.runCallback(id, { message: response, index: ... }))`. The relay webview performs that fetch itself and then hits `callbacks.get(id)` with the resolved payload.

3. **Memory measurement (aspirexc, WebKitGTK 4.1):**
   - Before `/spike/check` (no `remote-0`): `terax` RSS 188,388 KiB (184.0 MiB); main `WebKitWebProcess` RSS 457,916 KiB (447.2 MiB).
   - After `/spike/check` (`remote-0` open): `terax` RSS 187,776 KiB (183.4 MiB); main `WebKitWebProcess` RSS 463,552 KiB (452.7 MiB); new `remote-0` `WebKitWebProcess` RSS **191,484 KiB (187.0 MiB)**.
   - Note: 187.0 MiB RSS on WebKitGTK is above the 150 MiB exit-criteria target (RSS includes shared library mappings; PSS was not measured).
   - Windows (WebView2) memory was not measured (skipped per `resume.md`).

4. **Refactor needed for `stream_protocol` (and asset/preview schemes):**
   - `pty::protocol::stream_protocol` was decoupled into `dispatch_for_label(app, label, method, path, origin, body, reply: Box<dyn Reply>)` and `dispatch(hub, origins, incoming, reply)`. For Task 6, the `/scheme/terax-pty/*` HTTP route only needs to map `Reply`'s `(status, body)` to an Axum/Hyper response with `Content-Type` (`application/octet-stream` on 200, `text/plain` otherwise), `Cache-Control: no-store`, and `X-Content-Type-Options: nosniff`.
   - `fs::assets::serve` and `fs::preview::serve` still take `UriSchemeContext<'_, R>` and `UriSchemeResponder`, so Task 6 needs the same `dispatch_for_label` split for both.

**Exit criteria:**
- All five checks pass, and one relay costs under about 150 MiB.

**If dispatch or the callback relay fails:**
- Stop and report.
- The fallback is per-feature HTTP endpoints, porting modules one at a time. It costs far more and needs a new plan.

## Phase 1: backend bridge (`src-tauri/src/modules/web_remote/app/`)

### Task 1: command policy and guard test
- [x] `policy.rs`: `enum CommandClass { Read, Write, Control, Secret, Native }` and `fn class(cmd: &str) -> Option<CommandClass>`, covering app and plugin commands (`plugin:store|*`, `plugin:event|*`, `plugin:path|*`, …).
- [x] Guard test: parse `generate_handler!` in `lib.rs` (as the SSH guard test does) and fail on any unclassified command.
- [x] `fn allowed(class, grant, transport) -> bool`:
  - `Native` is never allowed;
  - `Secret` needs HTTPS or loopback, and `secrets_get_all` is never allowed;
  - everything else needs the `app` grant.

### Task 2: the `app` grant
- [x] Add `app: bool` to `Grant` in `devices.rs`. It defaults to false, and old device files load with false.
- [x] Revoking a device or removing `app` closes its relay sessions. (DeviceStore revision + `has_app` ready; session teardown wired in Task 3.)
- [x] Tests.

### Task 3: relay sessions
- [x] `sessions.rs`: create `remote-<n>` hidden webview windows on demand (cap 3) loading `remote-relay.html`.
  - Map session id -> (device id, label, SSE sender).
  - Close after 60 s with no SSE subscriber, on revoke, and on listener stop.
- [x] Teach the window-label helpers in `lib.rs` about `remote-*`:
  - not `main`, not a space window;
  - excluded from close-all-on-main-destroyed ordering, so it closes itself.
- [x] `remote-relay.html` + `src/web/relay/main.ts`: forward every callback (event or channel) to Rust with `remote_relay_push(id, payload)` (a `Native`-class command only the relay label may call).

### Task 4: `POST /api/app/invoke`
- [x] Body: `{ session, cmd, args }` (JSON) or raw bytes with `cmd` in a header, for `InvokeBody::Raw` commands.
  - Check the cookie, the device's `app` grant, `X-Terax-Remote`, same-origin, policy, then the size cap (reuse `MAX_INPUT_BYTES`-style limits per class).
- [x] Build the `InvokeRequest`:
  - fresh callback ids;
  - the relay webview's URL (so `is_local` holds);
  - `app.invoke_key()`, server side only.

  Then dispatch through `on_message` and map `InvokeResponse` to HTTP 200/4xx with the payload.
- [x] Tests with a mock runtime where possible; otherwise the live test in Task 9.

### Task 5: `GET /api/app/callbacks` (SSE)
- [ ] Stream the session's relayed callbacks as `event: callback`, `data: {id, payload}`, with `id:` as a sequence number.
- [ ] Reconnect with `Last-Event-ID` replays a bounded buffer (1,000 events / 4 MiB); beyond that, `event: reset` makes the client reload.

### Task 6: scheme routes
- [ ] Refactor the cores of `pty::stream_protocol`, `fs::assets::serve` and `fs::preview::serve` to take `(label, request)` instead of the Tauri context. The scheme handlers become thin wrappers.
- [ ] Routes `/scheme/terax-pty/*`, `/scheme/asset/*` and `/scheme/terax-doc/*` call the cores with the session's relay label. Same grants, `Cache-Control`, `nosniff`.

### Task 7: serve `/app/`
- [ ] `/app/` returns `index.html` from the bundled assets with `<script src="/app/remote-runtime.js">` inserted first in `<head>`. Other `/app/*` paths map to the bundle.
- [ ] CSP for `/app/`:
  - same as the mirror's;
  - plus `connect-src 'self'`, `worker-src 'self' blob:` and `wasm-unsafe-eval` (already there).
- [ ] No `tauri://` or `ipc://` origins.
- [ ] Only devices with `app` get anything but the pairing page.

### Task 8: register and wire
- [ ] Register the new commands in `lib.rs`, manage the session state, and add `remote-*` to the capabilities.
- [ ] Keep the ACL as narrow as `main`'s, not wider.

### Task 9: live backend test
- [ ] Add `web_remote/app/live_tests.rs`, `#[ignore]` by default, run with a flag:
  - start the app in `--remote-serve` under `xvfb-run` on Linux CI-less (aspirexc);
  - invoke `pty_list` and a store read;
  - subscribe to callbacks and see `fs:changed` after touching a file.

## Phase 2: browser runtime and the app shell

### Task 10: `__TAURI_INTERNALS__` shim (`src/web/runtime/`)
- [ ] `invoke(cmd, args, options)` → `POST /api/app/invoke`, with raw bodies for `Uint8Array`/`ArrayBuffer`. `transformCallback` keeps a local callback table, fed by the SSE stream.
- [ ] `convertFileSrc(path, protocol)` → `/scheme/<protocol>/…`.
- [ ] `metadata` gives `currentWindow` / `currentWebview` the relay label from `/api/app/session`.
- [ ] Plugin internals as the 2.12 packages expect them: event plugin unlisten, and window/webviewWindow calls answered locally or refused.
- [ ] `isRemoteRuntime()` helper in `src/lib/remoteRuntime.ts`, false on desktop.
- [ ] Import guard test (as `src/web/importGuard.test.ts`): the runtime never imports app code.

### Task 11: session bootstrap
- [ ] `/app/` boot:
  - fetch `/api/app/session`, which creates or reuses a relay;
  - open the callback SSE;
  - then let the app's `main.tsx` run.
- [ ] Reconnect UI as in the mirror; on `reset`, reload.

### Task 12: native features in remote mode
- [ ] `isRemoteRuntime()` branches:
  - hide window controls, vibrancy and updater UI;
  - `openSettingsWindow` routes in-page (`#settings/<tab>`);
  - `plugin-opener` URLs open with `window.open`, and opening host paths is hidden;
  - notifications use Web Notifications;
  - the clipboard uses `navigator.clipboard`, with a fallback.
- [ ] Shortcut table: browser-reserved combos get alternatives, shown in the palette.
- [ ] Settings → Remote access shows the `app` grant per device with a warning line.

### Task 13: terminal, tabs, spaces end to end
- [ ] New tab, split, blocks, close; local, WSL (Windows host) and SSH workspaces opened from the browser.
- [ ] Check `pty/gate.rs` tokens for the relay label and the `terax-pty` route under load (yes | head -c 50M).
- [ ] Shortcuts and the command palette.

## Phase 3: files, editor, git

### Task 14: explorer and file ops
- [ ] Browse, create, rename, delete, drag-and-drop move. `fs:changed` is relayed. Upload from the browser (drop files) is a new `Control`-class command writing into the workspace.
- [ ] Download a file from the host: new route `/scheme/download/…` with the asset grant.

### Task 15: editor and LSP
- [ ] Open, edit, save, and format through LSP over the relayed Channel. Check large documents and fast typing (channel ordering).
- [ ] `terax:open-file` and `fs:file-written` work across desktop and browser windows.

### Task 16: previews
- [ ] Markdown, HTML, DOCX, spreadsheet and image previews over the `/scheme/*` routes, including `useAssetGrant`.

### Task 17: git, source control, history
- [ ] Status, diff, stage, commit, branch, history and restore from the browser.
- [ ] Askpass and passphrase prompts for push and pull must surface in the browser session that started them (they go through the relay label). Do not remove or weaken the askpass or passphrase paths.

## Phase 4: AI, agents, ports, desktop tabs

### Task 18: AI and agents
- [ ] AI chat through the `net` proxy Channel. Prove from the network log that no API key reaches the browser. Fix any command that returns a key before enabling AI remotely.
- [ ] The agents launcher (per-workspace commands, PATH hint), agent signals and the status pill.

### Task 19: port proxy for previews and dev servers
- [ ] `/proxy/<port>/…`: a reverse proxy to `127.0.0.1:<port>` for ports Terax itself reports (live server, detected dev servers), with WebSocket upgrade for HMR.
- [ ] Off unless the device has `app`. Never an arbitrary host.

### Task 20: desktop tabs inside the full app
- [ ] A "Desktop" sidebar section listing other windows' tabs from the registry, rendered with the mirror's `RemoteTerminal` (read-only unless `input`).
- [ ] "Take over" moves a terminal into the browser window through the existing handoff, and "Send back" returns it.

## Phase 5: hardening and rollout

### Task 21: limits and audit
- [ ] Per-device rate limits on invoke.
- [ ] An audit log of `Control` and `Secret` commands (command name, device, time; never arguments), shown in Settings → Remote.
- [ ] Idle timeout for `app` sessions (default 12 h).

### Task 22: deploy and verify on aspirexc
- [ ] Rebuild under `--remote-serve`, grant `app` to one device, and run through the feature matrix by hand.
- [ ] Record what was looked at and what was not.
- [ ] Check memory with 1–3 sessions.

## Risks

- **Spike failure:** the callback relay depends on how Tauri 2.12.2 runs callbacks in a webview. If that changes in a Tauri update, the relay page breaks. Pin it with the Task 9 live test.
- **Security:** `app` is remote code execution by design, and the deny-by-default policy, grant, HTTPS rule for secrets and audit log are the guard rails. Keep the listener's bind defaults (loopback, Tailscale opt-in).
- **Browser-reserved shortcuts** cannot all be captured (Ctrl+W, Ctrl+T in some browsers). Remap rather than fight them.
- **Relay memory** on small hosts: cap sessions and close idle ones.
- **Clipboard** needs a secure context; plain HTTP on the tailnet falls back to a paste dialog.
- **Bundle size:** `/app/` loads the whole desktop bundle (Monaco, Ghostty WASM). Fine on a LAN or tailnet, slow on mobile data. The mirror remains for phones.
