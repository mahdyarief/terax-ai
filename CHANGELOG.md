# Changelog — Terax dy

This is the changelog of the **dy** build of Terax, a fork of
[crynta/terax-ai](https://github.com/crynta/terax-ai). It lists what the dy
build adds on top of upstream Terax 0.9.0. Versions carry a `-dy` suffix.

Installers are on the
[Releases page](https://github.com/mahdyarief/terax-ai/releases). They are not
code-signed; see the release notes for how to open them.

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
