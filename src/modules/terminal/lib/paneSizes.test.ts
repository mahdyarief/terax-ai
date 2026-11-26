import { describe, expect, it } from "vitest";
import {
  type PaneNode,
  removeLeaf,
  setSplitSizes,
  splitLeaf,
  validSplitSizes,
} from "@/modules/terminal/lib/panes";

const row = (sizes?: number[]): PaneNode => ({
  kind: "split",
  id: 100,
  dir: "row",
  children: [
    { kind: "leaf", id: 1 },
    { kind: "leaf", id: 2 },
    { kind: "leaf", id: 3 },
  ],
  ...(sizes && { sizes }),
});

const sizesOf = (n: PaneNode) => (n.kind === "split" ? n.sizes : undefined);

describe("split sizes", () => {
  it("records the sizes the user dragged to, rounded", () => {
    const tree = setSplitSizes(row(), 100, [50.123, 24.877, 25]);
    expect(sizesOf(tree)).toEqual([50.12, 24.88, 25]);
  });

  it("finds a split nested in the tree", () => {
    const tree: PaneNode = {
      kind: "split",
      id: 200,
      dir: "col",
      children: [row(), { kind: "leaf", id: 9 }],
    };
    const next = setSplitSizes(tree, 100, [60, 20, 20]);
    expect(next.kind === "split" && sizesOf(next.children[0])).toEqual([
      60, 20, 20,
    ]);
    expect(sizesOf(next)).toBeUndefined();
  });

  it("goes back to even for undefined or sizes that are even anyway", () => {
    const resized = row([60, 20, 20]);
    expect(sizesOf(setSplitSizes(resized, 100, undefined))).toBeUndefined();
    expect(
      sizesOf(setSplitSizes(resized, 100, [33.4, 33.3, 33.3])),
    ).toBeUndefined();
  });

  it("returns the same tree when nothing changes, so no re-render follows", () => {
    const even = row();
    expect(setSplitSizes(even, 100, undefined)).toBe(even);
    expect(setSplitSizes(even, 100, [33.33, 33.33, 33.34])).toBe(even);
    const resized = row([60, 20, 20]);
    expect(setSplitSizes(resized, 100, [60.001, 20, 19.999])).toBe(resized);
    expect(setSplitSizes(resized, 999, [10, 10, 80])).toBe(resized);
  });

  it("ignores sizes that do not describe the split", () => {
    const even = row();
    expect(setSplitSizes(even, 100, [50, 50])).toBe(even);
    expect(setSplitSizes(even, 100, [90, 20, -10])).toBe(even);
    expect(setSplitSizes(even, 100, [10, 10, 10])).toBe(even);
  });

  it("drops the sizes when a pane is added to or removed from the split", () => {
    const resized = row([60, 20, 20]);
    expect(sizesOf(splitLeaf(resized, 2, 500, 4, "row"))).toBeUndefined();
    const removed = removeLeaf(resized, 3);
    expect(removed && sizesOf(removed)).toBeUndefined();
  });

  it("keeps them when a pane changes deeper in the tree", () => {
    const tree: PaneNode = {
      kind: "split",
      id: 200,
      dir: "col",
      children: [row(), { kind: "leaf", id: 9 }],
      sizes: [70, 30],
    };
    const split = splitLeaf(tree, 1, 500, 4, "col");
    expect(sizesOf(split)).toEqual([70, 30]);
    const removed = removeLeaf(tree, 1);
    expect(removed && sizesOf(removed)).toEqual([70, 30]);
  });

  it("validates sizes read from disk", () => {
    expect(validSplitSizes([70, 30], 2)).toBe(true);
    expect(validSplitSizes([70, 30], 3)).toBe(false);
    expect(validSplitSizes([100], 1)).toBe(false);
    expect(validSplitSizes(["70", 30], 2)).toBe(false);
    expect(validSplitSizes([70, Number.NaN], 2)).toBe(false);
    expect(validSplitSizes("70,30", 2)).toBe(false);
  });
});
