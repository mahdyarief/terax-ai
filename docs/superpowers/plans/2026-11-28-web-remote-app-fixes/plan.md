# Web Remote full app: fixes after the first browser walkthrough

> **For agentic workers:**
> - Implement task by task on `dydev`: one task per commit, test first.
> - Read `../2026-11-28-web-remote-full-app/agent-prompt.md`; its rules apply here too: security, the user's running Terax, pnpm only, no `cargo fmt`, Indonesian reports.
> - Deploy to aspirexc the way that prompt describes, and tell the user before you restart `terax-remote`.

**Status (2026-11-28):** planned. Found while using `/app/` from Chrome against aspirexc, after `5732a20` (the terminal stream now sends the device cookie).

## What the user saw, and why

| # | Symptom in `/app/` | Root cause (verified) |
|---|---|---|
| 1 | "New tab" / "New terminal" adds no tab | A browser session's window label is `remote-<n>`. No space has `hostWindowLabel == "remote-<n>"`, so `useSpacesBoot` takes the "nothing hosted" branch: `hydrate(registry, null, {})` and `setActiveSpaceForNewTabs("")` (`src/modules/spaces/lib/useSpacesBoot.ts:186`). `newTab` then stamps `spaceId: ""` (`src/modules/tabs/lib/useTabs.ts:918`), so the tab exists but lives in a space nothing shows. |
| 2 | No spaces | Same cause. On aspirexc the registry (`~/.local/share/app.crynta.terax/terax-spaces.json`) holds only `default`, hosted by `main`, a headless Xvfb window nobody looks at. The browser sees spaces "in another window" and hosts none. The label also changes on every session (`remote-0`, `remote-1`, …), so a space hosted by a browser session is orphaned on reconnect. |
| 3 | "Open folder" does nothing | All three folder flows call `workspace_pick_folder` (`src/app/App.tsx:1504`, `:1536`, `:1593`). It opens a native `rfd` dialog on the host and is class `Native` in `policy.rs:177`, so the bridge refuses it. There is no in-app picker for local folders. |
| 4 | The block tab's logo does not render | `BlockWatermark` loads `src="/logo.png"`. The listener serves only `web-index.html` and `assets/*` (`http_rules::asset_key`), so `/logo.png` is 404. `notification.mp3` and `licenses/` (Vite `public/` files at the root of `dist`) are 404 for the same reason. |
| 5 | Unclear whether typing works | Verified: printable keys reach the shell. Each key is a `pty_write` with the right bytes and answers 200. Space, Enter, Backspace and Ctrl+U were **not** sent, but they were synthetic `keydown`s from the browser tool, which Ghostty's input path likely ignores. The repo's bench harness types through native OS events for the same reason. Still unverified with a real keyboard. |
| 6 | No quick keys or typing area in `/app/` | By design: they belong to the mirror (`/web-index.html`). The mirror links nowhere to `/app/`, and the pairing URL points at the mirror, so users land on the light view. |

## Decisions

1. **A browser session gets a stable window label per device: `remote-<device id>`**, not `remote-<n>`.
   - Spaces hosted by it survive reconnects and service restarts, exactly as `space-*` windows do.
   - A second session from the same device takes the label over: the older session is closed with "Opened in another tab", the way `pty` handoff supersedes.
   - The label stays within `remote-*`, so capabilities and `policy.rs` checks are unchanged.
2. **A browser session that hosts no space adopts spaces instead of showing nothing:**
   - **headless host** (`--remote-serve`): it takes over the spaces hosted by `main`, through the existing space handoff (`spaceTransfers`, `SPACE_TABS_MOVED_EVENT`). Nobody is looking at `main` there.
   - **desktop host** (a person at the machine): it creates its own space, named after the device, rooted at the home folder, and never takes the desktop's tabs. Other windows' spaces stay in the switcher and can be brought over on purpose.
   - **How Rust tells them apart:** the session API reports `headless: true` when the process runs with `--remote-serve`.
3. **The folder picker becomes an in-app dialog in the browser**, built on the existing `RemoteFolderPicker` UI and generalized to take a `browse(path)` function.
   - For the local workspace it lists directories through the already-allowed read commands.
   - `workspace_pick_folder` stays `Native`.
   - The desktop keeps the native dialog.
4. **The listener serves the Vite `public/` files by an explicit allowlist** (`logo.png`, `notification.mp3`, `licenses/*`), not "any file at the root". `remote-relay.html`, `settings.html` and `index.html` stay unreachable outside `/app/`.
5. **Special keys are verified with real key events** before anything is changed, so the cause is not guessed (see Task 5).

## Tasks

### Task 1: serve the Vite public files (symptom 4)
- [x] Test first (`http_rules.rs`):
  - `asset_key("/logo.png")` and `asset_key("/notification.mp3")` give their keys;
  - `asset_key("/licenses/<file>")` gives a key under `licenses/`;
  - `/remote-relay.html`, `/settings.html`, `/index.html`, `/licenses/../web-index.html` and dotfiles give `None`.
- [x] Implement the allowlist in `asset_key`. Caching for these files is `no-store` (like the page), not the immutable rule for hashed `assets/`.
- [x] Server test: `GET /logo.png` answers 200 with `image/png`.
- [ ] After deploy: in `/app/`, open a block tab and confirm the watermark logo loads (network 200), and that an agent notification plays its sound.

### Task 2: a stable per-device window label (symptom 2, part 1)
- [x] Rust, `app/sessions.rs`: the relay label is `remote-<device id>`.
  - When a session for that device is already open, it is closed first: the relay webview is destroyed, and its callbacks SSE ends with `event: superseded`.
  - The session id stays random and per session. Only the label is stable.
- [x] Revoking a device, or removing its `app` grant, still closes the session (existing behaviour, keep its test).
- [x] Tests:
  - two sessions from one device: the first is superseded, and the label is reused;
  - two devices get different labels;
  - a label never holds a character outside `[a-z0-9-]`.
- [x] Shim (`src/web/runtime/main.ts`): on `superseded`, show "Terax was opened in another tab on this device" with a "Use it here" button (reload, which takes the session back) instead of the reconnect banner. The stream also answers `superseded` when the old page reconnects after the takeover, so it never opens a session of its own and the two tabs do not take turns. `open_relay` destroys a window still under the label and waits for it to go, since labels are now reused.

### Task 3: a browser session always shows a space (symptoms 1 and 2)
- [x] `POST /api/app/session` reports `headless` (true under `--remote-serve`). It also reports `deviceName`. The runtime keeps the reply on `__TERAX_REMOTE_SESSION__`, read through `remoteSessionInfo()`.
- [x] `useSpacesBoot`, under `isRemoteRuntime()`, when `hosted` is empty:
  - headless: move every space hosted by `main` to this label through the existing "bring back to this window" path (`handleBringSpaceToWindow`, which `main` answers for any target window). Their tabs and live PTYs come along. Done in `App.tsx` once spaces are hydrated (`planRemoteSpaces` in `spaces/lib/remoteSpaces.ts`). `reclaimHost` and the switcher's "Bring back" now accept browser windows (`isHomeWindow`). If after 20 s the browser still hosts nothing, it makes its own space instead of reassigning the registry by hand.
  - desktop host: create a space `{ name: <device name>, root: home, hostWindowLabel: <label> }` and make it active.
  - Either way, `setActiveSpaceForNewTabs` gets a real id, never `""`.
- [x] Guard in `newTab`: not added. App already syncs `activeSpaceIdRef` from the active space (`App.tsx`, the `setActiveSpaceForNewTabs(activeSpaceId)` effect), so a fallback to `useSpaces.activeId` would add nothing; the empty id only happened because the browser hosted no space, which the adoption above removes.
- [x] Tests:
  - `resolveBootTargets` and boot under the remote runtime for headless and desktop hosts;
  - pure functions for the adoption choice.
- [ ] After deploy, on aspirexc:
  - the browser shows the `default` space with its tabs;
  - "New tab" adds a tab;
  - a new space can be created, and still exists after reload and after a service restart;
  - the switcher lists the spaces.

### Task 4: an in-app folder picker in the browser (symptom 3)
- [x] Route every folder pick through `pickWorkspaceFolder()` (`src/modules/workspace/env.ts`). Replace the three direct `invoke("workspace_pick_folder")` calls in `App.tsx`.
- [x] Share the picker body instead of a new generic dialog: `FolderListing` (exported from `RemoteFolderPicker.tsx`) takes optional `childPath` / `isRoot`, so it lists Windows hosts too; the browser dialog is `workspace/BrowserFolderPicker.tsx` with its store in `workspace/folderPickerStore.ts` (not `browserFolderPicker.ts`: the two names differ only in case and clash on Windows). The SSH picker is unchanged.
- [x] Under `isRemoteRuntime()`, `pickWorkspaceFolder()` opens the dialog and resolves with the chosen path, or `null` on cancel.
  - It lists folders with `list_subdirs` (class `Read`, no new command) for the current workspace env, which covers local, WSL and SSH alike.
- [x] Tests:
  - a pure helper that turns a listing into directory rows (hidden folders behind a toggle);
  - `pickWorkspaceFolder` resolves from the dialog in the remote runtime and still invokes the native command on the desktop;
  - a render test of the dialog with props.
- [ ] After deploy: "Open folder", "Open folder as space…" and "Choose folder" for a space all work from the browser, and the explorer shows the chosen root.

### Task 5: verify special keys with real key events (symptom 5)
- [ ] Drive Chrome through `chrome.debugger` `Input.dispatchKeyEvent`, which creates trusted events, or ask the user to type: Space, Enter, Backspace, Tab, arrows, Ctrl+C, Ctrl+U and Escape in a `/app/` terminal.
  - Pass: the `pty_write` bodies are `20`, `0d`, `7f`, `09`, `1b[A…`, `03`, `15`, `1b`.
  - Pass: `touch /tmp/<marker>` creates the file on aspirexc.
- [ ] Only if a key is missing with real events: trace `keydown` → Ghostty encode → `pty_write` in the remote runtime. Check the shortcut remapping added for the browser (`src/modules/shortcuts/shortcuts.ts`, `remote` parameter), since a browser-reserved remap can swallow a key. Fix with a test.

### Task 6: from the mirror to the full app (symptom 6)
- [ ] `GET /api/me` already returns the grant. The mirror header shows "Open full app" (link to `/app/`) when `grant.app` is true.
- [ ] The pairing URL keeps pointing at the mirror, since a new device starts without `app`. Settings and the headless log line add "after granting app access, open /app/".
- [ ] Test: the header renders the link only with the grant.

### Task 7: pairing codes without a restart, several at once (requested 2026-11-28)
Done. Note: `afd7767` had rewritten two of the RED tests to match the old single-code behaviour (`only_the_newest_code_stays_open` expected the earlier code to fail, and the several-devices test was dropped). They are restored. A test is never changed to fit the code it is meant to change.
- [x] `Pairing` keeps up to `PAIRING_OPEN_CODES` (8) open codes, each one-time with a 10-minute lifetime. A new code leaves the others working, and the oldest drops past the cap.
  - Wrong attempts count across all codes. `PAIRING_ATTEMPTS` wrong attempts end them all.
  - `start()` resets the counter.
- [x] Unix: on `SIGHUP` (first planned as `SIGUSR1`, which segfaulted the process on aspirexc: WebKitGTK's JavaScriptCore uses it for GC thread suspension) while remote access runs, mint a code and print the same `terax: pair a browser within 10 minutes: <url>` line to stderr.
  - Use tokio `signal`. That is a feature of an existing dependency, not a new crate.
  - Only the process owner can send the signal. The control socket is not used, because every terminal holds a token for it.
- [x] `docs/remote-serve.md`:

  ```
  systemctl --user reload terax-remote   # ExecReload=/usr/bin/pkill -HUP -P $MAINPID -x terax
  journalctl --user -u terax-remote | grep "pair a browser" | tail -1
  ```
- [x] Tests: the pairing unit tests above. The signal handler is a thin wrapper around `start_pairing`, so keep it out of unit tests and verify it on aspirexc: two codes, two devices paired, no restart (checked at deploy).

## Order
Task 1 (small and independent). Then Task 7 (already started). Then Tasks 2 → 3, which depend on each other. Then Task 4, then Task 5, then Task 6. Deploy after Tasks 1+7, after Task 3, and after Task 4, so the user can try each step.

## Verification notes
- `fastmcp-browser` on this machine:
  - screenshots fail ("image readback failed");
  - fill/type and special keys are synthetic, and the terminal ignores them;
  - Radix menus need `browser_pointer_click`, not `browser_click`;
  - single printable keys via `browser_press` do reach the terminal after a pointer click.
- Use the network log (decode `pty_write` bodies) and side effects on aspirexc (`ls /tmp/<marker>`) as evidence.
- Every symptom needs a check in the real browser after deploy, not only unit tests. The cookie bug in `5732a20` passed every unit and live test.

## Risks
- Taking over `main`'s spaces on a headless host moves live PTYs between webviews. The handoff path was built for desktop windows; watch for a replay gap on the first attach.
- A stable label means two browser tabs of one device cannot both run the app. That is accepted. The mirror view still works in parallel.
