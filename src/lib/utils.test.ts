import { describe, expect, it } from "vitest";
import {
  defaultSourceView,
  hasSourcePreview,
  isHtmlPath,
  isMarkdownPath,
  isMermaidPath,
} from "./utils";

describe("isMermaidPath", () => {
  it("matches mmd and mermaid extensions case-insensitively", () => {
    expect(isMermaidPath("flow.mmd")).toBe(true);
    expect(isMermaidPath("a/b/seq.MMD")).toBe(true);
    expect(isMermaidPath("c:\\docs\\arch.mermaid")).toBe(true);
  });

  it("rejects lookalikes", () => {
    expect(isMermaidPath("flow.mmd.bak")).toBe(false);
    expect(isMermaidPath("flow.mmdx")).toBe(false);
    expect(isMermaidPath("mmd")).toBe(false);
    expect(isMermaidPath("notes.md")).toBe(false);
  });
});

describe("source preview", () => {
  it("covers markdown and mermaid only", () => {
    expect(hasSourcePreview("README.md")).toBe(true);
    expect(hasSourcePreview("flow.mmd")).toBe(true);
    expect(hasSourcePreview("index.html")).toBe(false);
    expect(hasSourcePreview("main.ts")).toBe(false);
  });

  it("opens diagrams rendered and markdown as source", () => {
    expect(defaultSourceView("flow.mmd")).toBe("rendered");
    expect(defaultSourceView("arch.mermaid")).toBe("rendered");
    expect(defaultSourceView("README.md")).toBe("raw");
  });
});

describe("isMarkdownPath", () => {
  it("matches markdown extensions case-insensitively", () => {
    expect(isMarkdownPath("README.md")).toBe(true);
    expect(isMarkdownPath("notes.markdown")).toBe(true);
    expect(isMarkdownPath("doc.mdx")).toBe(true);
    expect(isMarkdownPath("a/b/c.Md")).toBe(true);
  });

  it("only matches the extension at the end of the path", () => {
    expect(isMarkdownPath("file.md.txt")).toBe(false);
    expect(isMarkdownPath("x.mdxx")).toBe(false);
  });

  it("rejects non-markdown or extensionless paths", () => {
    expect(isMarkdownPath("file.txt")).toBe(false);
    expect(isMarkdownPath("mdfile")).toBe(false);
    expect(isMarkdownPath("md")).toBe(false);
  });
});

describe("isHtmlPath", () => {
  it("matches html and htm extensions case-insensitively", () => {
    expect(isHtmlPath("index.html")).toBe(true);
    expect(isHtmlPath("report.HTML")).toBe(true);
    expect(isHtmlPath("a/b/page.htm")).toBe(true);
  });

  it("rejects lookalikes and other markup", () => {
    expect(isHtmlPath("index.html.bak")).toBe(false);
    expect(isHtmlPath("page.htmlx")).toBe(false);
    expect(isHtmlPath("index.md")).toBe(false);
    expect(isHtmlPath("html")).toBe(false);
  });
});
