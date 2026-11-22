import { describe, expect, it } from "vitest";
import {
  clampSplitRatio,
  planSplitRight,
  reconcileSecondary,
  SPLIT_DEFAULT,
  SPLIT_MAX,
  SPLIT_MIN,
  slotOf,
  slotStyle,
} from "./splitView";
import type { Tab } from "./useTabs";

function term(id: number, spaceId = "a"): Tab {
  return {
    id,
    kind: "terminal",
    spaceId,
    title: "shell",
    paneTree: { kind: "leaf", id: id * 10 },
    activeLeafId: id * 10,
  } as Tab;
}

describe("planSplitRight", () => {
  it("puts another tab beside the active one", () => {
    const tabs = [term(1), term(2), term(3)];
    expect(planSplitRight(tabs, 1, 3)).toEqual({ activeId: 1, secondaryId: 3 });
  });

  it("moves the active tab right and shows its neighbour on the left", () => {
    const tabs = [term(1), term(2), term(3)];
    expect(planSplitRight(tabs, 2, 2)).toEqual({ activeId: 1, secondaryId: 2 });
    expect(planSplitRight(tabs, 1, 1)).toEqual({ activeId: 2, secondaryId: 1 });
  });

  it("refuses when the space has a single tab", () => {
    expect(planSplitRight([term(1), term(2, "b")], 1, 1)).toBeNull();
  });

  it("never pairs tabs from different spaces", () => {
    expect(planSplitRight([term(1), term(2, "b")], 1, 2)).toBeNull();
  });

  it("ignores a tab that no longer exists", () => {
    expect(planSplitRight([term(1)], 1, 9)).toBeNull();
  });
});

describe("reconcileSecondary", () => {
  const tabs = [term(1), term(2), term(3, "b")];

  it("keeps a valid right-hand tab", () => {
    expect(reconcileSecondary(tabs, 1, 2)).toBe(2);
  });

  it("closes the split when the right tab became the active one", () => {
    expect(reconcileSecondary(tabs, 2, 2)).toBeNull();
  });

  it("closes the split when the right tab was closed", () => {
    expect(reconcileSecondary(tabs, 1, 7)).toBeNull();
  });

  it("closes the split when the active tab is in another space", () => {
    expect(reconcileSecondary(tabs, 3, 2)).toBeNull();
  });

  it("leaves an unsplit view alone", () => {
    expect(reconcileSecondary(tabs, 1, null)).toBeNull();
  });
});

describe("slots", () => {
  it("shows the active tab left and the secondary right", () => {
    expect(slotOf(1, 1, 2)).toBe("primary");
    expect(slotOf(2, 1, 2)).toBe("secondary");
    expect(slotOf(3, 1, 2)).toBeNull();
    expect(slotOf(2, 1, null)).toBeNull();
  });

  it("fills the surface unless split", () => {
    expect(slotStyle("primary", false)).toEqual({
      position: "absolute",
      inset: 0,
    });
    expect(slotStyle(null, true)).toEqual({ position: "absolute", inset: 0 });
    expect(slotStyle("primary", true).width).toContain("var(--terax-split");
    expect(slotStyle("secondary", true).left).toContain("var(--terax-split");
  });

  it("clamps the ratio", () => {
    expect(clampSplitRatio(0.05)).toBe(SPLIT_MIN);
    expect(clampSplitRatio(0.95)).toBe(SPLIT_MAX);
    expect(clampSplitRatio(0.4)).toBe(0.4);
    expect(clampSplitRatio(Number.NaN)).toBe(SPLIT_DEFAULT);
  });
});
