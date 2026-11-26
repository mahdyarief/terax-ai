import { describe, expect, it } from "vitest";
import { buildShortcuts } from "../shortcuts";
import { shortcutNotes } from "./shortcutConflicts";

const other = buildShortcuts(false);
const mac = buildShortcuts(true);

describe("shortcutNotes", () => {
  it("says nothing about the defaults, even the ones that share a chord on purpose", () => {
    expect(shortcutNotes(other, {})).toEqual({});
    // macOS: Command+K clears a terminal and opens the palette elsewhere.
    expect(shortcutNotes(mac, {})).toEqual({});
  });

  it("tells history it lost Ctrl+P when Go to file is recorded there", () => {
    const notes = shortcutNotes(other, {
      "commandPalette.files": [{ ctrl: true, key: "p" }],
    });
    expect(notes["terminal.historyPrev"]).toMatch(/“Go to file” uses Ctrl\+P/);
    expect(notes["commandPalette.files"]).toMatch(
      /also set for “Previous command in shell history”/,
    );
  });

  it("is quiet again once history moves to a free chord", () => {
    const notes = shortcutNotes(other, {
      "commandPalette.files": [{ ctrl: true, key: "p" }],
      "terminal.historyPrev": [{ ctrl: true, shift: true, key: "F9" }],
    });
    expect(notes["terminal.historyPrev"]).toBeUndefined();
    // Go to file now owns a plain Ctrl+P, which terminals read.
    expect(notes["commandPalette.files"]).toMatch(/also a terminal key/);
  });

  it("warns about a recorded control byte outside the editor only", () => {
    const notes = shortcutNotes(other, {
      "sidebar.toggle": [{ ctrl: true, key: "e" }],
      "editor.save": [{ ctrl: true, key: "s" }],
    });
    expect(notes["sidebar.toggle"]).toMatch(/Ctrl\+E is also a terminal key/);
    expect(notes["editor.save"]).toBeUndefined();
  });
});
