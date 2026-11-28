import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ invoke: async () => [] }));
vi.mock("@tauri-apps/api/event", () => ({ listen: async () => () => {} }));

const { RemoteSection, grantSummary } = await import("./RemoteSection");

const flag = globalThis as { __TERAX_REMOTE__?: boolean };

describe("RemoteSection", () => {
  afterEach(() => {
    delete flag.__TERAX_REMOTE__;
  });

  it("lets a browser pair a device and list devices, but never grant or revoke", () => {
    flag.__TERAX_REMOTE__ = true;
    const html = renderToStaticMarkup(createElement(RemoteSection));
    expect(html).toContain("managed in Terax on the machine itself");
    expect(html).toContain("Pair a device");
    expect(html).toContain("Devices");
    expect(html).not.toContain("role=\"switch\"");
    expect(html).not.toContain("Revoke");
    expect(html).not.toContain("Turn on");
  });

  it("summarises a grant in words", () => {
    expect(grantSummary({ input: false, tabs: false, app: false })).toBe("watch only");
    expect(grantSummary({ input: true, tabs: true, app: true })).toBe(
      "type, tabs, full app",
    );
  });

  it("keeps the controls on the desktop", () => {
    const html = renderToStaticMarkup(createElement(RemoteSection));
    expect(html).not.toContain("managed in Terax on the machine itself");
    expect(html).toContain("<button");
  });
});
