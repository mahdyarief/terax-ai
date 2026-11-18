import { describe, expect, it } from "vitest";
import { parseHeadings } from "@/components/markdown/useMarkdownHeadings";
import {
  classifyLink,
  headingForFragment,
  headingSlugs,
  rehypeKeepHref,
  slugify,
} from "./links";

describe("heading anchors", () => {
  it("slugs headings the way GitHub does", () => {
    expect(slugify("Getting Started")).toBe("getting-started");
    expect(slugify("**Setup** & `config`")).toBe("setup--config");
    expect(slugify("What's new in v2.0?")).toBe("whats-new-in-v20");
    expect(slugify("[Docs](https://x.dev) page")).toBe("docs-page");
    expect(slugify("Über uns")).toBe("über-uns");
  });

  it("numbers repeated headings", () => {
    const headings = parseHeadings("# Usage\n## Notes\n# Usage\n## Notes");
    expect(headingSlugs(headings)).toEqual([
      "usage",
      "notes",
      "usage-1",
      "notes-1",
    ]);
    expect(headingForFragment(headings, "Usage-1")?.line).toBe(3);
    expect(headingForFragment(headings, "missing")).toBeNull();
  });
});

describe("rehypeKeepHref", () => {
  it("copies every link address before it can be rewritten", () => {
    const link = (href: unknown) => ({
      type: "element",
      tagName: "a",
      properties: { href } as Record<string, unknown>,
      children: [],
    });
    const relative = link("../api/a.md#usage");
    const none = link(undefined);
    rehypeKeepHref()({
      type: "root",
      children: [
        { type: "element", tagName: "p", properties: {}, children: [relative] },
        none,
      ],
    });
    expect(relative.properties.dataHref).toBe("../api/a.md#usage");
    expect(none.properties.dataHref).toBeUndefined();
  });
});

describe("classifyLink", () => {
  const doc = "C:\\work\\docs\\guide.md";

  it("recognises web, mail and phone links as external", () => {
    for (const href of ["https://a.dev/x", "mailto:a@b.co", "tel:+1555"]) {
      expect(classifyLink(href, doc)).toEqual({ kind: "external" });
    }
  });

  it("treats a bare fragment as an anchor in this document", () => {
    expect(classifyLink("#getting-started", doc)).toEqual({
      kind: "anchor",
      fragment: "getting-started",
    });
    expect(classifyLink("#%C3%BCber", doc)).toEqual({
      kind: "anchor",
      fragment: "über",
    });
  });

  it("resolves relative links against the document's folder", () => {
    expect(classifyLink("./other.md", doc)).toEqual({
      kind: "file",
      path: "C:\\work\\docs\\other.md",
      fragment: null,
    });
    expect(classifyLink("../api/my%20file.md#usage", doc)).toEqual({
      kind: "file",
      path: "C:\\work\\api\\my file.md",
      fragment: "usage",
    });
    expect(classifyLink("sub/a.md", "/home/u/docs/guide.md")).toEqual({
      kind: "file",
      path: "/home/u/docs/sub/a.md",
      fragment: null,
    });
  });

  it("blocks everything it cannot place", () => {
    for (const href of [
      "",
      "javascript:alert(1)",
      "file:///etc/passwd",
      "/etc/passwd",
      "C:\\Windows\\system.ini",
    ]) {
      expect(classifyLink(href, doc)).toEqual({ kind: "blocked" });
    }
  });
});
