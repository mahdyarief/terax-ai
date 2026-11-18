import { describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/plugin-opener", () => ({ revealItemInDir: vi.fn() }));
vi.mock("@/modules/workspace", () => ({ currentWorkspaceEnv: () => null }));

import { formatHeadSize, headSummary } from "./TextHeadPreview";

describe("oversized text preview summary", () => {
  it("says how much of the file is shown", () => {
    expect(headSummary(256 * 1024, "73.7 MB", true)).toBe(
      "Showing the first 256 KB of 73.7 MB · read-only",
    );
  });

  it("does not claim a cut when the whole file fit", () => {
    expect(headSummary(900, "900 B", false)).toBe("900 B · read-only");
  });

  it("formats sizes in the unit that fits", () => {
    expect(formatHeadSize(512)).toBe("512 B");
    expect(formatHeadSize(200 * 1024)).toBe("200 KB");
    expect(formatHeadSize(3 * 1024 * 1024)).toBe("3.0 MB");
  });
});
