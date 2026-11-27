import { readFile } from "node:fs/promises";
import { TeraxGhostty } from "@terax/ghostty-core/adapted";
import { beforeAll, describe, expect, it } from "vitest";
import { AdaptedGhosttyTerminalModel } from "@/modules/terminal/ghostty/AdaptedGhosttyTerminalModel";
import { RemoteTerminalFeed, decodeBase64 } from "./feed";

const encoder = new TextEncoder();

function base64(bytes: Uint8Array): string {
  return btoa(String.fromCharCode(...bytes));
}

let core: TeraxGhostty;

beforeAll(async () => {
  const file = await readFile(
    new URL(
      "../../../packages/ghostty-core/adapted/ghostty-vt.wasm",
      import.meta.url,
    ),
  );
  core = await TeraxGhostty.loadBytes(Uint8Array.from(file).buffer);
});

function terminal(cols = 20, rows = 4) {
  const model = new AdaptedGhosttyTerminalModel(core, {
    backend: "ghostty-webgpu",
    cols,
    rows,
  });
  const feed = new RemoteTerminalFeed(model);
  feed.meta({ cols, rows, truncated: false, theme: null });
  return { model, feed, screen: () => model.readText(rows) };
}

describe("remote terminal feed", () => {
  it("shows the snapshot, then only what arrives after it", () => {
    const { feed, screen } = terminal();
    feed.snapshot("5", base64(encoder.encode("hello\r\n")));
    feed.data("10", base64(encoder.encode("world")));
    expect(feed.sequence).toBe(10);
    expect(screen()).toContain("hello");
    expect(screen()).toContain("world");
  });

  it("does not duplicate output when a viewer reconnects", () => {
    const { feed, screen } = terminal();
    feed.snapshot("5", base64(encoder.encode("hello\r\n")));
    feed.data("10", base64(encoder.encode("world")));
    // Reconnect: the server resumes after Last-Event-ID, so only new bytes come.
    feed.data("15", base64(encoder.encode(" again")));
    expect(screen().replace(/\s+/g, " ")).toContain("hello world again");
    expect(screen()?.match(/world/g)?.length).toBe(1);
  });

  it("rebuilds from a snapshot without keeping the old screen", () => {
    const { feed, screen } = terminal();
    feed.snapshot("5", base64(encoder.encode("stale\r\n")));
    feed.snapshot("9", base64(encoder.encode("fresh\r\n")));
    expect(screen()).not.toContain("stale");
    expect(screen()).toContain("fresh");
  });

  it("resizes to the PTY and keeps the size for later events", () => {
    const { model, feed } = terminal();
    feed.resize({ cols: 30, rows: 6 });
    expect(model.cols).toBe(30);
    expect(model.rows).toBe(6);
    expect(feed.cols).toBe(30);
  });

  it("decodes a snapshot's bytes exactly", () => {
    expect([...decodeBase64(base64(Uint8Array.of(0, 255, 65)))]).toEqual([
      0, 255, 65,
    ]);
  });
});
