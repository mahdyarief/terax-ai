---
title: "A running Terax gets a new pairing code on SIGHUP, never SIGUSR1"
author: Mahdy
date: 2026-11-28
branch: dydev
cost: { minutes: 40, tokens: 320114 }
id: l-714bd38e
scope: repo
status: active
src: agent
touches:
  - docs/superpowers/plans/2026-11-28-web-remote-app-fixes/plan.md
  - src-tauri/src/lib.rs
  - src-tauri/src/modules/web_remote/app/invoke.rs
  - src-tauri/src/modules/web_remote/mod.rs
  - src/app/App.tsx
  - src/lib/remoteRuntime.ts
  - src/modules/spaces/SpaceSwitcher.tsx
  - src/modules/spaces/lib/crossWindowSync.test.ts
  - src/modules/spaces/lib/crossWindowSync.ts
  - src/modules/spaces/lib/handoff.ts
---

## Decided
`--remote-serve` prints a new pairing URL when the terax process gets SIGHUP. Under systemd the unit gets `ExecReload=/usr/bin/pkill -HUP -P $MAINPID -x terax`, so `systemctl --user reload terax-remote` mints a code without a restart.

## Tried and ruled out
- SIGUSR1: the process segfaulted on aspirexc right after the signal. WebKitGTK's JavaScriptCore installs its own SIGUSR1 handler to suspend threads for garbage collection (`JSC_SIGNAL_FOR_GC`; older WebKit used SIGUSR2), and tokio taking the signal over breaks it. Do not use SIGUSR2 either.
- `systemctl --user kill -s <sig> terax-remote`: it signals every process in the unit, and the main process is `xvfb-run`, which exits on the signal and takes the service down. Signal only `terax`, the child of the main PID.
- The control socket: every terminal holds a token for it, so a shell in Terax could pair devices.

## Watch out
The listener comes up about 30 s after the service starts on aspirexc (the main webview loads first); probe after that, not after a few seconds.

In the code: `reprint_pairing_on_sighup` and `print_pairing_url` in `src-tauri/src/lib.rs`; `Pairing` in `src-tauri/src/modules/web_remote/devices.rs`; `docs/remote-serve.md`.
