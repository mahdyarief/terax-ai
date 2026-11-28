import { describe, expect, it } from "vitest";
import {
  applyHostChanged,
  type HostChangedPayload,
  reclaimHost,
  shouldAnswerWindowReady,
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

describe("shouldAnswerWindowReady", () => {
  it("answers a newcomer's announcement, but not answers or its own", () => {
    expect(shouldAnswerWindowReady({ window: "remote-a1" }, "main")).toBe(true);
    expect(
      shouldAnswerWindowReady({ window: "remote-a1", reply: true }, "main"),
    ).toBe(false);
    expect(shouldAnswerWindowReady({ window: "main" }, "main")).toBe(false);
    expect(shouldAnswerWindowReady(undefined, "main")).toBe(false);
  });
});

describe("reclaimHost", () => {
  it("returns the current host when main wants the space back", () => {
    expect(reclaimHost({ hostWindowLabel: "space-a" }, "main")).toBe("space-a");
  });

  it("returns null for a space already hosted by main", () => {
    expect(reclaimHost({ hostWindowLabel: "main" }, "main")).toBeNull();
    expect(reclaimHost({}, "main")).toBeNull();
  });

  it("lets a browser session take a space from main", () => {
    expect(reclaimHost({ hostWindowLabel: "main" }, "remote-a1")).toBe("main");
    expect(reclaimHost({}, "remote-a1")).toBe("main");
    expect(reclaimHost({ hostWindowLabel: "remote-a1" }, "remote-a1")).toBeNull();
  });

  it("is unavailable from space windows", () => {
    expect(reclaimHost({ hostWindowLabel: "space-b" }, "space-a")).toBeNull();
  });

  it("returns null for an unknown space", () => {
    expect(reclaimHost(undefined, "main")).toBeNull();
  });
});
