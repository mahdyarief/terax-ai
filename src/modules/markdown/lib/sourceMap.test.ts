import { describe, expect, it } from "vitest";
import { buildAnchors, lineForOffset, offsetForLine } from "./sourceMap";

describe("source line <-> preview offset", () => {
  // 100 source lines, 2000px of preview, headings on lines 11 and 51.
  const anchors = buildAnchors(
    [
      { line: 11, offset: 400 },
      { line: 51, offset: 1000 },
    ],
    100,
    2000,
  );

  it("lands exactly on headings and interpolates between them", () => {
    expect(offsetForLine(anchors, 1)).toBe(0);
    expect(offsetForLine(anchors, 11)).toBe(400);
    expect(offsetForLine(anchors, 31)).toBe(700);
    expect(offsetForLine(anchors, 51)).toBe(1000);
    expect(offsetForLine(anchors, 76)).toBe(1500);
  });

  it("clamps outside the document", () => {
    expect(offsetForLine(anchors, -5)).toBe(0);
    expect(offsetForLine(anchors, 5000)).toBe(2000);
    expect(lineForOffset(anchors, -10)).toBe(1);
    expect(lineForOffset(anchors, 99999)).toBe(101);
  });

  it("maps offsets back to lines", () => {
    expect(lineForOffset(anchors, 400)).toBe(11);
    expect(lineForOffset(anchors, 700)).toBe(31);
    expect(lineForOffset(anchors, 1000)).toBe(51);
  });

  it("works with no headings at all", () => {
    const plain = buildAnchors([], 40, 800);
    expect(offsetForLine(plain, 21)).toBe(400);
    expect(lineForOffset(plain, 400)).toBe(21);
  });

  it("ignores headings that go backwards", () => {
    const messy = buildAnchors(
      [
        { line: 20, offset: 500 },
        { line: 10, offset: 600 },
        { line: 30, offset: 300 },
      ],
      40,
      1000,
    );
    expect(messy.map((anchor) => anchor.line)).toEqual([1, 20, 41]);
  });
});
