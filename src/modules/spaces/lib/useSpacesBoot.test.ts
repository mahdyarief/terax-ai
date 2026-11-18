import { describe, expect, it } from "vitest";
import { reclaimOrphanSpaces, resolveBootTargets, spacesForWindow } from "./useSpacesBoot";

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

describe("resolveBootTargets", () => {
  const spaces = [
    { id: "a", hostWindowLabel: "main" },
    { id: "b", hostWindowLabel: "main" },
  ] as any;

  it("claims the space named in a new window's label before the host is published", () => {
    const result = resolveBootTargets(spaces, "space-b");
    expect(result.hosted.map((s) => s.id)).toEqual(["b"]);
    expect(result.awaitingHandoff).toBe(true);
  });

  it("uses persisted hosts once the handoff has been published", () => {
    const result = resolveBootTargets(
      [{ id: "a", hostWindowLabel: "main" }, { id: "b", hostWindowLabel: "space-b" }] as any,
      "space-b",
    );
    expect(result.hosted.map((s) => s.id)).toEqual(["b"]);
    expect(result.awaitingHandoff).toBe(false);
  });

  it("stays empty when a space window's id is unknown", () => {
    const result = resolveBootTargets(spaces, "space-missing");
    expect(result.hosted).toEqual([]);
    expect(result.awaitingHandoff).toBe(false);
  });

  it("never claims a space from the main window", () => {
    const result = resolveBootTargets(spaces, "main");
    expect(result.hosted.map((s) => s.id)).toEqual(["a", "b"]);
    expect(result.awaitingHandoff).toBe(false);
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
