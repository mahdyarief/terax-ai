import { describe, expect, it } from "vitest";
import { applyMermaidResult, initialMermaidPreview } from "./mermaidPreview";

describe("applyMermaidResult", () => {
  it("shows a rendered diagram and clears an earlier error", () => {
    const broken = applyMermaidResult(initialMermaidPreview, {
      kind: "error",
      message: "Parse error on line 2",
    });
    expect(applyMermaidResult(broken, { kind: "ok", svg: "<svg/>" })).toEqual({
      svg: "<svg/>",
      error: null,
      loading: false,
    });
  });

  it("keeps the last good diagram while the source is broken", () => {
    const good = applyMermaidResult(initialMermaidPreview, {
      kind: "ok",
      svg: "<svg>a</svg>",
    });
    const next = applyMermaidResult(good, {
      kind: "error",
      message: "Parse error on line 3:\n...\nExpecting 'NODE'",
    });
    expect(next.svg).toBe("<svg>a</svg>");
    expect(next.error).toBe("Parse error on line 3:");
  });

  it("has no diagram to keep when the first render fails", () => {
    const next = applyMermaidResult(initialMermaidPreview, {
      kind: "error",
      message: "",
    });
    expect(next).toEqual({ svg: null, error: "Unknown error", loading: false });
  });

  it("drops the diagram when the file is emptied", () => {
    const good = applyMermaidResult(initialMermaidPreview, {
      kind: "ok",
      svg: "<svg/>",
    });
    expect(applyMermaidResult(good, { kind: "empty" })).toEqual({
      svg: null,
      error: null,
      loading: false,
    });
  });
});
