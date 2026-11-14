import { describe, expect, it } from "vitest";
import { gitStatusLabel, truncateMiddle } from "./FileRowHoverCard";

// Pure-logic tests only: no DOM, no jsdom dependency, no async fs. The card's
// rendering is a thin shell over these helpers + existing UI primitives, so
// testing the helpers is the high-value, low-cost coverage.

describe("truncateMiddle", () => {
  it("returns the value unchanged when it fits", () => {
    expect(truncateMiddle("src/main.ts", 44)).toBe("src/main.ts");
  });

  it("returns empty string for non-positive budgets", () => {
    expect(truncateMiddle("anything", 0)).toBe("");
    expect(truncateMiddle("anything", -5)).toBe("");
  });

  it("returns a lone ellipsis when the budget is exactly one char", () => {
    expect(truncateMiddle("long/path.ts", 1)).toBe("\u2026");
  });

  it("keeps both ends visible and marks the middle truncation", () => {
    const result = truncateMiddle("src/modules/explorer/TreeRow.tsx", 20);
    expect(result.length).toBe(20);
    expect(result.startsWith("src/mo")).toBe(true);
    expect(result.endsWith("TreeRow.tsx".slice(-2))).toBe(true);
    expect(result).toContain("\u2026");
  });

  it("never exceeds the requested budget", () => {
    for (const budget of [2, 4, 7, 12, 30]) {
      expect(truncateMiddle("a".repeat(400), budget).length).toBeLessThanOrEqual(
        budget,
      );
    }
  });
});

describe("gitStatusLabel", () => {
  it("maps each code to a human label", () => {
    expect(gitStatusLabel("M")).toBe("Modified");
    expect(gitStatusLabel("A")).toBe("Added");
    expect(gitStatusLabel("D")).toBe("Deleted");
    expect(gitStatusLabel("U")).toBe("Untracked");
    expect(gitStatusLabel("R")).toBe("Renamed");
  });

  it("returns null for absent status", () => {
    expect(gitStatusLabel(null)).toBeNull();
    expect(gitStatusLabel(undefined)).toBeNull();
  });
});
