---
title: "Markdown preview: plugins need a remount, links need their written address, and the editor tab keeps its editor"
author: Mahdy
date: 2026-11-18
branch: dydev
cost: { minutes: 60, tokens: 282700 }
id: l-6b53f8f7
scope: repo
status: active
src: agent
touches:
  - src/app/hover-only-controls.test.ts
  - src/lib/touchHold.test.ts
  - src/lib/touchHold.ts
  - src/modules/ai/components/Chip.tsx
  - src/modules/spaces/SpaceSwitcher.tsx
  - src/modules/tabs/TabBar.tsx
  - src/settings/sections/ModelsSection.tsx
  - src/modules/ai/components/AiMiniWindow.tsx
  - src/modules/ai/components/PlanDiffReview.tsx
  - src/modules/source-control/SourceControlPanel.tsx
---

## Decided
- A markdown editor tab never turns back into a read-only preview tab. Raw, Split and Rendered are layouts of the same editor tab, and the editor stays mounted (hidden in Rendered), because swapping the tab kind unmounted it and dropped undo history, the cursor and unsaved text. The preview there renders the live buffer, so unsaved edits are visible.
- Streamdown is remounted with a `key` when the math parser loads. Its memo comparator ignores `remarkPlugins`, `rehypePlugins` and `components`, so a plugin that arrives after the first render never takes effect otherwise.
- Link clicks read the address from `data-href`, copied before the hardening pass. That pass resolves relative addresses against a dummy origin, so `./a.md` and `../a.md` both reach the component as `/a.md`.
- `$x$` is read as math only when the document clearly has TeX inside single dollars, and loose dollars are escaped first, so "$5 and $10" stays text.
- Mermaid's older pins (katex, marked, d3 through d3-sankey, cose-base through cose-bilkent) are unified with pnpm overrides, because the build fails on any package bundled in two versions.

## Tried and ruled out
- Passing new plugin arrays to Streamdown and expecting a re-render: nothing happens until the text changes.
- `rehype-katex`: it pins an older katex, which would ship twice. Math is typeset in the `code` renderer from a `data-math` mark instead.
- Classifying links from the `href` the component receives: relative links all looked absolute and were blocked.

## Watch out
- Heading ids are `heading-line-N`, not slugs; `#fragment` links are matched through GitHub-style slugs computed from the outline.
- Find-in-preview matches per text node, so a phrase split by inline formatting is not found as one match.
- Headless Edge (`--dump-dom --virtual-time-budget`) can render the preview from the Vite dev server for a check, but `requestAnimationFrame` is unreliable there.

In the code:
- `src/modules/markdown/MarkdownPreviewPane.tsx` (`MarkdownDocumentView`, `PreviewLink`, `PreviewCode`), `lib/links.ts` (`rehypeKeepHref`, `classifyLink`), `lib/math.ts`, `lib/sourceMap.ts`
- `src/modules/tabs/lib/useTabs.ts` `setMarkdownView`, `EditorTab.markdownView`; `src/modules/editor/EditorPane.tsx` `mdMode`
- `pnpm-workspace.yaml` overrides; `src/app/dist-budget.test.ts` is the duplicate-package guard (takes about 2 minutes)
