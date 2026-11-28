import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ invoke: async () => [] }));
vi.mock("@tauri-apps/api/event", () => ({ listen: async () => () => {} }));

const { RemoteSection } = await import("./RemoteSection");

const flag = globalThis as { __TERAX_REMOTE__?: boolean };

describe("RemoteSection", () => {
  afterEach(() => {
    delete flag.__TERAX_REMOTE__;
  });

  it("offers no remote access controls inside a browser session", () => {
    flag.__TERAX_REMOTE__ = true;
    const html = renderToStaticMarkup(createElement(RemoteSection));
    expect(html).toContain("managed in Terax on the machine itself");
    expect(html).not.toContain("role=\"switch\"");
    expect(html).not.toContain("<button");
  });

  it("keeps the controls on the desktop", () => {
    const html = renderToStaticMarkup(createElement(RemoteSection));
    expect(html).not.toContain("managed in Terax on the machine itself");
    expect(html).toContain("<button");
  });
});
