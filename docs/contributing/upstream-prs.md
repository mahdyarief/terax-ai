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

## Later

| PR | Why not now |
|---|---|
| [#1320](https://github.com/crynta/terax-ai/pull/1320) | Move live terminals between tabs and splits. A good feature but large (19 files, +1316 lines), touching tabs, panes, agents and the palette. Merge it with care in its own release. |
| [#1226](https://github.com/crynta/terax-ai/pull/1226) | Show the shell name when the OSC title is the user name. Draft upstream. |
| [#1308](https://github.com/crynta/terax-ai/pull/1308) | Alpha channel in `terminal.background`. Draft upstream. |

## Not reviewed yet

These applied to dy cleanly or with a three-way merge on 2026-11-24, but were not looked at in detail: #1506, #1504, #1484, #1478, #1472, #1410, #1371, #1269, #1268, #1265, #1247, #1246.

These did not apply and are older than the move to the Ghostty terminal, so they are probably out of date: #1392, #1376, #1272, #1270, #1252, #1215.

## When upstream merges one of these

The next upstream sync (see [upstream-sync.md](upstream-sync.md)) will bring the same change again, possibly in a later revision.

- If it conflicts with a taken PR's commit, take upstream's version: it is the reviewed, final one.
- Then move the PR on this page from "Taken" to a note that upstream merged it.
