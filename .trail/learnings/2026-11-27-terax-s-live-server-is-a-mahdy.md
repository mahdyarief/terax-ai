---
title: "Terax's Live Server is a real localhost HTTP server, guarded by Host checks and confined reads"
author: Mahdy
date: 2026-11-27
branch: dydev
cost: { minutes: 48, tokens: 146049 }
id: l-844005c5
scope: repo
status: active
src: agent
touches:
  - src-tauri/src/modules/live_server/http.rs
  - src-tauri/src/modules/live_server/mod.rs
  - src/modules/live-server/liveServer.ts
  - src/modules/live-server/liveServer.test.ts
  - src/modules/statusbar/DevServersPill.tsx
  - src/modules/statusbar/DevServersPill.test.tsx
  - src-tauri/src/modules/remote/live_server_tests.rs
---

## Decided
- Serving a folder is a real HTTP server on 127.0.0.1, not a custom URI scheme. It works in any browser and on other devices, and gives a real `http://localhost` origin (fetch, ES modules, localStorage, service workers, OAuth redirects). The user chose this for universality after weighing a scheme.
- Guards: only `localhost` and IP literals pass the `Host` check, so a website cannot reach the server through DNS rebinding. Paths are decoded per segment and dotfiles are refused. Every file is opened with `read_confined` plus the secret-path policy. No CORS headers are sent. Sharing on the LAN (0.0.0.0) is opt-in per server, and a drive, the filesystem root or a home folder is never served.
- Files go through the workspace backend, so a folder on WSL or an SSH server is served by this machine. Live reload uses a long-poll on `/__terax/reload` with a version number: `notify` watches local and WSL folders, and saves from the Terax editor (`fs_write_file`) bump every server whose folder holds the file, which also covers SSH.

## Watch out
- A new command that takes a `WorkspaceEnv` must be listed in ssh_commands_guard_tests.rs with a server test (see remote/live_server_tests.rs).
- Pages get `<script src="/__terax/live-reload.js">` before `</body>`, and `/__terax/` is reserved.
- Sharing on the LAN binds 0.0.0.0, which triggers the Windows firewall prompt the first time.
- Changing sharing restarts the server on the same port when that port is free.

In the code: src-tauri/src/modules/live_server/{mod.rs,http.rs}, src/modules/live-server/liveServer.ts, src/modules/statusbar/DevServersPill.tsx (Stop/Share), Explorer "Serve with Live Server", the HtmlPreview bar button.
