---
title: "Pane split sizes are saved only from the user's own drag"
author: Mahdy
date: 2026-11-26
branch: dydev
cost: { minutes: 12, tokens: 67821 }
id: l-f0105c97
scope: repo
status: active
src: agent
touches:
  - src/app/App.tsx
  - src/modules/shortcuts/index.ts
  - src/modules/shortcuts/lib/resolveShortcut.test.ts
  - src/modules/shortcuts/lib/shellKeys.test.ts
  - src/modules/shortcuts/lib/shellKeys.ts
  - src/modules/shortcuts/lib/shortcutConflicts.test.ts
  - src/modules/shortcuts/lib/shortcutConflicts.ts
  - src/modules/shortcuts/shortcutDefaults.test.ts
  - src/modules/shortcuts/shortcutDefaults.ts
  - src/modules/shortcuts/shortcuts.ts
---

## Decided
- A split's sizes live on the pane tree (`sizes`, percent per child) and are saved with the space. They are written only from `onLayoutChanged` when `meta.isUserInteraction` is true (pointer release or resize keys), so mount, constraint recompute and setLayout never write.
- Adding or removing a child of a split drops its sizes (back to even); a change deeper in the tree keeps them.
- Double-clicking a separator sets the group even via the group ref and clears the stored sizes.
- setSplitSizes returns the same tree when the sizes already match (0.01 tolerance; within 0.5 % of even counts as even), so the layout callback never re-renders the tabs for nothing.

## Watch out
- Do not give the ResizablePanelGroup a key derived from its panes: a new key remounts every terminal in the split.
- `defaultLayout` is read when the set of panes changes, not on every size update; the group owns the layout after mount.
- The repo has no DOM test environment; PaneTreeView.test.tsx checks the restored sizes via renderToStaticMarkup flex-grow. Drag and double-click need a manual check in Terax Dev.

In the code: src/modules/terminal/lib/panes.ts (setSplitSizes, validSplitSizes), PaneTreeView.tsx PaneSplitView, spaces/lib/serialize.ts.
