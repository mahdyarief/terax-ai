---
title: Split view lays two tabs out inside one tree; a second subtree would remount and kill the shell
author: Mahdy
date: 2026-11-22
branch: dydev
cost: { minutes: 47, tokens: 191835 }
id: l-3b2ec6f6
scope: repo
status: active
src: agent
touches:
  - src/lib/utils.test.ts
  - src/lib/utils.ts
  - src/modules/editor/EditorPane.tsx
  - src/modules/editor/EditorStack.tsx
  - src/modules/markdown/MermaidBlock.tsx
  - src/modules/markdown/MermaidPreview.tsx
  - src/modules/markdown/lib/mermaidPreview.test.ts
  - src/modules/markdown/lib/mermaidPreview.ts
  - src/modules/tabs/lib/useTabs.ts
  - src/modules/spaces/SpaceSwitcher.test.tsx
---

## Decided
- Tabs side by side keep every tab in the one `SurfaceLayers` tree. Each stack places its visible tabs in a left (`primary` = active tab) or right (`secondary`) slot via `slotOf`/`slotStyle`; the ratio is the `--terax-split` CSS variable on the surface, so dragging the divider restyles without re-rendering the seven stacks.
- Layers are `pointer-events-none`; only the shown tab wrappers opt back in, because in a split two full-size layers overlap.
- Picking the right-hand tab in the tab bar focuses its pane instead of making it active; the left side stays `activeId`, which shortcuts, title and AI context keep using.

## Tried and ruled out
- The earlier secondary pane rendered a second `SurfaceLayers` subtree. Moving an existing tab into it remounts the tab: a terminal's pane unmount calls `pty_close` (shell dies), an editor drops unsaved text and undo, and a second `data-terminal-area` breaks the one-canvas-per-window WebGPU design.

## Watch out
- WebGPU terminal panes re-measure on size changes (ResizeObserver); a pure move with no size change is only caught by the ~1 s `verifyGeometry` safety net. That is why there is no "swap sides" action.
- `backdrop-blur` (any backdrop filter) fails `compositingBudget.test.ts`; run the full `pnpm test`, not only the module's tests.

In the code:
- `src/modules/tabs/lib/splitView.ts` (`planSplitRight`, `reconcileSecondary`), `src/modules/tabs/lib/splitLayout.ts`, `src/app/components/WorkspaceSurface.tsx` (`SplitDivider`), stacks' `secondaryId` prop, `App.tsx` `selectTab` / `splitTabRight`
