import { describe, expect, it } from "vitest";
import { planSpaceHandoff } from "./handoff";

describe("planSpaceHandoff", () => {
  it("focuses an existing window instead of re-hosting", () => {
    expect(
      planSpaceHandoff({ spaceId: "a", hostWindowLabel: "space-a" }, "main"),
    ).toEqual({ kind: "focus", windowLabel: "space-a" });
  });

  it("hands off a locally hosted space", () => {
    expect(
      planSpaceHandoff({ spaceId: "a", hostWindowLabel: "main" }, "main"),
    ).toEqual({ kind: "handoff", spaceId: "a", from: "main", to: "space-a" });
  });

  it("treats a null host as main", () => {
    expect(
      planSpaceHandoff({ spaceId: "b", hostWindowLabel: null }, "main"),
    ).toEqual({ kind: "handoff", spaceId: "b", from: "main", to: "space-b" });
  });

  it("does not hand off a space from another space window", () => {
    expect(
      planSpaceHandoff({ spaceId: "a", hostWindowLabel: "space-a" }, "space-a"),
    ).toBeNull();
  });
});
