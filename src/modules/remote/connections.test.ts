import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import {
  describeState,
  isValidHost,
  MAX_RECENT_HOSTS,
  parseSnapshot,
  pickerEntries,
  type RemoteState,
  touchRecentHost,
  useRemoteStore,
} from "./connections";
import { RemoteLogLines } from "./RemoteLogDialog";
import { RemoteStatusLabel } from "./RemoteStatusItems";

describe("remote state events", () => {
  it("accept what the app sends", () => {
    expect(parseSnapshot({ host: "dypod", state: "ready" })).toEqual({
      host: "dypod",
      state: { state: "ready" },
    });
    expect(
      parseSnapshot({ host: "dypod", state: "installing", bytes: 2863928 }),
    ).toEqual({ host: "dypod", state: { state: "installing", bytes: 2863928 } });
    expect(
      parseSnapshot({
        host: "dypod",
        state: "reconnecting",
        retry: 2,
        reason: "reset",
      })?.state,
    ).toEqual({ state: "reconnecting", retry: 2, reason: "reset" });
    expect(
      parseSnapshot({ host: "dypod", state: "failed", reason: "denied" })
        ?.state,
    ).toEqual({ state: "failed", reason: "denied" });
  });

  it("refuse anything else", () => {
    for (const bad of [
      null,
      "ready",
      { state: "ready" },
      { host: "dypod", state: "exploded" },
      { host: "dypod", state: "installing" },
      { host: "dypod", state: "failed" },
      { host: "dypod", state: "reconnecting", reason: "x" },
    ]) {
      expect(parseSnapshot(bad)).toBeNull();
    }
  });

  it("update the store per host", () => {
    const { apply } = useRemoteStore.getState();
    apply({ host: "a", state: { state: "connecting" } });
    apply({ host: "b", state: { state: "ready" } });
    apply({ host: "a", state: { state: "failed", reason: "no" } });
    expect(useRemoteStore.getState().hosts).toMatchObject({
      a: { state: "failed", reason: "no" },
      b: { state: "ready" },
    });
  });
});

describe("hosts", () => {
  it("are checked like the app checks them", () => {
    expect(isValidHost("dypod")).toBe(true);
    expect(isValidHost("ubuntu@203.0.113.7")).toBe(true);
    expect(isValidHost("")).toBe(false);
    expect(isValidHost("-oProxyCommand=x")).toBe(false);
    expect(isValidHost("two words")).toBe(false);
    expect(isValidHost("tab\there")).toBe(false);
    expect(isValidHost("nul\u0000")).toBe(false);
    expect(isValidHost("a".repeat(255))).toBe(true);
    expect(isValidHost("a".repeat(256))).toBe(false);
  });

  it("list recents first, then the config's, then a typed one", () => {
    const recents = ["dypod", "me@box"];
    const config = ["web1", "dypod", "db"];
    expect(pickerEntries(recents, config, "")).toEqual([
      { host: "dypod", source: "recent" },
      { host: "me@box", source: "recent" },
      { host: "web1", source: "config" },
      { host: "db", source: "config" },
    ]);
    expect(pickerEntries(recents, config, "D")).toEqual([
      { host: "dypod", source: "recent" },
      { host: "db", source: "config" },
      { host: "D", source: "typed" },
    ]);
    expect(pickerEntries(recents, config, " ubuntu@10.0.0.5 ")).toEqual([
      { host: "ubuntu@10.0.0.5", source: "typed" },
    ]);
    expect(pickerEntries(recents, config, "db")).toEqual([
      { host: "db", source: "config" },
    ]);
    expect(pickerEntries([], [], "-oBad")).toEqual([]);
  });

  it("keep recents newest first, without repeats, capped", () => {
    let list: string[] = [];
    for (let i = 0; i < MAX_RECENT_HOSTS + 2; i++) {
      list = touchRecentHost(list, `h${i}`);
    }
    expect(list).toHaveLength(MAX_RECENT_HOSTS);
    expect(list[0]).toBe(`h${MAX_RECENT_HOSTS + 1}`);
    expect(touchRecentHost(["a", "b", "c"], "c")).toEqual(["c", "a", "b"]);
  });
});

describe("the status bar item", () => {
  const face = (state: RemoteState) =>
    renderToStaticMarkup(
      createElement(RemoteStatusLabel, { host: "dypod", state }),
    );

  it("shows each state", () => {
    expect(face({ state: "connecting" })).toContain("Connecting…");
    expect(face({ state: "connecting" })).toContain('data-tone="busy"');
    const installing = face({ state: "installing", bytes: 2863928 });
    expect(installing).toContain("Installing agent…");
    expect(installing).toContain("2.7 MB");
    expect(face({ state: "handshaking" })).toContain("Starting agent…");
    const ready = face({ state: "ready" });
    expect(ready).toContain("SSH: dypod");
    expect(ready).toContain("Connected");
    expect(ready).toContain('data-tone="ok"');
    const reconnecting = face({
      state: "reconnecting",
      retry: 1,
      reason: "Connection reset",
    });
    expect(reconnecting).toContain("Reconnecting…");
    expect(reconnecting).toContain("Connection reset");
  });

  it("shows why it failed", () => {
    const failed = face({
      state: "failed",
      reason: "ubuntu@dypod: Permission denied (publickey).",
    });
    expect(failed).toContain("Failed");
    expect(failed).toContain('data-tone="error"');
    expect(failed).toContain("Permission denied (publickey).");
    expect(describeState({ state: "failed", reason: "x" }).detail).toBe("x");
  });
});

describe("the log", () => {
  const log = (lines: string[] | null, error: string | null = null) =>
    renderToStaticMarkup(createElement(RemoteLogLines, { lines, error }));

  it("shows lines, loading, empty and errors", () => {
    expect(log(["ssh: OpenSSH 9.5", "state: connected"])).toContain(
      "ssh: OpenSSH 9.5\nstate: connected",
    );
    expect(log(null)).toContain("Loading…");
    expect(log([])).toContain("Nothing logged yet.");
    expect(log(null, "remote connections are not available")).toContain(
      "remote connections are not available",
    );
  });
});
