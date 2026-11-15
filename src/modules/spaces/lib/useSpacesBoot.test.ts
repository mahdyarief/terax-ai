import { describe, expect, it } from "vitest";
import { reclaimOrphanSpaces, spacesForWindow } from "./useSpacesBoot";

describe("spacesForWindow", () => {
  it("boot only hydrates spaces hosted by this window", () => {
    const spaces = [
      { id: "a", hostWindowLabel: "main" },
      { id: "b", hostWindowLabel: "space-b" },
    ] as any;
    const hosted = spacesForWindow(spaces, "space-b");
    expect(hosted.map((s) => s.id)).toEqual(["b"]);
  });
});

describe("reclaimOrphanSpaces", () => {
  const base = [
    { id: "a", hostWindowLabel: "main", updatedAt: 1 },
    { id: "b", hostWindowLabel: "space-b", updatedAt: 1 },
    { id: "c", hostWindowLabel: "space-c", updatedAt: 1 },
  ] as any;

  it("moves spaces whose window is gone back to main", () => {
    const next = reclaimOrphanSpaces(base, (label) => label === "space-c", 99);
    expect(next.map((s) => s.hostWindowLabel)).toEqual([
      "main",
      "main",
      "space-c",
    ]);
    expect(next[1]).toMatchObject({ id: "b", updatedAt: 99 });
    expect(base[1].hostWindowLabel).toBe("space-b");
  });

  it("returns the same array when nothing is orphaned", () => {
    const next = reclaimOrphanSpaces(
      base,
      (label) => label === "space-b" || label === "space-c",
      99,
    );
    expect(next).toBe(base);
  });
});
