import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invoke(...args),
}));

const {
  childPath,
  ensureConnected,
  folderName,
  parentPath,
  useRemoteStore,
} = await import("./connections");
const { FolderListing } = await import("./RemoteFolderPicker");
const { RemoteGateNotice } = await import("./RemoteGate");

const noop = () => {};

beforeEach(() => {
  invoke.mockReset();
  invoke.mockResolvedValue(undefined);
  useRemoteStore.setState({ hosts: {}, readyEpochs: {} });
});

describe("server paths", () => {
  it("go up and down", () => {
    expect(parentPath("/home/me/repo")).toBe("/home/me");
    expect(parentPath("/home/me/repo/")).toBe("/home/me");
    expect(parentPath("/home")).toBe("/");
    expect(parentPath("/")).toBe("/");
    expect(childPath("/home/me", "repo")).toBe("/home/me/repo");
    expect(childPath("/", "srv")).toBe("/srv");
    expect(folderName("/home/me/repo")).toBe("repo");
    expect(folderName("/")).toBe("/");
  });
});

describe("ensureConnected", () => {
  const apply = (host: string, state: Parameters<
    ReturnType<typeof useRemoteStore.getState>["apply"]
  >[0]["state"]) => useRemoteStore.getState().apply({ host, state });

  it("resolves at once when the host is ready", async () => {
    apply("box", { state: "ready" });
    await ensureConnected("box");
    expect(invoke).not.toHaveBeenCalled();
  });

  it("connects and resolves when the host becomes ready", async () => {
    const done = ensureConnected("box");
    expect(invoke).toHaveBeenCalledWith("remote_connect", { host: "box" });
    apply("box", { state: "connecting" });
    apply("box", { state: "installing", bytes: 10 });
    apply("box", { state: "ready" });
    await expect(done).resolves.toBeUndefined();
  });

  it("rejects with ssh's reason when this attempt fails", async () => {
    // Left over from an earlier attempt: not this one's outcome.
    apply("box", { state: "failed", reason: "old" });
    const done = ensureConnected("box");
    let settled = false;
    void done.then(
      () => (settled = true),
      () => (settled = true),
    );
    await Promise.resolve();
    expect(settled).toBe(false);
    apply("box", { state: "connecting" });
    apply("box", { state: "failed", reason: "Permission denied (publickey)." });
    await expect(done).rejects.toThrow("Permission denied (publickey).");
  });

  it("rejects when the app refuses to connect", async () => {
    invoke.mockRejectedValueOnce("remote connections are not available");
    await expect(ensureConnected("box")).rejects.toThrow("not available");
  });

  it("counts each new connection", () => {
    apply("box", { state: "connecting" });
    apply("box", { state: "ready" });
    apply("box", { state: "ready" });
    apply("box", { state: "reconnecting", retry: 0, reason: "x" });
    apply("box", { state: "ready" });
    expect(useRemoteStore.getState().readyEpochs.box).toBe(2);
  });
});

describe("the folder picker", () => {
  const render = (listing: Parameters<typeof FolderListing>[0]["listing"]) =>
    renderToStaticMarkup(
      createElement(FolderListing, {
        listing,
        state: { state: "installing", bytes: 1 },
        onUp: noop,
        onEnter: noop,
        onRetry: noop,
      }),
    );

  it("shows the connection while connecting", () => {
    expect(render({ phase: "connecting" })).toContain("Installing agent…");
  });

  it("lists folders with a way up", () => {
    const html = render({
      phase: "ready",
      browse: { path: "/home/me", dirs: [".config", "repo"] },
    });
    expect(html).toContain("/home/me");
    expect(html).toContain("..");
    expect(html).toContain("repo");
    expect(html).toContain(".config");
  });

  it("has no way up at the top, and says when a folder is empty", () => {
    const html = render({ phase: "ready", browse: { path: "/", dirs: [] } });
    expect(html).not.toContain("..");
    expect(html).toContain("No folders here.");
  });

  it("shows errors with a retry", () => {
    const html = render({
      phase: "error",
      message: "No such file or directory",
      path: "/nope",
    });
    expect(html).toContain("No such file or directory");
    expect(html).toContain("Try again");
    expect(render({ phase: "loading", path: null })).toContain("Loading…");
  });
});

describe("the explorer while a server is not usable", () => {
  const notice = (
    state: Parameters<typeof RemoteGateNotice>[0]["state"],
    error: string | null = null,
  ) =>
    renderToStaticMarkup(
      createElement(RemoteGateNotice, { host: "box", state, error, onConnect: noop }),
    );

  it("offers Connect when down, with the reason", () => {
    const failed = notice({ state: "failed", reason: "Connection timed out" });
    expect(failed).toContain("SSH: box");
    expect(failed).toContain("Connection timed out");
    expect(failed).toContain("Connect");
    expect(notice(undefined)).toContain("Not connected");
    expect(notice(undefined)).toContain("Connect");
  });

  it("shows progress without a button while connecting", () => {
    const busy = notice({ state: "connecting" });
    expect(busy).toContain("Connecting…");
    expect(busy).not.toContain("<button");
  });

  it("shows an authorization error once connected", () => {
    const html = notice({ state: "ready" }, "permission denied");
    expect(html).toContain("Opening folder…");
    expect(html).toContain("permission denied");
  });
});
