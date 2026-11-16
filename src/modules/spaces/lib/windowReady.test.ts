import { beforeEach, describe, expect, it } from "vitest";
import {
  isWindowReady,
  noteWindowReady,
  resetWindowReady,
  waitForWindowReady,
} from "./windowReady";

describe("windowReady", () => {
  beforeEach(resetWindowReady);

  it("resolves immediately for an already announced window", async () => {
    noteWindowReady("space-a");

    await expect(waitForWindowReady("space-a")).resolves.toBeUndefined();
    expect(isWindowReady("space-a")).toBe(true);
  });

  it("resolves when the window announces later", async () => {
    const pending = waitForWindowReady("space-b", 1000);
    noteWindowReady("space-b");

    await expect(pending).resolves.toBeUndefined();
  });

  it("resolves every waiter for the same window", async () => {
    const a = waitForWindowReady("space-c", 1000);
    const b = waitForWindowReady("space-c", 1000);
    noteWindowReady("space-c");

    await expect(Promise.all([a, b])).resolves.toEqual([undefined, undefined]);
  });

  it("rejects on timeout without marking the window ready", async () => {
    await expect(waitForWindowReady("space-d", 5)).rejects.toThrow(
      "window ready timeout",
    );
    expect(isWindowReady("space-d")).toBe(false);
  });

  it("ignores empty window labels", () => {
    noteWindowReady("");
    expect(isWindowReady("")).toBe(false);
  });
});
