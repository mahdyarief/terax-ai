import { describe, expect, it } from "vitest";
import { selectHostedSpaces } from "./useSpaces";

describe("selectHostedSpaces", () => {
  it("filters spaces hosted by the given window", () => {
    const spaces = [
      { id: "a", hostWindowLabel: "main" },
      { id: "b", hostWindowLabel: "space-b" },
    ] as any;
    expect(selectHostedSpaces(spaces, "main").map((s) => s.id)).toEqual(["a"]);
  });

  it("treats missing hostWindowLabel as main", () => {
    const spaces = [{ id: "c" }, { id: "d", hostWindowLabel: "space-d" }] as any;
    expect(selectHostedSpaces(spaces, "main").map((s) => s.id)).toEqual(["c"]);
  });
});
