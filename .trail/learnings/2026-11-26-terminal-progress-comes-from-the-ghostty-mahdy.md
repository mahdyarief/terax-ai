---
title: "Terminal progress comes from the Ghostty core, not a scan of the output"
author: Mahdy
date: 2026-11-26
branch: dydev
cost: { minutes: 24, tokens: 96581 }
id: l-572d6062
scope: repo
status: active
src: agent
touches:
  - src/modules/terminal/lib/terminalProgress.ts
  - src/modules/terminal/lib/terminalProgress.test.ts
  - packages/ghostty-core/adapted/wasm/src/terax.zig
---

## Decided
- OSC 9;4 progress (issue #5) is forwarded by the adapted Ghostty core as semantic event 13 (two bytes: state, percent or 255). Ghostty's parser already recognises it (`conemu_progress_report` -> stream action `progress_report`); terax.zig used to drop it in the `else` branch.
- The router clears a tab's progress at the next prompt or command end, so a program that exits or crashes mid progress does not leave a stuck bar.

## Tried and ruled out
- Scanning PTY output in TypeScript for ESC ] 9;4: it puts a per-chunk scan on the terminal's hottest path and duplicates a parser the core already has.

## Watch out
- Rebuilding the cores needs Zig 0.16.0. The unchanged source built on Windows was not byte-identical to the artifact built before (4 bytes in the code section), so verify a rebuild with the core tests and `pnpm soak:ghostty`, and update the SHA-256 constants in TeraxGhostty.ts and UPSTREAM.md (AdaptedGhostty.test.ts checks them).
- A plain OSC 9 (iTerm-style notification) must keep arriving as a notification; AdaptedGhostty.test.ts covers both.

In the code: packages/ghostty-core/adapted/wasm/src/terax.zig (.progress_report), lib/adapted/TeraxGhostty.ts decode case 13, src/modules/terminal/lib/terminalProgress.ts, TabBar.tsx TerminalTabProgress.
