import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import type { DevServer } from "@/modules/terminal/lib/devServers";

vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: async () => {} }));

const { DevServersMenu, devServerLabel } = await import("./DevServersPill");

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

const render = (servers: DevServer[]) =>
  renderToStaticMarkup(
    createElement(DevServersMenu, { servers, onOpenPreview: () => {} }),
  );

describe("DevServersMenu", () => {
  it("is hidden until a terminal prints a server address", () => {
    expect(render([])).toBe("");
  });

  it("shows the first server's port and how many more there are", () => {
    const vite = server(5173, "http://localhost:5173/");
    expect(render([vite])).toContain(
      'title="Dev server: localhost:5173 · Vite"',
    );
    const html = render([vite, server(8000, "http://127.0.0.1:8000/")]);
    expect(html).toContain(":5173");
    expect(html).toContain("+1");
    expect(html).toContain('title="2 dev servers"');
  });

  it("names the host of a server on an SSH workspace", () => {
    expect(
      devServerLabel(server(3000, "http://localhost:3000/docs", "dypod")),
    ).toBe("localhost:3000/docs · Next.js on dypod");
  });
});
