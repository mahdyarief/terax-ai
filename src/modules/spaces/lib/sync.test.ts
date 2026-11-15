import { describe, expect, it } from "vitest";
import { applyDeletedSpace, mergeSpacesByUpdatedAt } from "./sync";

describe("mergeSpacesByUpdatedAt", () => {
  it("keeps the newer metadata per space id", () => {
    const local = [{ id: "a", name: "Old", updatedAt: 1 }] as any;
    const remote = [{ id: "a", name: "New", updatedAt: 2 }] as any;
    expect(mergeSpacesByUpdatedAt(local, remote)[0].name).toBe("New");
  });

  it("keeps local on an updatedAt tie", () => {
    const local = [{ id: "a", name: "Local", updatedAt: 5 }] as any;
    const remote = [{ id: "a", name: "Remote", updatedAt: 5 }] as any;
    expect(mergeSpacesByUpdatedAt(local, remote)[0].name).toBe("Local");
  });

  it("appends remote-only spaces in remote order", () => {
    const local = [{ id: "a", updatedAt: 1 }] as any;
    const remote = [
      { id: "b", updatedAt: 1 },
      { id: "c", updatedAt: 1 },
    ] as any;
    expect(mergeSpacesByUpdatedAt(local, remote).map((s) => s.id)).toEqual([
      "a",
      "b",
      "c",
    ]);
  });

  it("ignores stale broadcasts that arrive after newer metadata", () => {
    const local = [{ id: "a", name: "New", updatedAt: 9 }] as any;
    const stale = [{ id: "a", name: "Stale", updatedAt: 2 }] as any;
    expect(mergeSpacesByUpdatedAt(local, stale)[0].name).toBe("New");
  });
});

describe("applyDeletedSpace", () => {
  it("removes a deleted space without mutating input", () => {
    const spaces = [{ id: "a" }, { id: "b" }];
    const out = applyDeletedSpace(spaces, "a");
    expect(out.map((s) => s.id)).toEqual(["b"]);
    expect(spaces.map((s) => s.id)).toEqual(["a", "b"]);
  });
});
