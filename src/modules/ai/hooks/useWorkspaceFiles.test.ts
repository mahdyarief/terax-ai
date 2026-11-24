import { describe, expect, it, vi } from "vitest";
import { isFresh } from "./useWorkspaceFiles";

vi.mock("@/modules/workspace", () => ({
  currentWorkspaceEnv: () => ({ kind: "local" }),
}));

describe("the workspace's file list", () => {
  it("is fetched again sooner for a server's folder", () => {
    const entry = { fetchedAt: 0 };
    expect(isFresh(entry, false, 30_000)).toBe(true);
    expect(isFresh(entry, true, 30_000)).toBe(false);
    expect(isFresh(entry, true, 5_000)).toBe(true);
    expect(isFresh(entry, false, 61_000)).toBe(false);
  });
});
