# Changelog — Terax dy

This is the changelog of the **dy** build of Terax, a fork of
[crynta/terax-ai](https://github.com/crynta/terax-ai). It lists what the dy
build adds on top of upstream Terax 0.9.0. Versions carry a `-dy` suffix.

Installers are on the
[Releases page](https://github.com/mahdyarief/terax-ai/releases). They are not
code-signed; see the release notes for how to open them.

## Unreleased

### From issues

- Terminal tabs show a program's OSC 9;4 progress (winget, PowerShell 7.5
  and others) as a thin bar along the tab: the share done, a sweeping
  segment when there is no number, red on error and amber while paused
  (#5).
- The status bar shows the repository's branch, `*` for changes and `↑`/`↓`
  for commits to push and pull. Clicking it switches to another branch or
  creates one. Settings > General turns it off (#9).
- Split panes keep the sizes they were dragged to when a space is restored,
  and a double-click on the line between them makes them even (#8).
- Shell history (Ctrl+P / Ctrl+N) is a shortcut you can move, so Go to file
  can take Ctrl+P. Settings > Shortcuts notes a key two shortcuts share, or
  a recorded Ctrl+letter that terminals then may not receive (#7).

### From pull requests

- The spaces switcher has a search box, and a preview the site refuses to
  frame has an "Open in browser" button (#4 by rayss868).

## 0.9.6-dy — 2026-11-24

### From upstream pull requests

Open pull requests from upstream Terax, taken ahead of their merge. See
[docs/contributing/upstream-prs.md](docs/contributing/upstream-prs.md) for
the full review, including those dy already covers.

- Explorer: Rename in the context menu, and F2 (#1508).
- Explorer: a file at a drive's root (`C:/`) is renamed, moved, pasted and
  watched in that root, not in the drive's current folder (#1464).
- Terminal: double-click selects a word and triple-click a line again
  (#1520); selection autoscroll stays steady while dragging (#1515); the
  WebGL view stays fitted to the grid (#1514); TUI apps no longer get bursts
  of mouse wheel reports (#1500); the prompt keeps its original presentation
  deadline (#1521).
- PowerShell terminals no longer fail to start their integration for users
  with `Set-StrictMode` in their profile (#1263).
- The tab strip scrolls by the wheel's real distance (#1466).
- Pasting: two quick pastes arrive in the order they were made, an image's
  path included (#1506); a paste waits for a copy still in progress, and
  quick copies land in order (#1484); with Claude Code or another agent
  running, Cmd+V or Ctrl+Shift+V of an image sends the empty paste the agent
  reads the image on (#1472); Ctrl released before V is reported correctly
  (#1478).
- macOS: Cmd+click opens terminal links while a program tracks the mouse,
  such as Claude Code in fullscreen (#1504).
- Middle-click closes a tab (#1265).

## 0.9.5-dy — 2026-11-24

### SSH remote workspaces

- **Connect to SSH Host...** from the command palette or the new SSH menu in
  the status bar opens a folder on a server as a space, like VS Code's
  Remote-SSH. Hosts come from `~/.ssh/config` and recent connections.
- Terax installs a small agent on the server the first time (static Linux
  builds for x86_64 and arm64 ship with the installer) and reinstalls it
  when the app's version changes.
- In a server's space, everything works on the server: the file explorer,
  the editor, file operations, find in files and the file finder, source
  control with live status, terminals, and the AI's file and shell tools,
  including background processes. Secrets on the server stay hidden from the
  AI as they do locally.
- ssh's own questions (passwords, passphrases, host keys) are asked in a
  Terax dialog.
- Connections are kept alive and reconnect on their own. A terminal that
  dropped with its server reopens in the same folder when the server is back,
  and the explorer shows the state, the reason and Connect while it is away.
- The SSH menu lists every connection with its state, Reconnect, Disconnect
  and Show log. The space switcher and the tab strip name a server's host.

### Terminal

- Paste an image: an image on the clipboard is saved as a PNG in a
  `terax-image` folder and its path is pasted (on a server it is uploaded to
  `~/.cache/terax-image`). Ctrl+V still hands the image to a running AI agent
  such as Claude Code, which attaches it itself.
- Open a tab with any installed shell: hover Terminal in the "+" menu.
  Windows finds PowerShell, Windows PowerShell, Command Prompt, Git Bash
  (also from Scoop or a custom folder), MSYS2, Cygwin and Nushell. The default
  stays the one in Settings.

### Interface

- The SSH log, connection errors, terminal errors, toasts and the askpass
  prompt can be selected and copied, with Copy buttons where they help.
- Everything in the status bar, its menus included, uses one text and icon
  size; long menus scroll with a visible scrollbar.
- The new tab menu no longer cuts its shortcuts.

### Fixes

- In a space with a folder, the file explorer follows the terminal's `cd`
  again while it stays inside that folder.

## 0.9.4-dy — 2026-11-23

### Tabs and editor

- **Split view**: a tab's context menu has "Split right". The tab shows on the
  right while the active tab stays on the left; drag the divider to resize
  (double-click resets), and "Close split view" returns to one pane.
- **Open to the Side**: a file's context menu in the explorer opens it on the
  right of a split view, so two files (say a .docx and an .xlsx) sit side by
  side.
- `.mmd` and `.mermaid` files open as rendered diagrams, with the same Raw,
  Split and Rendered views as Markdown. A broken diagram keeps the last good
  render under an error banner.
- Text files too large to open show their start and load more as you scroll.

### Workspace

- Open or change the workspace folder from the File Explorer, the space
  switcher and the command palette, with a native folder picker. New terminals
  and spaces start in the workspace folder.

### Fixes

- Word documents that contain line breaks open again.
- The space list shows its scrollbar.
- The diagram error banner no longer sits on its own layer over the preview.

### From upstream

- Markdown preview renders links and local images.
- New AI models in the model list (Gemini 3.5 Flash-Lite, Gemini 4 Hydrogen)
  and deprecated ones removed.

## 0.9.3-dy — 2026-11-18

### Markdown

- Markdown files get three views: **Rendered**, **Split** and **Raw**. The
  preview follows what you type, including changes you have not saved, and in
  Split it scrolls with the editor.
- Switching views keeps undo history, the cursor, unsaved text and your place
  in the document.
- Links work in the preview: `#heading` links jump to the heading, and relative
  links open the file in a tab.
- Mermaid diagrams and TeX math are rendered.
- YAML frontmatter is shown as a small table.
- Find in the rendered view with Ctrl/Cmd+F.
- In the editor, Ctrl+B and Ctrl+I toggle bold and italic, and pasting a URL
  over selected text makes a link.
- The outline remembers whether you hid it.

### File previews

- SVG files open as an image, with a Rendered / Source switch.
- Text in DOCX, XLSX and CSV previews can be selected and copied.
- DOCX and XLSX previews reload when the file changes on disk and keep the
  selected sheet.
- A text file too large to open (a 70 MB JSON, a long log) shows its first
  256 KB read-only instead of only "File too large", with a "Reveal in folder"
  button. Large CSV files show their first rows as a table.
- The HTML preview keeps its scroll position while you edit. Scripts in a page
  that pulls in local files stay off until you trust the folder.

### Spaces and windows

- Opening a space in its own window now carries its tabs and running terminals
  with it. Before, the tabs could come back as fresh shells or disappear.
- Opening the same space in a window a second time works.
- Tabs that cannot move (unsaved editors, private terminals) stay in the main
  window, and a message says so.
- A space window shows only its own space.

### Touch and keyboard

- Hover styles and hover-only buttons work on touchscreen laptops.
- Tabs and spaces can be dragged by touch (press and hold).
- Delete closes the focused tab.

## 0.9.2-dy — 2026-11-16

### Spaces and windows

- Tabs move between windows with their terminal still running: the shell, its
  output and its scrollback carry over instead of restarting.
- A space window can no longer open another copy of itself, and opening a
  space that already has a window focuses that window.

### Fixes

- Terminal input and resizes are accepted only from the window that currently
  shows the terminal.

## 0.9.1-dy — 2026-11-15

### Spaces and windows

- A space can be opened in its own window. All windows stay in sync: renaming,
  recoloring, reordering or deleting a space shows up everywhere.
- "Bring back to this window" returns a space to the main window, and tabs can
  be moved across windows.
- Space windows left over from a crash are reclaimed on the next start.

### Editor and previews

- HTML files have a sandboxed preview, and binary files show a summary.
- A Problems panel lists diagnostics, with counts in the status bar.
- The Markdown preview has an outline.

### Interface

- Sidebar panels and the terminal have visible scrollbars.
- A button and a shortcut jump the terminal to its newest output.
- The Files / Git / Problems rail collapses to icons when the sidebar is
  narrow.

### AI

- Gemini 3.5 Flash-Lite and Gemini 4 Hydrogen are in the model list.

### Fixes

- The command palette searches content and recent files in the current
  workspace only.
- The Markdown preview no longer misses a change made right after it opens.

## 0.9.0-dy — 2026-11-14

First dy build, based on upstream Terax 0.9.0.

- CSV, DOCX and XLSX files, and images, can be previewed.
- Ctrl+C copies when text is selected in the terminal, and still interrupts
  the running program when nothing is selected.
- The terminal resizes right away after its window moves to another monitor.
