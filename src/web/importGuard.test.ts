import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { traceEager } from "../../scripts/eager-graph.mjs";

// The web client is served by the remote HTTP server to a plain browser, where
// `@tauri-apps/*` exists only as a broken import. Passthrough code (the Ghostty
// model, packers, blocks) is shared with the desktop; this test makes sure no
// Tauri-bound module (window focus, PTY plumbing) is pulled into it.
const ENTRIES = ["src/web/main.tsx", "src/web/RemoteApp.tsx"];

describe("web client import graph", () => {
  it.each(ENTRIES)("%s imports nothing from @tauri-apps", (entry) => {
    const { files } = traceEager(entry);
    const tauriBound = [...files].filter((file) =>
      readFileSync(file, "utf8").includes("@tauri-apps"),
    );
    expect(tauriBound).toEqual([]);
  });

  it("remote runtime never imports app code or @tauri-apps", () => {
    const { files } = traceEager("src/web/runtime/main.ts");
    const invalid = [...files].filter((file) => {
      const normalized = file.replace(/\\/g, "/");
      if (!normalized.includes("/src/web/runtime/")) {
        return true;
      }
      return readFileSync(file, "utf8").includes("@tauri-apps");
    });
    expect(invalid).toEqual([]);
  });
});
