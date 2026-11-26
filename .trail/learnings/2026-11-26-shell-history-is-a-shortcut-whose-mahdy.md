---
title: Shell history is a shortcut whose default the app lets through to the shell
author: Mahdy
date: 2026-11-26
branch: dydev
cost: { minutes: 11, tokens: 136065 }
id: l-605e6cf1
scope: repo
status: active
src: agent
touches:
  - src/modules/preview/PreviewAddressBar.tsx
  - src/modules/preview/PreviewPane.tsx
  - src/modules/spaces/SpaceSwitcher.test.tsx
  - src/modules/spaces/SpaceSwitcher.tsx
  - ".github/ISSUE_TEMPLATE/bug_report.yml"
  - ".github/ISSUE_TEMPLATE/config.yml"
  - SECURITY.md
  - CHANGELOG.md
  - package.json
  - src-tauri/Cargo.lock
---

## Decided
- Previous/Next command in shell history are app shortcuts (terminal.historyPrev/Next) so a user can give Ctrl+P to Go to file (issue #7) and move history elsewhere.
- Their default is the shell's own Ctrl+P / Ctrl+N on every platform. When the event IS that chord, the host disables the shortcut so the key reaches the terminal untouched, encoded by the terminal as usual (Kitty keyboard protocol included). Only a different, user-chosen chord makes the app write the byte (0x10 / 0x0e) to the focused terminal.
- Settings shows a note when a recorded binding is shared with another shortcut (the first in resolver order owns it: recorded first, then defaults, registry order) or is a plain Ctrl+letter outside the editor.

## Tried and ruled out
- Leaving the history shortcuts unbound by default: Settings would show "Unassigned" next to a key that does work, which reads as broken.
- Always writing the byte from the app, even for Ctrl+P: it would bypass the terminal's own key encoding.

## Watch out
- The default-binding tests forbid control-byte chords; the shell-key shortcuts are exempted there via isShellKeyShortcut. Any new shell-key shortcut needs an entry in SHELL_KEYS.
- Default chords that two shortcuts share on purpose (macOS Cmd+K: clear terminal / palette) must not get a conflict note; notes only fire when a recorded binding is involved.

In the code: src/modules/shortcuts/lib/shellKeys.ts, src/modules/shortcuts/lib/shortcutConflicts.ts, App.tsx shortcutsDisabled.
