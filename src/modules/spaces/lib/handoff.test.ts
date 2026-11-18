import { describe, expect, it } from "vitest";
import {
  isSpaceWindow,
  pickHandoffFallback,
  planSpaceHandoff,
  spaceIdFromWindowLabel,
  spaceWindowLabel,
  visibleSpacesForWindow,
} from "./handoff";

describe("space window labels", () => {
  it("round-trips a space id through its window label", () => {
    const label = spaceWindowLabel("sp-42");
    expect(label).toBe("space-sp-42");
    expect(spaceIdFromWindowLabel(label)).toBe("sp-42");
  });

  it("returns null for the main window and malformed labels", () => {
    expect(spaceIdFromWindowLabel("main")).toBeNull();
    expect(spaceIdFromWindowLabel("space-")).toBeNull();
    expect(spaceIdFromWindowLabel("other")).toBeNull();
  });
});

describe("visibleSpacesForWindow", () => {
  const spaces = [
    { id: "a", hostWindowLabel: "main" },
    { id: "b", hostWindowLabel: "space-b" },
    { id: "c", hostWindowLabel: "main" },
  ];

  it("lists every space in main", () => {
    expect(visibleSpacesForWindow(spaces, "main").map((s) => s.id)).toEqual([
      "a",
      "b",
      "c",
    ]);
  });

  it("lists only its own space in a space window", () => {
    expect(visibleSpacesForWindow(spaces, "space-b").map((s) => s.id)).toEqual([
      "b",
    ]);
    expect(isSpaceWindow("space-b")).toBe(true);
    expect(isSpaceWindow("main")).toBe(false);
  });

  it("picks another space hosted by the same window as the fallback", () => {
    expect(pickHandoffFallback(spaces, "a", "main")).toBe("c");
    expect(pickHandoffFallback(spaces, "b", "space-b")).toBeNull();
  });
});

describe("planSpaceHandoff", () => {
  it("creates a target label from the shared helper", () => {
    const plan = planSpaceHandoff(
      { spaceId: "sp-42", hostWindowLabel: "main" },
      "main",
    );
    expect(plan).toMatchObject({ kind: "handoff", to: "space-sp-42" });
  });
});
