import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import type { DevServer } from "@/modules/terminal/lib/devServers";

vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: async () => {} }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: async () => {} }));

const { DevServersMenu, devServerLabel, liveServerEntry } = await import(
  "./DevServersPill"
);
type Entry = Parameters<typeof DevServersMenu>[0]["entries"][number];

const server = (
  port: number,
  url: string,
  host: string | null = null,
): DevServer => ({
  leafId: port,
  port,
  url,
  host,
});

const entry = (s: DevServer): Entry => ({
  key: String(s.port),
  port: s.port,
  url: s.url,
  label: devServerLabel(s),
  remoteHost: s.host,
});

const render = (entries: Entry[]) =>
  renderToStaticMarkup(
    createElement(DevServersMenu, { entries, onOpenPreview: () => {} }),
  );

describe("DevServersMenu", () => {
  it("is hidden until there is a server", () => {
    expect(render([])).toBe("");
  });

  it("shows the first server's port and how many more there are", () => {
    const vite = entry(server(5173, "http://localhost:5173/"));
    expect(render([vite])).toContain('title="Server: localhost:5173 · Vite"');
    const html = render([vite, entry(server(8000, "http://127.0.0.1:8000/"))]);
    expect(html).toContain(":5173");
    expect(html).toContain("+1");
    expect(html).toContain('title="2 servers"');
  });

  it("names the host of a server on an SSH workspace", () => {
    expect(
      devServerLabel(server(3000, "http://localhost:3000/docs", "dypod")),
    ).toBe("localhost:3000/docs · Next.js on dypod");
  });

  it("labels a Live Server by its folder", () => {
    const live = liveServerEntry(
      {
        id: 1,
        root: "D:\\Work\\dashboard\\html",
        port: 5501,
        url: "http://localhost:5501/",
        lanUrls: [],
        watching: true,
        workspace: { kind: "local" },
      },
      { start: () => {}, stop: () => {} },
    );
    expect(live.label).toBe("localhost:5501 · Live Server: html");
    expect(render([live])).toContain(
      'title="Server: localhost:5501 · Live Server: html"',
    );
  });
});
