import { describe, expect, it } from "vitest";
import {
  attachTabsBySpace,
  detachTabsBySpace,
  detachTabsForHandoff,
  planHandoffDetach,
} from "./useTabs";

describe("detachTabsBySpace", () => {
  it("detaches all tabs for a space and keeps remaining", () => {
    const tabs = [
      { id: 1, spaceId: "a" },
      { id: 2, spaceId: "b" },
    ] as any;
    const { detached, remaining } = detachTabsBySpace(tabs, "a");
    expect(detached.map((t: any) => t.id)).toEqual([1]);
    expect(remaining.map((t: any) => t.id)).toEqual([2]);
  });

  it("returns empty detached when space has no tabs", () => {
    const tabs = [{ id: 9, spaceId: "z" }] as any;
    const { detached, remaining } = detachTabsBySpace(tabs, "a");
    expect(detached).toHaveLength(0);
    expect(remaining).toHaveLength(1);
  });
});

describe("detachTabsForHandoff", () => {
  it("collects live PTY ids and detaches views without disposing them", () => {
    const detached: number[] = [];
    const tabs = [
      {
        id: 1,
        kind: "terminal",
        spaceId: "a",
        paneTree: {
          kind: "split",
          children: [
            { kind: "leaf", id: 11 },
            { kind: "leaf", id: 12 },
          ],
        },
      },
      { id: 2, kind: "editor", spaceId: "a" },
    ] as any;

    const { ptyIds, leafPtyIds } = detachTabsForHandoff(tabs, "a", {
      ptyIdForLeaf: (leafId) => (leafId === 11 ? 41 : null),
      detachGhosttyView: (leafId) => {
        detached.push(leafId);
        return true;
      },
    });

    expect(ptyIds).toEqual([41]);
    expect(leafPtyIds).toEqual([41, null]);
    expect(detached).toEqual([11, 12]);
  });

  it("skips private terminal tabs so leaf order matches serialization", () => {
    const detached: number[] = [];
    const tabs = [
      {
        id: 1,
        kind: "terminal",
        spaceId: "a",
        private: true,
        paneTree: { kind: "leaf", id: 11 },
      },
      {
        id: 2,
        kind: "terminal",
        spaceId: "a",
        paneTree: { kind: "leaf", id: 12 },
      },
    ] as any;

    const { ptyIds, leafPtyIds } = detachTabsForHandoff(tabs, "a", {
      ptyIdForLeaf: (leafId) => (leafId === 11 ? 41 : 42),
      detachGhosttyView: (leafId) => {
        detached.push(leafId);
        return true;
      },
    });

    expect(ptyIds).toEqual([42]);
    expect(leafPtyIds).toEqual([42]);
    expect(detached).toEqual([12]);
  });

  it("only collects PTYs from the requested tab ids", () => {
    const detached: number[] = [];
    const tabs = [
      {
        id: 1,
        kind: "terminal",
        spaceId: "a",
        paneTree: { kind: "leaf", id: 11 },
      },
      {
        id: 2,
        kind: "terminal",
        spaceId: "a",
        paneTree: { kind: "leaf", id: 12 },
      },
    ] as any;

    const { ptyIds, leafPtyIds } = detachTabsForHandoff(tabs, "a", {
      ptyIdForLeaf: (leafId) => (leafId === 12 ? 42 : 41),
      detachGhosttyView: (leafId) => {
        detached.push(leafId);
        return true;
      },
      tabIds: [2],
    });

    expect(ptyIds).toEqual([42]);
    expect(leafPtyIds).toEqual([42]);
    expect(detached).toEqual([12]);
  });

  it("leaves other spaces untouched", () => {
    const detached: number[] = [];
    const tabs = [
      { id: 1, kind: "terminal", spaceId: "b", paneTree: { kind: "leaf", id: 9 } },
    ] as any;

    const { ptyIds, leafPtyIds } = detachTabsForHandoff(tabs, "a", {
      ptyIdForLeaf: () => 7,
      detachGhosttyView: (leafId) => {
        detached.push(leafId);
        return true;
      },
    });

    expect(ptyIds).toEqual([]);
    expect(leafPtyIds).toEqual([]);
    expect(detached).toEqual([]);
  });
});

describe("planHandoffDetach", () => {
  const tabs = [
    { id: 1, kind: "terminal", spaceId: "a" },
    { id: 2, kind: "ai-diff", spaceId: "a" },
    { id: 3, kind: "editor", spaceId: "b" },
    { id: 4, kind: "editor", spaceId: "a" },
  ] as any;
  const isMovable = (t: any) => t.kind !== "ai-diff";

  it("moves movable tabs and re-homes the rest onto the fallback space", () => {
    const { detached, kept, remaining } = planHandoffDetach(tabs, "a", {
      isMovable,
      fallbackSpaceId: "b",
    });
    expect(detached.map((t: any) => t.id)).toEqual([1, 4]);
    expect(kept).toEqual([{ id: 2, kind: "ai-diff", spaceId: "b" }]);
    expect(remaining.map((t: any) => [t.id, t.spaceId])).toEqual([
      [2, "b"],
      [3, "b"],
    ]);
  });

  it("reports unmovable tabs without touching them when there is no fallback", () => {
    const { kept, remaining } = planHandoffDetach(tabs, "a", {
      isMovable,
      fallbackSpaceId: null,
    });
    expect(kept.map((t: any) => [t.id, t.spaceId])).toEqual([[2, "a"]]);
    expect(remaining.map((t: any) => t.id)).toEqual([2, 3]);
  });

  it("honours a tab id filter", () => {
    const { detached, remaining } = planHandoffDetach(tabs, "a", {
      isMovable,
      fallbackSpaceId: "b",
      tabIds: [4],
    });
    expect(detached.map((t: any) => t.id)).toEqual([4]);
    expect(remaining.map((t: any) => t.id)).toEqual([1, 2, 3]);
  });
});

describe("attachTabsBySpace", () => {
  it("appends tabs for the space and returns the resulting active id", () => {
    const current = [{ id: 1, spaceId: "main" }] as any;
    const incoming = [{ id: 2, spaceId: "a" }] as any;
    const result = attachTabsBySpace(current, incoming, 1, null);
    expect(result.tabs.map((t: any) => t.id)).toEqual([1, 2]);
    expect(result.activeId).toBe(1);
  });

  it("uses the requested active id when the current active tab is absent", () => {
    const current = [{ id: 1, spaceId: "main" }] as any;
    const incoming = [{ id: 2, spaceId: "a" }] as any;
    const result = attachTabsBySpace(current, incoming, 99, 2);
    expect(result.activeId).toBe(2);
  });
});
