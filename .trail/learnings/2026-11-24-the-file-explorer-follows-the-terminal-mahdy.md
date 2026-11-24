---
title: "The file explorer follows the terminal's cd only inside the space's root"
author: Mahdy
date: 2026-11-24
branch: dydev
cost: { minutes: 10, tokens: 25230 }
id: l-229e656f
scope: repo
status: active
src: agent
touches:
  - TERAX.md
  - src-tauri/Cargo.lock
  - src-tauri/Cargo.toml
  - src-tauri/src/lib.rs
  - src-tauri/src/modules/pty/mod.rs
  - src-tauri/src/modules/pty/paste_image.rs
  - src/modules/terminal/ghostty/input/GhosttyInputController.pasteKey.test.ts
  - src/modules/terminal/ghostty/input/GhosttyInputController.ts
  - src/modules/terminal/ghostty/useGhosttyTerminalSession.ts
  - src/modules/terminal/lib/pasteImage.test.ts
---

## Decided
- In a space with a root folder, the file explorer shows the active terminal's folder while it is inside that root, and the root otherwise, because users expect `cd` to move the explorer (it did before spaces had roots) while the root must stay the boundary.
- Only the explorer's view follows; AI, git, file search and a server's authorization keep the space root, because moving them on every cd would re-index and, on SSH, authorize whatever folder the user cd's into (for example /etc).

## Tried and ruled out
- Letting the whole explorer root follow cd everywhere, because the same root feeds RemoteGate's authorization and the AI/git root.
- A VS Code style "reveal" in the tree, because it needs expand-and-highlight plumbing in the explorer for little visible gain.

## Watch out
- Keep RemoteGate on the space root, not the view root, because a changing gate root remounts the explorer with "Opening folder…" on every cd.

In the code: `explorerViewRoot` and `isWithinRoot` in src/modules/tabs/lib/useWorkspaceCwd.ts; App.tsx passes it to FileExplorer's rootPath only.
