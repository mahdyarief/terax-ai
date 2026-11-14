import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const here = path.dirname(fileURLToPath(import.meta.url));
const src = readFileSync(path.join(here, "MarkdownPreviewPane.tsx"), "utf8");
const streamdownMatch = src.match(/<Streamdown[\s\S]*?<\/Streamdown>/);
const streamdownJsx = streamdownMatch?.[0] ?? "";

describe("MarkdownPreviewPane Streamdown configuration", () => {
  it("renders complete markdown files in static mode", () => {
    expect(streamdownJsx).toMatch(/mode="static"/);
  });

  it("does not run streaming incomplete-markdown repair for files", () => {
    expect(streamdownJsx).toMatch(/parseIncompleteMarkdown=\{false\}/);
  });
});

describe("MarkdownPreviewPane outline", () => {
  it("exposes the outline as a navigation landmark", () => {
    expect(src).toMatch(/role="navigation"/);
    expect(src).toMatch(/aria-label="Document outline"/);
  });

  it("scrolls to the clicked heading via an escaped id selector", () => {
    expect(src).toMatch(/CSS\.escape\(heading\.id\)/);
    expect(src).toMatch(/scrollIntoView\(\{ behavior: "smooth", block: "start" \}\)/);
    expect(src).toMatch(/onNavigate=\{navigateTo\}/);
  });

  it("wires the hide and re-show toggles", () => {
    expect(src).toMatch(/onHide=\{\(\) => setOutlineHidden\(true\)\}/);
    expect(src).toMatch(/onClick=\{\(\) => setOutlineHidden\(false\)\}/);
  });
});
