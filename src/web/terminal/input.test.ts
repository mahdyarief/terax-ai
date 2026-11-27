import { readFile } from "node:fs/promises";
import { TeraxGhostty } from "@terax/ghostty-core/adapted";
import { beforeAll, describe, expect, it } from "vitest";
import { AdaptedGhosttyTerminalModel } from "@/modules/terminal/ghostty/AdaptedGhosttyTerminalModel";
import { PHONE_KEYS, encodeKeydown, encodePaste } from "./input";

let core: TeraxGhostty;
let model: AdaptedGhosttyTerminalModel;

beforeAll(async () => {
  const file = await readFile(
    new URL(
      "../../../packages/ghostty-core/adapted/ghostty-vt.wasm",
      import.meta.url,
    ),
  );
  core = await TeraxGhostty.loadBytes(Uint8Array.from(file).buffer);
  model = new AdaptedGhosttyTerminalModel(core, {
    backend: "ghostty-webgpu",
    cols: 20,
    rows: 4,
  });
});

function key(partial: Partial<Record<string, unknown>>) {
  return {
    key: "",
    code: "",
    shiftKey: false,
    ctrlKey: false,
    altKey: false,
    metaKey: false,
    getModifierState: () => false,
    ...partial,
  } as unknown as KeyboardEvent;
}

const text = (bytes: Uint8Array | null) =>
  bytes ? new TextDecoder().decode(bytes) : null;

describe("browser key encoding", () => {
  it("sends a typed letter as itself", () => {
    expect(text(encodeKeydown(model, key({ key: "a", code: "KeyA" })))).toBe(
      "a",
    );
  });

  it("sends Enter and Tab as their control bytes", () => {
    expect(
      text(encodeKeydown(model, key({ key: "Enter", code: "Enter" }))),
    ).toBe("\r");
    expect(text(encodeKeydown(model, key({ key: "Tab", code: "Tab" })))).toBe(
      "\t",
    );
  });

  it("uses the terminal's cursor-key mode for arrows", () => {
    expect([
      ...(encodeKeydown(model, key({ key: "ArrowUp", code: "ArrowUp" })) ?? []),
    ]).toEqual([...PHONE_KEYS.up]);
    expect(
      text(encodeKeydown(model, key({ key: "Escape", code: "Escape" }))),
    ).toBe("\x1b");
  });

  it("encodes Ctrl+C as an interrupt", () => {
    const bytes = encodeKeydown(model, key({ key: "c", code: "KeyC" }), {
      ctrl: true,
    });
    expect([...(bytes ?? [])]).toEqual([3]);
  });

  it("sends text from a soft keyboard with no physical code", () => {
    expect(
      text(encodeKeydown(model, key({ key: "é", code: "Unidentified" }))),
    ).toBe("é");
    expect(encodeKeydown(model, key({ key: "Shift", code: "ShiftLeft" }))).toBe(
      null,
    );
  });

  it("pastes through the bracketed-paste mode the application chose", () => {
    expect(text(encodePaste(model, "two\nlines"))).toBe("two\rlines");
  });
});
