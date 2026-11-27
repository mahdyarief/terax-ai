---
title: "Dev servers in the status bar come from addresses terminals print, not from scanning ports"
author: Mahdy
date: 2026-11-27
branch: dydev
cost: { minutes: 22, tokens: 181217 }
id: l-81deb116
scope: repo
status: active
src: agent
touches:
  - CHANGELOG.md
  - package.json
  - src-tauri/Cargo.lock
  - src-tauri/Cargo.toml
  - src-tauri/crates/terax-agent/Cargo.toml
  - src-tauri/tauri.conf.json
  - src-tauri/tauri.windows.conf.json
  - src/modules/terminal/lib/devServers.ts
  - src/modules/terminal/lib/devServers.test.ts
  - src/modules/statusbar/DevServersPill.tsx
---

## Decided
- A dev server shows in the status bar when a terminal prints a loopback address with a port (Vite's `Local:` line, uvicorn's `Running on`). No OS-level port scanning: reading output works the same for local, WSL and SSH terminals and needs no native code or permissions.
- An address belongs to the command that printed it: it is cleared when shell integration reports a command starting or ending, when the shell exits, and when the terminal is respawned or closed. With shell integration, an address printed at the prompt is ignored.
- On an SSH workspace the address is that server's localhost, so Open in Preview and Open in Browser are disabled and only Copy Address works. WSL counts as local, because WSL2 forwards localhost to Windows.

## Tried and ruled out
- Matching each output chunk on its own: Vite bolds the port (`localhost:\x1b[1m5173`), and an address can be split across chunks. The scanner removes escapes and only reads complete lines, keeping at most 4 KB waiting for a line end.

## Watch out
- Scan the bytes before `model.write`, since the model may take the buffer.
- A server started in the background (`npm run dev &`) disappears when the prompt returns, because the command that printed it "ended".
- Zustand stores render their initial state under SSR, so test the presentational `DevServersMenu` with servers passed in, not `DevServersPill`.

In the code: src/modules/terminal/lib/devServers.ts (scanner, store), src/modules/statusbar/DevServersPill.tsx, wiring in src/modules/terminal/ghostty/useGhosttyTerminalSession.ts; port labels moved to src/modules/preview/lib/portPresets.ts.
