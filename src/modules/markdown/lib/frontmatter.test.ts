import { describe, expect, it } from "vitest";
import { splitFrontmatter } from "./frontmatter";

describe("splitFrontmatter", () => {
  it("lifts simple frontmatter out and keeps the body's line offset", () => {
    const result = splitFrontmatter(
      '---\ntitle: "Hello"\ndraft: true\n---\n# Heading\ntext',
    );
    expect(result.frontmatter?.fields).toEqual([
      ["title", "Hello"],
      ["draft", "true"],
    ]);
    expect(result.body).toBe("# Heading\ntext");
    expect(result.lineOffset).toBe(4);
  });

  it("keeps nested YAML as raw text", () => {
    const result = splitFrontmatter("---\ntags:\n  - a\n  - b\n---\nbody");
    expect(result.frontmatter?.fields).toBeNull();
    expect(result.frontmatter?.raw).toBe("tags:\n  - a\n  - b");
    expect(result.body).toBe("body");
  });

  it("accepts CRLF, a BOM and a `...` closing fence", () => {
    const result = splitFrontmatter("﻿---\r\na: 1\r\n...\r\nbody");
    expect(result.frontmatter?.fields).toEqual([["a", "1"]]);
    expect(result.body).toBe("body");
  });

  it("leaves documents without frontmatter untouched", () => {
    for (const source of [
      "# Title\n\n---\n\ntext",
      "---\n\n---\nJust two rules",
      "---\ntitle: never closed\n# Heading",
      "",
    ]) {
      expect(splitFrontmatter(source)).toEqual({
        frontmatter: null,
        body: source,
        lineOffset: 0,
      });
    }
  });
});
