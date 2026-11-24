import { describe, expect, it } from "vitest";
import { saveFailure } from "./saveFailure";

describe("a failed save", () => {
  it("names the server that is not connected", () =>
    expect(saveFailure("not connected to dypod")).toBe(
      "dypod is not connected. Your changes are kept; save again once it is back.",
    ));

  it("passes any other reason on", () => {
    expect(saveFailure("permission denied")).toBe("permission denied");
    expect(saveFailure(new Error("disk full"))).toBe("disk full");
  });
});
