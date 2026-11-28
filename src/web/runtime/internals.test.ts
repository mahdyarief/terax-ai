import { describe, expect, it, vi } from "vitest";
import { createTauriInternals } from "./internals";

describe("createTauriInternals", () => {
  it("dispatches JSON and raw invokes to /api/app/invoke", async () => {
    const fetchFn = vi.fn(async (_url: RequestInfo | URL, init?: RequestInit) => {
      const headers = new Headers(init?.headers);
      if (headers.get("X-Terax-Cmd") === "fs_write_file") {
        return new Response(new Uint8Array([9, 8]).buffer, {
          status: 200,
          headers: { "Content-Type": "application/octet-stream" },
        });
      }
      return new Response(JSON.stringify(["bash"]), {
        status: 200,
        headers: { "Content-Type": "application/json" },
      });
    });

    const { tauriInternals } = createTauriInternals({
      getSession: () => ({ id: "sess-1", label: "remote-0" }),
      waitForSession: async () => ({ id: "sess-1", label: "remote-0" }),
      fetchFn: fetchFn as unknown as typeof fetch,
    });

    const shells = await tauriInternals.invoke("pty_list_shells", {});
    expect(shells).toEqual(["bash"]);
    expect(fetchFn).toHaveBeenCalledWith(
      "/api/app/invoke",
      expect.objectContaining({
        method: "POST",
        body: JSON.stringify({
          session: "sess-1",
          cmd: "pty_list_shells",
          args: {},
        }),
      }),
    );

    const raw = await tauriInternals.invoke(
      "fs_write_file",
      new Uint8Array([1, 2, 3]),
    );
    expect(raw).toBeInstanceOf(ArrayBuffer);
    expect(Array.from(new Uint8Array(raw as ArrayBuffer))).toEqual([9, 8]);
  });

  it("handles callbacks, convertFileSrc, window stubs and unlisten", async () => {
    const fetchFn = vi.fn(
      async () =>
        new Response("null", {
          status: 200,
          headers: { "Content-Type": "application/json" },
        }),
    );
    const { tauriInternals, eventPluginInternals } = createTauriInternals({
      getSession: () => ({ id: "sess-2", label: "remote-1" }),
      waitForSession: async () => ({ id: "sess-2", label: "remote-1" }),
      fetchFn: fetchFn as unknown as typeof fetch,
    });

    const hits: unknown[] = [];
    const onceId = tauriInternals.transformCallback((d) => hits.push(d), true);
    tauriInternals.runCallback(onceId, { a: 1 });
    tauriInternals.runCallback(onceId, { a: 2 });
    expect(hits).toEqual([{ a: 1 }]);

    expect(tauriInternals.convertFileSrc("poll", "terax-pty")).toBe(
      "/scheme/terax-pty/sess-2/poll",
    );
    expect(tauriInternals.convertFileSrc("/home/dy/a.png")).toBe(
      "/scheme/asset/sess-2/%2Fhome%2Fdy%2Fa.png",
    );

    expect(await tauriInternals.invoke("plugin:window|is_maximized")).toBe(
      false,
    );
    expect(await tauriInternals.invoke("plugin:window|show")).toBeNull();

    eventPluginInternals.unregisterListener("fs:changed", 42);
    await Promise.resolve();
    expect(fetchFn).toHaveBeenCalledWith(
      "/api/app/invoke",
      expect.objectContaining({
        body: JSON.stringify({
          session: "sess-2",
          cmd: "plugin:event|unlisten",
          args: { event: "fs:changed", eventId: 42 },
        }),
      }),
    );
  });
});
