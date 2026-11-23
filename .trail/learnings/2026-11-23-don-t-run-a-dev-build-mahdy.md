---
title: "Don't run a dev build of Terax while the installed Terax is open"
author: Mahdy
date: 2026-11-23
branch: dydev
cost: { minutes: 44, tokens: 237441 }
id: l-89020db5
scope: repo
status: active
src: agent
touches:
  - docs/superpowers/plans/2026-11-23-remote-ssh-phase3-transport.md
  - src-tauri/src/lib.rs
  - src-tauri/src/modules/remote/commands.rs
  - src-tauri/src/modules/remote/mod.rs
  - src-tauri/src/modules/remote/ssh_config.rs
  - src/app/App.tsx
  - src/modules/command-palette/commands.ts
  - src/modules/remote/ConnectSshDialog.tsx
  - src/modules/remote/RemoteConnections.tsx
  - src/modules/remote/RemoteLogDialog.tsx
---

## Watch out
- A dev build (`pnpm tauri dev`) has the same identifier as the installed app and writes the same control descriptor, `%LOCALAPPDATA%\terax\control.json` (`dirs::cache_dir()/terax`), then removes it on exit. The running installed Terax then loses its control endpoint: `terax-cli`, hooks and askpass in it stop working until it restarts. Its settings store and app data are shared too.
- So the SSH end-to-end check for Phase 3 runs the app's own code without the GUI: `remote::e2e_tests` drives the real `Registry`, `SshConnector` and `terax-cli` askpass against a server, with a stand-in control endpoint (`ControlState::for_askpass_tests`).

## Decided
- Click through the UI only with the installed Terax closed, or with the owner's go-ahead.

In the code:
- `src-tauri/src/modules/control.rs` (`descriptor_path`)
- `src-tauri/src/modules/remote/e2e_tests.rs`
