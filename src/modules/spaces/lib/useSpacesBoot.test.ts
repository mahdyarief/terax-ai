import { describe, expect, it } from "vitest";
import { spacesForWindow } from "./useSpacesBoot";

describe("spacesForWindow", () => {
  it("boot only hydrates spaces hosted by this window", () => {
    const spaces = [
      { id: "a", hostWindowLabel: "main" },
      { id: "b", hostWindowLabel: "space-b" },
    ] as any;
    const hosted = spacesForWindow(spaces, "space-b");
    expect(hosted.map((s) => s.id)).toEqual(["b"]);
  });
});
