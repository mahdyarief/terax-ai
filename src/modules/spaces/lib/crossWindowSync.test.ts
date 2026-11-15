import { describe, expect, it } from "vitest";
import {
  applyHostChanged,
  type HostChangedPayload,
} from "./crossWindowSync";
import type { SpaceMeta } from "./store";

function spaces(): SpaceMeta[] {
  return [
    {
      id: "a",
      name: "A",
      root: null,
      env: { kind: "local" },
      hostWindowLabel: "space-a",
      createdAt: 1,
      updatedAt: 1,
    },
    {
      id: "b",
      name: "B",
      root: null,
      env: { kind: "local" },
      hostWindowLabel: "main",
      createdAt: 1,
      updatedAt: 1,
    },
  ];
}

describe("applyHostChanged", () => {
  it("assigns the new host without mutating the input", () => {
    const input = spaces();
    const payload: HostChangedPayload = {
      spaceId: "a",
      hostWindowLabel: "main",
      updatedAt: 42,
    };
    const next = applyHostChanged(input, payload);
    expect(next[0].hostWindowLabel).toBe("main");
    expect(next[0].updatedAt).toBe(42);
    expect(next[1]).toBe(input[1]);
    expect(input[0].hostWindowLabel).toBe("space-a");
  });

  it("keeps the registry unchanged for an unknown space", () => {
    const input = spaces();
    expect(
      applyHostChanged(input, {
        spaceId: "missing",
        hostWindowLabel: "main",
        updatedAt: 42,
      }),
    ).toBe(input);
  });

  it("is idempotent for a repeated broadcast", () => {
    const payload: HostChangedPayload = {
      spaceId: "a",
      hostWindowLabel: "main",
      updatedAt: 42,
    };
    const once = applyHostChanged(spaces(), payload);
    const twice = applyHostChanged(once, payload);
    expect(twice).toEqual(once);
  });
});
