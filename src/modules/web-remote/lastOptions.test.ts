import { describe, expect, it } from "vitest";
import {
  DEFAULT_REMOTE_OPTIONS,
  normalizeRemoteOptions,
  readRemoteOptions,
  writeRemoteOptions,
} from "./lastOptions";

function memoryStorage() {
  const map = new Map<string, string>();
  return {
    getItem: (key: string) => map.get(key) ?? null,
    setItem: (key: string, value: string) => {
      map.set(key, value);
    },
  };
}

describe("remote access options", () => {
  it("remembers the last start", () => {
    const storage = memoryStorage();
    writeRemoteOptions(
      { bind: "tailscale", port: 7682, allowPlainLan: false, https: true },
      storage,
    );
    expect(readRemoteOptions(storage)).toEqual({
      bind: "tailscale",
      port: 7682,
      allowPlainLan: false,
      https: true,
    });
  });

  it("starts from the safe default when nothing or junk is stored", () => {
    expect(readRemoteOptions(memoryStorage())).toEqual(DEFAULT_REMOTE_OPTIONS);
    const storage = memoryStorage();
    storage.setItem("terax.remoteAccess.last", "{not json");
    expect(readRemoteOptions(storage)).toEqual(DEFAULT_REMOTE_OPTIONS);
    expect(readRemoteOptions(null)).toEqual(DEFAULT_REMOTE_OPTIONS);
  });

  it("never keeps an opt-in that does not fit the bind", () => {
    expect(
      normalizeRemoteOptions({
        bind: "loopback",
        port: 80,
        allowPlainLan: true,
        https: true,
      }),
    ).toEqual(DEFAULT_REMOTE_OPTIONS);
    expect(normalizeRemoteOptions({ bind: "everywhere" }).bind).toBe(
      "loopback",
    );
  });
});
