import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import type { PaneNode } from "./lib/panes";

vi.mock("./TerminalPane", () => ({
  TerminalPane: ({ leafId }: { leafId: number }) =>
    createElement("div", { "data-test-leaf": leafId }),
}));

const { PaneTreeView } = await import("./PaneTreeView");

const render = (node: PaneNode) =>
  renderToStaticMarkup(
    createElement(PaneTreeView, {
      node,
      tabVisible: true,
      activeLeafId: 1,
      blocks: false,
      onFocusLeaf: () => {},
      onSplitSizes: () => {},
      getBundle: () => ({
        setRef: () => {},
        onSearchReady: () => {},
        onCwd: () => {},
        onExit: () => {},
      }),
    }),
  );

const split = (sizes?: number[]): PaneNode => ({
  kind: "split",
  id: 10,
  dir: "row",
  children: [
    { kind: "leaf", id: 1 },
    { kind: "leaf", id: 2 },
  ],
  ...(sizes && { sizes }),
});

/** The flex-grow of each panel, in order. */
const grows = (html: string) =>
  [...html.matchAll(/data-panel[^>]*flex-grow:\s*([\d.]+)/g)].map((m) =>
    Number(m[1]),
  );

describe("PaneTreeView split sizes", () => {
  it("opens a resized split at the sizes it was left at", () => {
    const html = render(split([70, 30]));
    expect(html).toContain('data-test-leaf="1"');
    expect(html).toContain('data-test-leaf="2"');
    const [a, b] = grows(html);
    expect(a / b).toBeCloseTo(70 / 30, 1);
  });

  it("opens an untouched split even", () => {
    const [a, b] = grows(render(split()));
    expect(a).toBeCloseTo(b, 5);
  });

  it("tells the user a double-click makes the panes even", () => {
    expect(render(split())).toContain(
      'title="Drag to resize, double-click to make even"',
    );
  });
});
