import { describe, expect, it } from "vitest";
import {
  buildOutlineModel,
  formatDepthLabel,
  headingKeys,
  minHeadingLevel,
  parseHeadings,
} from "./useMarkdownHeadings";

describe("parseHeadings", () => {
  it("reads ATX levels, stripping closing hashes", () => {
    const headings = parseHeadings("# One\n### Three ###\n###### Six");
    expect(headings.map((h) => [h.level, h.text])).toEqual([
      [1, "One"],
      [3, "Three"],
      [6, "Six"],
    ]);
  });

  it("ids point at the heading's own source line", () => {
    const headings = parseHeadings("intro\n\n## Setup\n\n## Usage");
    expect(headings.map((h) => h.id)).toEqual(["heading-line-3", "heading-line-5"]);
  });

  it("ignores hashes inside fenced code", () => {
    const source = [
      "# Real",
      "```bash",
      "# not a heading",
      "```",
      "~~~",
      "## also not",
      "~~~",
      "## Back",
    ].join("\n");
    expect(parseHeadings(source).map((h) => h.text)).toEqual(["Real", "Back"]);
  });

  it("does not close a backtick fence on a tilde run", () => {
    const source = ["```", "~~~", "# fenced", "```", "# after"].join("\n");
    expect(parseHeadings(source).map((h) => h.text)).toEqual(["after"]);
  });

  it("recognises setext headings", () => {
    const headings = parseHeadings("Title\n=====\n\nSub\n---");
    expect(headings.map((h) => [h.level, h.text])).toEqual([
      [1, "Title"],
      [2, "Sub"],
    ]);
  });

  it("does not treat a horizontal rule as a setext heading", () => {
    expect(parseHeadings("para\n\n---\n\nnext")).toEqual([]);
  });

  it("requires whitespace after the hashes", () => {
    expect(parseHeadings("#nope\n####### seven")).toEqual([]);
  });
});

describe("headingKeys", () => {
  it("disambiguates repeated headings at the same level", () => {
    const headings = parseHeadings("## Notes\ntext\n## Notes");
    expect(headingKeys(headings)).toEqual(["2:Notes:0", "2:Notes:1"]);
  });
});

describe("buildOutlineModel", () => {
  const headings = parseHeadings("# A\n## B\n## C\n### D\n# E");

  it("nests rows and marks parents", () => {
    const model = buildOutlineModel(headings, new Set());
    expect(model.rows.map((row) => [row.heading.text, row.depth, row.hasChildren])).toEqual([
      ["A", 0, true],
      ["B", 1, false],
      ["C", 1, true],
      ["D", 2, false],
      ["E", 0, false],
    ]);
    expect(model.parentKeys).toEqual(["1:A:0", "2:C:0"]);
  });

  it("hides descendants and counts them", () => {
    const model = buildOutlineModel(headings, new Set(["1:A:0"]));
    expect(model.rows.map((row) => row.heading.text)).toEqual(["A", "E"]);
    expect(model.rows[0].hiddenCount).toBe(3);
    expect(model.rows[0].isCollapsed).toBe(true);
  });

  it("keeps hidden deep headings pointing at the collapsed parent", () => {
    const model = buildOutlineModel(headings, new Set(["1:A:0"]));
    expect(model.visibleIdFor.get("heading-line-4")).toBe("heading-line-1");
    expect(model.visibleIdFor.get("heading-line-2")).toBe("heading-line-1");
  });

  it("survives a heading level jump without losing rows", () => {
    const model = buildOutlineModel(parseHeadings("### Deep\n# Top"), new Set());
    expect(model.rows.map((row) => [row.heading.text, row.depth])).toEqual([
      ["Deep", 2],
      ["Top", 0],
    ]);
    expect(minHeadingLevel(parseHeadings("### Deep\n# Top"))).toBe(1);
  });
});

describe("formatDepthLabel", () => {
  it("formats ranges", () => {
    expect(formatDepthLabel(1)).toBe("H1");
    expect(formatDepthLabel(3)).toBe("H1–H3");
  });
});
