import { beforeEach, describe, expect, it, vi } from "vitest";

const calls: string[] = [];
vi.mock("@/modules/remote", () => ({
  ensureConnected: async (host: string) => {
    calls.push(`connect ${host}`);
    if (host === "down") throw new Error("Connection timed out");
  },
  fetchRemoteHome: async (host: string) => {
    calls.push(`home ${host}`);
    return "/home/ubuntu";
  },
}));
vi.mock("@tauri-apps/api/path", () => ({
  homeDir: async () => String.raw`C:\Users\me`,
}));
vi.mock("@/modules/ai/lib/native", () => ({ native: {} }));

const { resolveEnvHome } = await import("./useWorkspaceSwitcher");

beforeEach(() => {
  calls.length = 0;
});

describe("a workspace's home", () => {
  it("on a server is the server's, after connecting", async () => {
    await expect(resolveEnvHome({ kind: "ssh", host: "dypod" })).resolves.toBe(
      "/home/ubuntu",
    );
    expect(calls).toEqual(["connect dypod", "home dypod"]);
  });

  it("is not this machine's when the server cannot be reached", async () => {
    await expect(resolveEnvHome({ kind: "ssh", host: "down" })).rejects.toThrow(
      "Connection timed out",
    );
    expect(calls).toEqual(["connect down"]);
  });

  it("locally is this machine's, with forward slashes", async () => {
    await expect(resolveEnvHome({ kind: "local" })).resolves.toBe("C:/Users/me");
  });
});
