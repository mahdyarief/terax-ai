import { describe, expect, it } from "vitest";
import { isPreviewTrusted, trustPreview } from "./htmlPreviewTrust";

function memoryStorage() {
  const data = new Map<string, string>();
  return {
    data,
    getItem: (key: string) => data.get(key) ?? null,
    setItem: (key: string, value: string) => void data.set(key, value),
  };
}

describe("html preview trust", () => {
  it("is off until the folder is trusted, and is stored for later sessions", () => {
    const storage = memoryStorage();
    expect(isPreviewTrusted("D:\\work\\site", storage)).toBe(false);
    trustPreview("D:\\work\\site\\", storage);
    // Either separator and a trailing slash name the same folder.
    expect(isPreviewTrusted("D:/work/site", storage)).toBe(true);
    expect([...storage.data.values()]).toEqual(['["D:/work/site"]']);
    expect(isPreviewTrusted("D:/work/other", storage)).toBe(false);
  });

  it("still works for the session when storage is unavailable", () => {
    trustPreview("/tmp/site", null);
    expect(isPreviewTrusted("/tmp/site/", null)).toBe(true);
  });
});
