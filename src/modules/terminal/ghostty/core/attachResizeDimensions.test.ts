/**
 * Regression: after the host view finishes measuring and the grid is resized
 * (the moment a re-attached / replayed session locks onto the live pane size),
 * the state used to render and to replay the scrollback must reflect the NEW
 * dimensions — never the stale PTY size the session was first opened with.
 *
 * Uses the same real Ghostty core + TerminalResizeSync harness as
 * resizePromptRedraw.test.ts: no module mocks, no bespoke fake renderer.
 */
import { readFile } from "node:fs/promises";
import { TeraxGhostty } from "@terax/ghostty-core/adapted";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { TerminalResizeSync } from "@/modules/terminal/lib/terminalResizeSync";
import { AdaptedGhosttyTerminalModel } from "../AdaptedGhosttyTerminalModel";

const encoder = new TextEncoder();

// The size the PTY was first opened with — the "old" size a stale attach would reuse.
const ATTACH_COLS = 80;
const ATTACH_ROWS = 24;
// The size the host view reports once it has measured (the "new" size).
const VIEW_COLS = 120;
const VIEW_ROWS = 30;

let core: TeraxGhostty;

beforeAll(async () => {
  const file = await readFile(
    new URL(
      "../../../../../packages/ghostty-core/adapted/ghostty-vt.wasm",
      import.meta.url,
    ),
  );
  core = await TeraxGhostty.loadBytes(Uint8Array.from(file).buffer);
});

afterEach(() => vi.useRealTimers());

/** Opens a session at the attach size and returns a synced model plus a resize log. */
function attachedSession() {
  const model = new AdaptedGhosttyTerminalModel(core, {
    backend: "ghostty-webgpu",
    cols: ATTACH_COLS,
    rows: ATTACH_ROWS,
  });
  // Seed some content so the model has a live render state before the view measures.
  const longLine = "x".repeat(100);
  model.write(encoder.encode(`${longLine}\r\n`));

  const pty: Array<[number, number]> = [];
  const sync = new TerminalResizeSync({
    model,
    notify: (cols, rows) => pty.push([cols, rows]),
  });

  // Render state before the view measured — must never be the one reused later.
  const beforeResize = {
    cols: model.cols,
    rows: model.rows,
    cells: model.renderCells().length,
  };

  return { model, pty, sync, beforeResize, longLine };
}

describe("attach/resize uses the measured dimensions, not the stale PTY size", () => {
  it("updates model dimensions, render state and the PTY to the new size", () => {
    const { model, pty, sync, beforeResize } = attachedSession();
    try {
      expect(beforeResize.cols).toBe(ATTACH_COLS);
      expect(beforeResize.rows).toBe(ATTACH_ROWS);
      expect(beforeResize.cells).toBe(ATTACH_COLS * ATTACH_ROWS);

      // The host view finished measuring: resize once, like the end of a gesture.
      expect(sync.request(VIEW_COLS, VIEW_ROWS)).toBe("applied");

      expect(model.cols).toBe(VIEW_COLS);
      expect(model.rows).toBe(VIEW_ROWS);

      // The packed render state the renderer draws uses the new grid, not the old one.
      const viewport = model.viewport();
      expect(viewport.cols).toBe(VIEW_COLS);
      expect(viewport.rows).toBe(VIEW_ROWS);
      expect(model.renderCells().length).toBe(VIEW_COLS * VIEW_ROWS);

      // The PTY heard exactly the measured size, once — never the stale attach size.
      expect(pty).toEqual([[VIEW_COLS, VIEW_ROWS]]);
      expect(pty).not.toContainEqual([ATTACH_COLS, ATTACH_ROWS]);
    } finally {
      model.dispose();
    }
  });

  it("bases the replayed render state on the measured size, discarding the stale one", () => {
    const { model, sync, beforeResize } = attachedSession();
    try {
      // The session attached at the old PTY size. Capture the packed viewport the
      // renderer would have drawn at that size — this is the snapshot that must not
      // be reused once the view has measured.
      const stale = model.viewport();
      expect(stale.cols).toBe(ATTACH_COLS);
      expect(stale.cols).not.toBe(VIEW_COLS);

      // The host view finishes measuring and resizes exactly once; resize() must
      // invalidate the cached render state and repack it at the new dimensions.
      sync.request(VIEW_COLS, VIEW_ROWS);

      // A fresh render state — the one a replay/attach would consume — uses the new
      // dimensions, not the stale attach size the session first opened with.
      const replayed = model.viewport();
      expect(replayed.cols).toBe(VIEW_COLS);
      expect(replayed.rows).toBe(VIEW_ROWS);
      expect(replayed.cols).not.toBe(beforeResize.cols);
      expect(model.renderCells().length).toBe(VIEW_COLS * VIEW_ROWS);
    } finally {
      model.dispose();
    }
  });
});
