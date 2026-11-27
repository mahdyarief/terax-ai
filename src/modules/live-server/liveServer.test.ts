import { beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...a: unknown[]) => invoke(...a),
}));

const { liveServerName, liveServerUrl, useLiveServerStore } = await import(
  "./liveServer"
);

const info = (over: Partial<Parameters<typeof liveServerUrl>[0]> = {}) => ({
  id: 1,
  root: "D:\\Work\\site",
  port: 5501,
  url: "http://localhost:5501/",
  lanUrls: [],
  watching: true,
  ...over,
});

describe("liveServerUrl", () => {
  it("opens an HTML file inside the folder at its own address", () => {
    expect(liveServerUrl(info(), "D:\\Work\\site\\docs\\my page.html")).toBe(
      "http://localhost:5501/docs/my%20page.html",
    );
    expect(liveServerUrl(info(), "d:/work/site/index.HTM")).toBe(
      "http://localhost:5501/index.HTM",
    );
  });

  it("opens the folder for anything else", () => {
    expect(liveServerUrl(info(), "D:\\Work\\site")).toBe(
      "http://localhost:5501/",
    );
    expect(liveServerUrl(info(), "D:\\Work\\site\\app.js")).toBe(
      "http://localhost:5501/",
    );
    expect(liveServerUrl(info(), "D:\\Work\\other\\index.html")).toBe(
      "http://localhost:5501/",
    );
  });

  it("compares paths on a server exactly", () => {
    const remote = info({ root: "/home/me/site" });
    expect(liveServerUrl(remote, "/home/me/site/a.html")).toBe(
      "http://localhost:5501/a.html",
    );
    expect(liveServerUrl(remote, "/home/me/Site/a.html")).toBe(
      "http://localhost:5501/",
    );
  });
});

describe("useLiveServerStore", () => {
  beforeEach(() => {
    invoke.mockReset();
    useLiveServerStore.setState({ servers: [] });
  });

  it("replaces a folder's server when it is started again", async () => {
    const local = { kind: "local" } as const;
    invoke.mockResolvedValueOnce(info({ id: 1 }));
    await useLiveServerStore
      .getState()
      .start("D:\\Work\\site", { workspace: local });
    invoke.mockResolvedValueOnce(
      info({ id: 2, lanUrls: ["http://192.168.1.4:5501/"] }),
    );
    await useLiveServerStore
      .getState()
      .start("D:\\Work\\site", { workspace: local, lan: true });
    expect(invoke).toHaveBeenLastCalledWith("live_server_start", {
      path: "D:\\Work\\site",
      workspace: local,
      lan: true,
    });
    expect(useLiveServerStore.getState().servers.map((s) => s.id)).toEqual([2]);
  });

  it("forgets a stopped server", async () => {
    invoke.mockResolvedValueOnce(info({ id: 3 }));
    await useLiveServerStore
      .getState()
      .start("D:\\Work\\site", { workspace: { kind: "local" } });
    invoke.mockResolvedValueOnce(undefined);
    await useLiveServerStore.getState().stop(3);
    expect(useLiveServerStore.getState().servers).toEqual([]);
  });

  it("names a server by its folder", () => {
    expect(liveServerName(info())).toBe("site");
    expect(liveServerName(info({ root: "/home/me/site/" }))).toBe("site");
  });
});
