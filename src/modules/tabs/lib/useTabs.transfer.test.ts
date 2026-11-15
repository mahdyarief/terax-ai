import { describe, expect, it } from "vitest";
import { attachTabsBySpace, detachTabsBySpace } from "./useTabs";

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
