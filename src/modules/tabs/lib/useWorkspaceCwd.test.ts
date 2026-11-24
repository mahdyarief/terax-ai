import { describe, expect, it } from "vitest";
import { isWithinRoot } from "./useWorkspaceCwd";

describe("a terminal folder inside the space's root", () => {
  it("is the root itself or any folder below it", () => {
    expect(isWithinRoot("/home/me/repo", "/home/me/repo")).toBe(true);
    expect(isWithinRoot("/home/me/repo/src/ui", "/home/me/repo")).toBe(true);
    expect(isWithinRoot("/home/me/repo/", "/home/me/repo")).toBe(true);
    expect(isWithinRoot("/srv/x", "/")).toBe(true);
  });

  it("is not a sibling that shares a prefix, nor a folder above", () => {
    expect(isWithinRoot("/home/me/repo-old", "/home/me/repo")).toBe(false);
    expect(isWithinRoot("/home/me", "/home/me/repo")).toBe(false);
    expect(isWithinRoot("/etc", "/home/me/repo")).toBe(false);
  });

  it("compares Windows paths without case or slash style", () => {
    expect(
      isWithinRoot(String.raw`c:\Users\Me\Repo\src`, "C:/Users/me/repo"),
    ).toBe(true);
    expect(isWithinRoot(String.raw`D:\Repo`, String.raw`C:\Repo`)).toBe(false);
  });
});
