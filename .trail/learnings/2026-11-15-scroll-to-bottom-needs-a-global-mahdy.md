---
title: "Scroll-to-bottom needs a global shortcut, not just a button"
author: Mahdy
date: 2026-11-15
branch: dydev
id: l-8c64ff7e
scope: repo
status: active
src: agent
touches:
  - src/app/App.tsx
  - src/modules/editor/ProblemsPanel.tsx
  - src/modules/explorer/FileExplorer.tsx
  - src/modules/git-history/GitHistoryPane.tsx
  - src/modules/shortcuts/shortcutDefaults.test.ts
  - src/modules/shortcuts/shortcutDefaults.ts
  - src/modules/shortcuts/shortcuts.ts
  - src/modules/source-control/SourceControlPanel.tsx
  - src/modules/terminal/TerminalPane.tsx
  - src/modules/terminal/ghostty/useGhosttyTerminalSession.ts
---

## Decided

- The mouse-free path for "jump terminal to newest output" is a global shortcut
  (`terminal.scrollToBottom`), not just the chevron-down button. `Tab` inside
  the Ghostty terminal is encoded to the PTY and consumed there, so focus never
  reaches the button; a real `<button>` alone is unreachable by keyboard from
  terminal focus.
- Windows/Linux binds `Ctrl+Alt+End`; macOS binds `Shift+Cmd+ArrowDown`.

## Tried and ruled out

- `Ctrl+Shift+End` / `Ctrl+Shift+J` / `Ctrl+Shift+K` / `Ctrl+Shift+G` all
  collided: CodeMirror `Mod-*` keymaps, the existing `ai.askSelection` chord,
  and TUI/readline control bytes. `shortcutDefaults.test.ts` fails on each.
- `Ctrl+Alt+<single character>` is banned as AltGr on many layouts, but
  `Ctrl+Alt+End` is a named key rather than a character, so it passes.

## Watch out

- Gate the shortcut on focus the way `terminal.clear` is gated: intercept only
  while a terminal surface is focused, and never `preventDefault` elsewhere.
