---
title: "A browser session can never manage remote access, its devices or the ports the proxy may reach"
author: Mahdy
date: 2026-11-28
branch: dydev
cost: { minutes: 20, tokens: 175119 }
id: l-f5f80b16
scope: repo
status: active
src: agent
touches:
  - docs/superpowers/plans/2026-11-28-web-remote-full-app/plan.md
  - src-tauri/src/lib.rs
  - src-tauri/src/modules/web_remote/app/audit.rs
  - src-tauri/src/modules/web_remote/app/callbacks.rs
  - src-tauri/src/modules/web_remote/app/invoke.rs
  - src-tauri/src/modules/web_remote/app/mod.rs
  - src-tauri/src/modules/web_remote/app/policy.rs
  - src-tauri/src/modules/web_remote/app/proxy.rs
  - src-tauri/src/modules/web_remote/app/schemes.rs
  - src-tauri/src/modules/web_remote/app/serve.rs
---

## Decided
- The remote_* management commands are class Native in the bridge policy: remote_access_start/stop, remote_pairing_start, remote_device_update/revoke and remote_report_ports. They are refused for every grant and transport.
- Reading the status, the device list and the audit log stays allowed.
- In the browser runtime, the Settings "Remote access" section is a short notice, the status bar switch is hidden, and DevServersPill does not report ports.

## Tried and ruled out
- Class Control, as first implemented: any device with the `app` grant could grant or revoke devices, pair new ones, move the listener to every interface, or report any port and reach any local service through /proxy/.

## Watch out
- Any new command that changes remote access, pairing, grants or the proxy allowlist must be Native. The guard test only checks that a command has a class, not which class.

In the code: src-tauri/src/modules/web_remote/app/policy.rs (`class`, test `a_browser_never_manages_remote_access_its_devices_or_the_proxy_ports`), src/settings/sections/RemoteSection.tsx, src/modules/statusbar/StatusBar.tsx, src/modules/statusbar/DevServersPill.tsx
