import { describe, expect, it } from "vitest";
import {
  isShellKeyShortcut,
  isTheShellsOwnChord,
  SHELL_KEYS,
} from "./shellKeys";

const ev = (key: string, mods: Partial<KeyboardEvent> = {}) => ({
  key,
  ctrlKey: false,
  shiftKey: false,
  altKey: false,
  metaKey: false,
  ...mods,
});

describe("shell keys", () => {
  it("send readline's previous and next history bytes", () => {
    expect(SHELL_KEYS["terminal.historyPrev"].data).toBe(
      String.fromCharCode(16),
    );
    expect(SHELL_KEYS["terminal.historyNext"].data).toBe(
      String.fromCharCode(14),
    );
  });

  it("are the history shortcuts only", () => {
    expect(isShellKeyShortcut("terminal.historyPrev")).toBe(true);
    expect(isShellKeyShortcut("terminal.historyNext")).toBe(true);
    expect(isShellKeyShortcut("terminal.clear")).toBe(false);
  });

  it("recognize the shell's own chord, which the app lets through", () => {
    expect(
      isTheShellsOwnChord("terminal.historyPrev", ev("p", { ctrlKey: true })),
    ).toBe(true);
    expect(
      isTheShellsOwnChord("terminal.historyPrev", ev("P", { ctrlKey: true })),
    ).toBe(true);
    expect(
      isTheShellsOwnChord("terminal.historyNext", ev("n", { ctrlKey: true })),
    ).toBe(true);
  });

  it("send the byte themselves from any other chord", () => {
    const prev = "terminal.historyPrev" as const;
    expect(
      isTheShellsOwnChord(prev, ev("p", { ctrlKey: true, shiftKey: true })),
    ).toBe(false);
    expect(
      isTheShellsOwnChord(prev, ev("p", { ctrlKey: true, altKey: true })),
    ).toBe(false);
    expect(isTheShellsOwnChord(prev, ev("p", { metaKey: true }))).toBe(false);
    expect(isTheShellsOwnChord(prev, ev("ArrowUp", { altKey: true }))).toBe(
      false,
    );
  });
});
