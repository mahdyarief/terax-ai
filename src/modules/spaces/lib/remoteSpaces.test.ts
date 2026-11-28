import { describe, expect, it } from "vitest";
import {
  isHomeWindow,
  isRemoteWindowLabel,
  planRemoteSpaces,
} from "./remoteSpaces";
import type { SpaceMeta } from "./store";

function space(id: string, host?: string | null): SpaceMeta {
  return {
    id,
    name: id,
    root: null,
    hostWindowLabel: host ?? undefined,
    createdAt: 0,
    updatedAt: 0,
  } as SpaceMeta;
}

describe("planRemoteSpaces", () => {
  const label = "remote-a1b2";

  it("does nothing when the browser already hosts a space", () => {
    expect(
      planRemoteSpaces({
        spaces: [space("default", "main"), space("mine", label)],
        windowLabel: label,
        headless: true,
        deviceName: "Laptop",
      }),
    ).toEqual({ kind: "none" });
  });

  it("takes over every space of the hidden main window on a headless host", () => {
    expect(
      planRemoteSpaces({
        spaces: [
          space("default", "main"),
          space("legacy", null),
          space("other", "remote-ffff"),
          space("win", "space-win"),
        ],
        windowLabel: label,
        headless: true,
        deviceName: "Laptop",
      }),
    ).toEqual({ kind: "reclaim", spaceIds: ["default", "legacy"] });
  });

  it("makes its own space on a desktop someone is using", () => {
    expect(
      planRemoteSpaces({
        spaces: [space("default", "main")],
        windowLabel: label,
        headless: false,
        deviceName: "Laptop",
      }),
    ).toEqual({ kind: "create", name: "Laptop" });
  });

  it("makes its own space when a headless main holds none", () => {
    expect(
      planRemoteSpaces({
        spaces: [space("other", "remote-ffff")],
        windowLabel: label,
        headless: true,
        deviceName: null,
      }),
    ).toEqual({ kind: "create", name: "Browser" });
  });
});

describe("window kinds", () => {
  it("treats main and browser sessions as home windows, not space windows", () => {
    expect(isHomeWindow("main")).toBe(true);
    expect(isHomeWindow("remote-a1b2")).toBe(true);
    expect(isHomeWindow("space-abc")).toBe(false);
    expect(isHomeWindow("settings")).toBe(false);
    expect(isRemoteWindowLabel("remote-a1b2")).toBe(true);
    expect(isRemoteWindowLabel("main")).toBe(false);
  });
});
