# Upstream PRs taken into dy

This fork takes some **open, not yet merged** pull requests from [crynta/terax-ai](https://github.com/crynta/terax-ai/pulls) when they are useful to the dy build. This page records each one reviewed, with its outcome.

The outcomes are:

- **Taken:** the PR's change is in dy. It is one commit per PR, with the original author kept as the commit author. The message says `Taken from upstream PR crynta/terax-ai#<n>`.
- **Already in dy:** dy already does what the PR does, often in a different way. Taking the PR would duplicate it.
- **Not needed:** the problem does not exist in dy.
- **Later:** useful, but not taken yet.

Last reviewed: **2026-11-24**, against `dydev` at 0.9.5-dy, for **0.9.6-dy**. `upstream/main` was fully merged at that point.

## Taken (0.9.6-dy)

| PR | Author | Change |
|---|---|---|
| [#1263](https://github.com/crynta/terax-ai/pull/1263) | mydd7 | PowerShell profile is safe under `Set-StrictMode`. Before, every new terminal threw `VariableIsUndefined` for users with StrictMode in `$PROFILE`. |
| [#1464](https://github.com/crynta/terax-ai/pull/1464) | mydd7 | Explorer keeps drive roots absolute (`C:/`, not `C:`, which means "the current folder on drive C") when it finds a parent folder. Applies to rename, drag and drop, paste and watching. |
| [#1508](https://github.com/crynta/terax-ai/pull/1508) | arsyadal | Explorer: a Rename item in the context menu, and F2. |
| [#1521](https://github.com/crynta/terax-ai/pull/1521) | mydd7 | Terminal keeps the original prompt presentation deadline. |
| [#1520](https://github.com/crynta/terax-ai/pull/1520) | mydd7 | Terminal: double-click selects a word and triple-click a line again. |
| [#1515](https://github.com/crynta/terax-ai/pull/1515) | mydd7 | Terminal: selection autoscroll stays steady while dragging. |
| [#1514](https://github.com/crynta/terax-ai/pull/1514) | mydd7 | Terminal: the WebGL viewport stays fitted to the grid. |
| [#1500](https://github.com/crynta/terax-ai/pull/1500) | mydd7 | Terminal: no bursts of mouse wheel reports to TUI apps. |
| [#1466](https://github.com/crynta/terax-ai/pull/1466) | mydd7 | Tab strip honors the wheel's delta units when scrolling. |
| [#1504](https://github.com/crynta/terax-ai/pull/1504) | AuricX | macOS: Cmd+click opens terminal links while a program tracks the mouse (Claude Code fullscreen), and the program doesn't get that click. Its TERAX.md line was merged by hand. |
| [#1478](https://github.com/crynta/terax-ai/pull/1478) | mydd7 | Windows Ctrl+V: a release of V after Ctrl reports the modifiers it was released with. |
| [#1506](https://github.com/crynta/terax-ai/pull/1506) | mydd7 | Two quick pastes reach the terminal in key press order. **Merged by hand** with dy's image paste: an image's path is delivered in order too. |
| [#1472](https://github.com/crynta/terax-ai/pull/1472) | craraque | Cmd+V (and Ctrl+Shift+V) without text sends an empty bracketed paste, so Claude Code reads an image from the clipboard itself. **Merged by hand** with dy's image paste: with an agent running, the empty paste goes to the agent; otherwise the image is saved and its path pasted. |
| [#1484](https://github.com/crynta/terax-ai/pull/1484) | mydd7 | Copies to the native clipboard run in order, and a paste waits for a copy in progress. **Merged by hand:** the ordering lives in dy's shared `writeClipboardText` (`src/lib/clipboard.ts`), so it covers every copy in the app. |
| [#1265](https://github.com/crynta/terax-ai/pull/1265) | mydd7 | Middle-click closes a tab (on mousedown; `auxclick` never fired). |

## Already in dy

| PR | What it does | Where dy does it |
|---|---|---|
| [#1271](https://github.com/crynta/terax-ai/pull/1271) | cmd.exe reports its folder (OSC 7) so the explorer follows `cd`. | `cmd_prompt` in `src-tauri/src/modules/pty/shell_init.rs` sets `PROMPT` to emit OSC 7. The PR adds a `profile.cmd` with OSC 133 A/B/D too; taking it would install a second prompt hook. Prompt markers for cmd could be added to `cmd_prompt` later if Blocks needs them. |
| [#1413](https://github.com/crynta/terax-ai/pull/1413) | Bash `PROMPT_COMMAND` arrays without a duplicate hook. | `bashrc.bash` already checks every array entry, detects arrays with `${PROMPT_COMMAND@a}` and keeps `_terax_finish_prompt` last. The same script runs on SSH servers through the agent. |
| [#1412](https://github.com/crynta/terax-ai/pull/1412) | Bash cwd reporting under ble.sh. | `blehook PRECMD` registration in `bashrc.bash` (upstream #1417). |
| [#1519](https://github.com/crynta/terax-ai/pull/1519) | Test only: ble.sh keeps reporting the cwd. | The fix is in. The test could still be taken; it adds coverage only. |
| [#1264](https://github.com/crynta/terax-ai/pull/1264) | Answer OSC 10/11 color queries so Claude Code and Codex match the theme. | The Ghostty engine answers them with the theme's colors (`AdaptedGhostty.test.ts`, "answers ... color queries"). The PR targets the xterm-era handlers. |
| [#1306](https://github.com/crynta/terax-ai/pull/1306) | Don't type `cd` into a terminal running a program. | `sendCd` in `src/app/App.tsx` uses `writeAtPrompt`, and shows "Terminal is busy" otherwise. |
| [#1250](https://github.com/crynta/terax-ai/pull/1250) | Explorer follows the active space, not another space's last terminal. | `useWorkspaceCwd`: the explorer view is always within the space root (`explorerViewRoot`). |
| [#1410](https://github.com/crynta/terax-ai/pull/1410) | Shift-only text keys send the shifted character in Kitty keyboard mode. | `buildKeyEvent` in `src/modules/terminal/ghostty/input/keyRouting.ts` gives the base key only with Ctrl, Alt or Super, or when the app asked for every key (upstream #1419). |
| [#1247](https://github.com/crynta/terax-ai/pull/1247) | No native context menu while a TUI tracks the mouse. | `TerminalNativeSelection` cancels `contextmenu` while mouse tracking is on (Shift still opens it). The PR targets xterm.js. |
| [#1268](https://github.com/crynta/terax-ai/pull/1268) | Open `file:` OSC 8 hyperlinks (`ls --hyperlink`, eza). | `terminalLinkTargets.ts` resolves `file:` URLs to local files (`file-uri`) and opens existing ones. |

## Later

| PR | Why not now |
|---|---|
| [#1320](https://github.com/crynta/terax-ai/pull/1320) | Move live terminals between tabs and splits. A good feature but large (19 files, +1316 lines), touching tabs, panes, agents and the palette. Merge it with care in its own release. |
| [#1226](https://github.com/crynta/terax-ai/pull/1226) | Show the shell name when the OSC title is the user name. Draft upstream. |
| [#1308](https://github.com/crynta/terax-ai/pull/1308) | Alpha channel in `terminal.background`. Draft upstream. |
| [#1246](https://github.com/crynta/terax-ai/pull/1246) | Cmd+W in a preview closes the tab, not the window. **Has a flaw:** it closes a tab whenever focus is in the preview's iframe when the window is asked to close. Focus stays there when the window's own close button is clicked, so that button would close a tab instead of the window. It also carries an unrelated redo-shortcut change. Needs a way to tell Cmd+W from the close button, and a Mac to test (WKWebView only). |
| [#1371](https://github.com/crynta/terax-ai/pull/1371), [#1269](https://github.com/crynta/terax-ai/pull/1269) | Nerd Fonts installed on macOS drawn in the terminal (WKWebView canvas does not resolve them by name). The two overlap; #1371 loads the font through `FontFace` with a new Rust command, #1269 only improves auto-detect. macOS only: take one when a Mac can verify it. |

## Not reviewed

These did not apply and are older than the move to the Ghostty terminal, so they are probably out of date: #1392, #1376, #1272, #1270, #1252, #1215.

## When upstream merges one of these

The next upstream sync (see [upstream-sync.md](upstream-sync.md)) will bring the same change again, possibly in a later revision.

- If it conflicts with a taken PR's commit, take upstream's version: it is the reviewed, final one.
- Then move the PR on this page from "Taken" to a note that upstream merged it.
