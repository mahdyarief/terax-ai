import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ invoke: async () => {} }));

const { CliAgentsSection } = await import("./CliAgentsSection");

describe("CliAgentsSection", () => {
  it("lists the built-in agents with their commands, Antigravity in Gemini's place", () => {
    const html = renderToStaticMarkup(createElement(CliAgentsSection));
    for (const command of [
      "claude",
      "codex",
      "agy",
      "pi",
      "opencode",
      "grok",
    ]) {
      expect(html).toContain(`>${command}</code>`);
    }
    expect(html).toContain("Antigravity");
    expect(html).not.toContain("Gemini<");
    expect(html).toContain("Add agent");
  });
});
