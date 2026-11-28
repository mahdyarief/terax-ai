import { describe, expect, it } from "vitest";
import { composerKeyAction } from "./composer";
import { PHONE_KEYS } from "./input";

const key = (k: string, shiftKey = false, composing = false) => ({
  key: k,
  shiftKey,
  composing,
});

describe("typing area keys", () => {
  it("sends the line on Enter and keeps Shift+Enter as a new line", () => {
    expect(composerKeyAction(key("Enter"), "ls -la", false)).toEqual({
      kind: "submit",
    });
    expect(composerKeyAction(key("Enter"), "", false)).toEqual({
      kind: "submit",
    });
    expect(composerKeyAction(key("Enter", true), "ls", false)).toBeNull();
  });

  it("passes editing keys to the terminal only while nothing is typed", () => {
    expect(composerKeyAction(key("ArrowUp"), "", false)).toEqual({
      kind: "bytes",
      bytes: PHONE_KEYS.up,
    });
    expect(composerKeyAction(key("Backspace"), "", false)).toEqual({
      kind: "bytes",
      bytes: new TextEncoder().encode("\x7f"),
    });
    expect(composerKeyAction(key("ArrowUp"), "git", false)).toBeNull();
    expect(composerKeyAction(key("Backspace"), "g", false)).toBeNull();
  });

  it("turns a letter into Ctrl+letter when Ctrl is latched", () => {
    expect(composerKeyAction(key("c"), "", true)).toEqual({
      kind: "ctrl",
      letter: "c",
    });
    expect(composerKeyAction(key("c"), "", false)).toBeNull();
  });

  it("leaves an input method's composition alone", () => {
    expect(composerKeyAction(key("Enter", false, true), "", false)).toBeNull();
  });
});
