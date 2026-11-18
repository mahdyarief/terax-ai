import { describe, expect, it } from "vitest";
import { findMatches, stepMatch } from "./findInPreview";

describe("findMatches", () => {
  it("finds every occurrence, ignoring case", () => {
    expect(findMatches(["Foo bar foo", "nothing", "FOO"], "foo")).toEqual([
      { node: 0, start: 0, end: 3 },
      { node: 0, start: 8, end: 11 },
      { node: 2, start: 0, end: 3 },
    ]);
  });

  it("does not count overlapping matches twice", () => {
    expect(findMatches(["aaaa"], "aa")).toHaveLength(2);
  });

  it("returns nothing for an empty query and stops at the limit", () => {
    expect(findMatches(["abc"], "")).toEqual([]);
    expect(findMatches(["a".repeat(50)], "a", 10)).toHaveLength(10);
  });
});

describe("stepMatch", () => {
  it("wraps in both directions", () => {
    expect(stepMatch(-1, 3, 1)).toBe(0);
    expect(stepMatch(-1, 3, -1)).toBe(2);
    expect(stepMatch(2, 3, 1)).toBe(0);
    expect(stepMatch(0, 3, -1)).toBe(2);
    expect(stepMatch(0, 0, 1)).toBe(-1);
  });
});
