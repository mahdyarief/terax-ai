import { describe, expect, it, vi } from "vitest";
import { ensureTauriBridge } from "./tauriBridge";

function env(over: { ready?: () => boolean; flag?: string | null } = {}) {
  const store = new Map<string, string>();
  if (over.flag) store.set("terax:tauri-bridge-reload", over.flag);
  let t = 0;
  return {
    store,
    reload: vi.fn(),
    env: {
      hasMetadata: over.ready ?? (() => false),
      storage: {
        getItem: (k: string) => store.get(k) ?? null,
        setItem: (k: string, v: string) => void store.set(k, v),
        removeItem: (k: string) => void store.delete(k),
      },
      reload: () => {},
      sleep: async (ms: number) => {
        t += ms;
      },
      now: () => t,
    },
  };
}

describe("ensureTauriBridge", () => {
  it("resolves at once and clears the reload flag when metadata exists", async () => {
    const e = env({ ready: () => true, flag: "1" });
    await expect(ensureTauriBridge(e.env)).resolves.toBe(true);
    expect(e.store.size).toBe(0);
  });

  it("waits for metadata that arrives late", async () => {
    let calls = 0;
    const e = env({ ready: () => ++calls > 3 });
    await expect(ensureTauriBridge(e.env)).resolves.toBe(true);
  });

  it("reloads once when the init script missed this document", async () => {
    const e = env();
    const reload = vi.fn();
    void ensureTauriBridge({ ...e.env, reload }, 100);
    await vi.waitFor(() => expect(reload).toHaveBeenCalledTimes(1));
    expect(e.store.get("terax:tauri-bridge-reload")).toBe("1");
  });

  it("does not loop: gives up after the one reload", async () => {
    const e = env({ flag: "1" });
    const reload = vi.fn();
    await expect(ensureTauriBridge({ ...e.env, reload }, 100)).resolves.toBe(
      false,
    );
    expect(reload).not.toHaveBeenCalled();
  });
});
