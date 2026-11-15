import { describe, expect, it } from "vitest";
import { getActiveSpaceIdForWindow, migrateSpaceMeta } from "./store";

describe("migrateSpaceMeta", () => {
  it("defaults missing hostWindowLabel to main", () => {
    const migrated = migrateSpaceMeta({
      id: "default",
      name: "Default",
      root: null,
      env: "host",
      createdAt: 1,
      updatedAt: 1,
    } as any);
    expect(migrated.hostWindowLabel).toBe("main");
  });

  it("preserves explicit hostWindowLabel", () => {
    const migrated = migrateSpaceMeta({
      id: "s1",
      name: "Space 1",
      root: null,
      env: "host",
      createdAt: 1,
      updatedAt: 1,
      hostWindowLabel: "space-s1",
    } as any);
    expect(migrated.hostWindowLabel).toBe("space-s1");
  });
});

describe("getActiveSpaceIdForWindow", () => {
  it("reads active space per window with fallback", () => {
    const map = { main: "default", "space-a": "a" };
    expect(getActiveSpaceIdForWindow(map, "space-a")).toBe("a");
    expect(getActiveSpaceIdForWindow(map, "space-missing")).toBeNull();
    expect(getActiveSpaceIdForWindow(null, "main")).toBeNull();
  });
});
