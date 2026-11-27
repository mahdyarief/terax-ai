# Web Remote: monitor and drive every terminal tab from a browser

> **For agentic workers:** implement task by task. Each task is one commit on `dydev`, test first where it adds behaviour. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Open `http://<host>:<port>/web-index.html` in any browser (desktop or phone, typically over Tailscale) and see every terminal tab of every workspace (local, WSL, SSH; main window and space windows) live, in terminal or block mode, exactly as the desktop shows it, and type into any of them when the device is allowed to. On a machine nobody sits at (the test host `aspirexc`), the browser can also open and close terminal tabs.

**Status (2026-11-27):** planned, nothing implemented.

## Why this works (evidence, no prototype needed)

| Claim | Evidence in this repo |
|---|---|
| The terminal renderer runs in a plain browser | `packages/ghostty-core` is Ghostty's VT core compiled to WASM (`ghostty-vt.wasm`) with the render bridge from Restty, a browser terminal (`packages/ghostty-core/adapted/UPSTREAM.md`). The backends are WebGL and WebGPU. The core, GPU and model files import nothing from `@tauri-apps`; its tests already run outside Tauri, in vitest. |
| Only thin glue is Tauri-bound | In `src/modules/terminal/ghostty/`, only `useGhosttyTerminalSession.ts` (PTY plumbing), `windowPresentation.ts` and `renderScheduling.ts` (window focus and visibility) import `@tauri-apps`. The web client replaces exactly those. |
| A remote viewer can follow output without disturbing the desktop | Every PTY keeps an ordered, sequenced replay log (`Session::replay`, `src-tauri/src/modules/pty/replay.rs`: `replay_all`, `replay_after(checkpoint)`, 8 MiB / 65,536 events). A viewer reads from it; the desktop's credit-based stream (`hub.rs`, `output.rs`) is untouched. |
| Blocks come with the bytes | Blocks are built from OSC 133 marks in the output (`da_filter.rs` passes them through; `GhosttyBlocks.ts` builds blocks). The replay already carries them, which is how a tab moved between windows keeps its blocks. |
| Every terminal tab has a live PTY | `planTabRetention` never cools a terminal tab ("cooling it would drop a live session"). |
| The bundle is already in the binary | Tauri embeds `dist/`; `app.asset_resolver()` serves it. The Vite build is already multi-page (`index.html`, `settings.html` in `vite.config.ts` `rollupOptions.input`); `web-index.html` is a third entry. |
| The shape is proven elsewhere | Orca serves `web-index.html` plus hashed `./assets/*` from its runtime on the same port, with the renderer's own chunks reused (checked live on `aspirexc:6768`). wmux tees PTY output to browsers over SSE plus POST and gates everything with pairing and per-device grants. |

## Decisions

- **A dedicated web app, not the whole Terax UI.** `web-index.html` loads `src/web/`, which reuses the terminal renderer, blocks, agent icons and tab status, and talks only to the remote API. There are no shims for the roughly 150 Tauri commands.
- **Transport: HTTP only.** SSE carries tabs and terminal output; POST carries input and actions. This matches the existing HTTP-shaped PTY stream, reconnects natively, and keeps auth on every request. Unlike Orca, no socket is opened before auth.
- **Viewers are passive about size.** The browser renders at the PTY's current size (scaled to fit, with zoom) and never resizes the desktop's terminal. An explicit "use this screen size" claim is out of scope for v1.
- **Mirroring, not handoff.** Remote viewers never take the PTY's attachment. Input goes into the session's ordered writer queue, the same queue the desktop's keystrokes use.
- **Tabs stay owned by the windows.** Each window publishes its terminal tabs to a registry in Rust. Remote "new tab" and "close tab" are forwarded to the window that hosts the workspace, which runs its existing code.
- **Security, deny by default:**
  - Off until turned on.
  - Default bind is `127.0.0.1`. A Tailscale address (`100.64.0.0/10`, already WireGuard-encrypted) is an explicit option. "All interfaces" over plain HTTP needs a second, explicit opt-in.
  - Pairing uses a one-time code that expires after 10 minutes and allows 5 attempts. The pairing URL carries it in the fragment, and a QR shows that URL.
  - A paired device gets a random 256-bit token, stored hashed, in an `HttpOnly; SameSite=Strict` cookie (`Secure` behind HTTPS).
  - Each device has a grant. It is read-only by default; input and tab actions are allowed per device. Grants are enforced on the server.
  - Revoking a device kills its streams at once.
  - The server checks a Host allowlist (DNS rebinding) and rejects `Sec-Fetch-Site: cross-site`.
  - Every POST must be same-origin (`Origin` must match) and carry `X-Terax-Remote: 1`.
  - Headers: CSP with no inline script, `X-Frame-Options: DENY`, `nosniff`, `no-referrer`.
- **No relay and no E2EE layer.** Remote reach comes from Tailscale (optionally `tailscale serve` for HTTPS) or a reverse proxy.

## Out of scope (v1)

- Editor, preview, git, AI and Settings in the browser. These tabs are listed but their content is not shown.
- Terminals surviving Terax quitting, which would need a separate daemon (as in wmux and Orca).
- Resizing the PTY from the browser.
- Per-agent environment variables.

---

## Phase 1: Backend (Rust, `src-tauri/src/modules/web_remote/`)

### Task 1: replay change notification
- [x] Add a `tokio::sync::watch::Sender<u64>` to `Session`, holding the replay sequence. The reader bumps it after every `push_output` and `push_resize`; exit sends a final value.
- [x] `Session::subscribe_replay() -> watch::Receiver<u64>`.
- [x] Test: pushing output wakes a receiver with the new sequence, and `replay_after(old)` returns exactly the new events.

### Task 2: tab registry
- [ ] Command `remote_publish_tabs(tabs: Vec<RemoteTab>)`. The window label comes from the calling webview, never from the payload. The registry is keyed by window label. A version counter and a `watch` channel go up on every change, and a window's tabs are dropped when the window closes.
- [ ] `RemoteTab = { id, title, spaceId, workspace (scope key + label), mode: terminal|blocks, panes: [{ leafId, ptyId, active }], agent: { kind, state } | null }`. Non-terminal tabs are sent as `{ id, title, kind }` only.
- [ ] Frontend `src/modules/web-remote/publishTabs.ts`: subscribes to `useTabs`, the leaf→PTY bindings (`pty-bridge`) and agent activity, and publishes at most every 250 ms while remote access is on.
- [ ] Tests: merging two windows, dropping a closed window, and no publish when nothing changed.

### Task 3: server skeleton
- [ ] hyper 1 (as in `live_server`) on a tokio listener. Commands `remote_access_start({ bind: "loopback"|"tailscale"|"all", port, allowPlainLan })`, `remote_access_stop` and `remote_access_status`.
- [ ] Bind resolution: for `tailscale`, find this machine's `100.x` address. The listener refuses `all` without `allowPlainLan`.
- [ ] Host allowlist: localhost, bound IPs, MagicDNS name, configured extras.
- [ ] Static files: only `web-index.html` and `assets/*`, read through `app.asset_resolver()`. GET/HEAD only, `immutable` cache for hashed assets, `no-store` for HTML.
- [ ] Security headers and CSP.
- [ ] Tests: Host rejection, traversal and unknown path give 404, and the headers are present.

### Task 4: pairing and devices
- [ ] Pairing code from `A-Z2-9` without lookalikes, 6 characters, single use, 10 min TTL, 5 attempts. `remote_pairing_start` returns `{ code, url }` with `url = http(s)://<host>:<port>/web-index.html#pair=<code>`.
- [ ] `POST /api/pair` with `{ code, deviceName }`. On success it sets the device cookie.
- [ ] Device store in app data (`remote-devices.json`, 0600 on Unix): `{ id, name, tokenSha256, grant: { input, tabs }, createdAt, lastSeen }`. Tokens are compared in constant time.
- [ ] Commands `remote_devices_list`, `remote_device_update(grant)`, `remote_device_revoke`. Revoking closes that device's streams.
- [ ] Tests: expiry, attempt limit, single use, a revoked cookie gets 401, a cross-site POST gets 403, and a missing header gets 403.

### Task 5: `GET /api/tabs` (SSE)
- [ ] Event `tabs` carries the full registry snapshot on connect and on every version bump. A heartbeat goes out every 25 s.
- [ ] Test: a publish reaches a connected stream.

### Task 6: `GET /api/pty/:id/stream` (SSE)
- [ ] On connect, or when `Last-Event-ID` is older than the log:
  - `meta { cols, rows, truncated }`;
  - `snapshot`: the base64 of `replay_all`, applied to a fresh model.
- [ ] Afterwards:
  - `data { seq }` carrying base64 output;
  - `resize { cols, rows }`;
  - `exit { code }`.
- [ ] The event id is the replay sequence, so a reconnect resumes with `replay_after`.
- [ ] A viewer that falls behind the log, or has more than 2 MiB queued, gets a new snapshot instead of the backlog.
- [ ] Limits: 16 streams per device.
- [ ] The PTY must be in the registry; ids that are not listed give 404.
- [ ] Tests: snapshot then live data, resume by id, lag leads to a snapshot, an unknown id gives 404.

### Task 7: `POST /api/pty/:id/input`
- [ ] Requires `grant.input`. The body is raw bytes, at most 64 KiB.
- [ ] Input goes through a new `Session::write_remote(bytes)` into the same ordered writer queue. It does not need the window's attachment lease, because it never takes the attachment.
- [ ] Tests: refused without the grant, and the bytes arrive in order.

### Task 8: remote tab actions
- [ ] `POST /api/tabs` with `{ action: "new", spaceId, mode }` or `{ action: "close", tabId }`. Requires `grant.tabs`.
- [ ] Rust emits `remote:tab-action` to the window hosting that space. That window runs its existing new-tab or close-tab code and the registry update follows.
- [ ] Tests: the action is routed to the right window and refused without the grant.

### Task 9: wiring
- [ ] Manage the state and register the commands in `lib.rs`.
- [ ] Add entries to `ssh_commands_guard_tests.rs` for any command that takes a `WorkspaceEnv`.
- [ ] Learning note.

## Phase 2: Web client (`web-index.html`, `src/web/`)

### Task 10: entry and pairing
- [ ] Add `web-index.html` to `rollupOptions.input`. `src/web/main.tsx` shows the pairing screen: it reads `#pair=` from the fragment, asks for a device name, then removes the fragment. It then lazy-loads `RemoteApp`.
- [ ] Guard test: nothing reachable from `src/web/main.tsx` imports `@tauri-apps/*`.

### Task 11: remote terminal view
- [ ] `useRemoteTerminal(ptyId)`: an EventSource feeds the Ghostty model. A snapshot resets the model, data is written, and a resize sets the size.
- [ ] Renders at the PTY size, scaled to fit the width, with pinch or zoom controls. WebGL by default; WebGPU when available.
- [ ] Theme colors are sent in `meta`.
- [ ] Tests: model state after snapshot plus data, and a reconnect does not duplicate output.

### Task 12: block mode
- [ ] Reuse `GhosttyBlocks` and the block overlay for tabs whose `mode` is `blocks`.

### Task 13: tab list
- [ ] Tabs grouped by workspace (local, WSL, SSH), with agent icon and state (running, waiting, done), updated live.
- [ ] Split panes show as a pane picker.
- [ ] Phone layout: the list is a drawer.

### Task 14: input
- [ ] A "Type" toggle, only when the device has the grant. Keys are encoded with the Ghostty input encoder.
- [ ] A phone key row: Esc, Tab, Ctrl, arrows, Enter.
- [ ] A paste box sends text as one write.

### Task 15: tab actions
- [ ] "New terminal" per workspace, and "Close" on a tab, when the device has the grant.

## Phase 3: Desktop controls

### Task 16: UI
- [ ] Settings > Remote access:
  - on/off;
  - bind (This computer only / Tailscale / All interfaces + plain-HTTP warning) and port;
  - "Pair a device" with the code, URL and QR;
  - device list with input and tab grants and Revoke.
- [ ] Status bar pill while it is on, showing connected viewers.

### Task 17: optional HTTPS through Tailscale
- [ ] "Use HTTPS (tailscale serve)" runs `tailscale serve --bg --https=443 http://127.0.0.1:<port>` and adds the MagicDNS name to the Host allowlist. It never overwrites a serve config it did not create, and removes its own entry on stop.

### Task 18: run without a desktop session
- [ ] A `--remote-serve` CLI flag starts with remote access on (bind and port from settings or flags) and the main window hidden.
- [ ] On Linux without a display it runs under `xvfb-run`. A systemd user unit example goes in `docs/`.

## Phase 4: Test host `aspirexc`

`aspirexc` (Ubuntu 26.04, x86_64, 12 cores, 14 GiB, Tailscale `100.84.56.62`, SSH `dy-aspire`) is headless. It has Xvfb but not `libwebkit2gtk-4.1-0`, and passwordless sudo. Orca already runs there on port 6768, so Terax uses another port, for example 7681.

### Task 19: deploy
- [ ] Install the runtime dependencies with apt: `libwebkit2gtk-4.1-0`, `libgtk-3-0` and the `.deb`'s own dependencies.
- [ ] Build on the host from `dydev` (Rust, Node and pnpm in user space; avoids spending GitHub Actions), or install a `.deb` built there.
- [ ] Run `xvfb-run terax --remote-serve --bind tailscale --port 7681` as a systemd user unit.
- [ ] Pair from a browser on the tailnet. Verify:
  - live output;
  - blocks;
  - input with and without the grant;
  - new and close tab;
  - reconnect after a network drop;
  - a phone layout.

## Risks

- **Mobile WebGPU** is patchy, so WebGL is the default for the web client.
- **Plain HTTP on the tailnet** means the cookie cannot be `Secure`. Acceptable inside WireGuard; `tailscale serve` gives real HTTPS.
- **An SSH workspace's output** already lands in the local replay log, so it is covered. Its input goes through the same writer queue.
- **A shell without OSC 133 integration** shows as a plain terminal. Blocks behave as on the desktop.
