import { describe, expect, it } from "vitest";
import { readOutlineHidden, writeOutlineHidden } from "./outlinePref";

function memoryStorage() {
  const data = new Map<string, string>();
  return {
    getItem: (key: string) => data.get(key) ?? null,
    setItem: (key: string, value: string) => void data.set(key, value),
    removeItem: (key: string) => void data.delete(key),
  };
}

describe("outline preference", () => {
  it("defaults to shown and remembers a change", () => {
    const storage = memoryStorage();
    expect(readOutlineHidden(storage)).toBe(false);
    writeOutlineHidden(true, storage);
    expect(readOutlineHidden(storage)).toBe(true);
    writeOutlineHidden(false, storage);
    expect(readOutlineHidden(storage)).toBe(false);
  });

  it("survives storage that is missing or throws", () => {
    const broken = {
      getItem: () => {
        throw new Error("denied");
      },
      setItem: () => {
        throw new Error("denied");
      },
      removeItem: () => {},
    };
    expect(readOutlineHidden(null)).toBe(false);
    expect(readOutlineHidden(broken)).toBe(false);
    expect(() => writeOutlineHidden(true, broken)).not.toThrow();
  });
});
