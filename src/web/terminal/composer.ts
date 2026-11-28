import { PHONE_KEYS, type PhoneKey } from "./input";

/** What a key pressed in the typing area does. */
export type ComposerAction =
  /** Send the text (if any) and press Enter. */
  | { kind: "submit" }
  /** Send these bytes straight to the terminal. */
  | { kind: "bytes"; bytes: Uint8Array }
  /** Ctrl + this letter, from the latched Ctrl button. */
  | { kind: "ctrl"; letter: string }
  /** Let the typing area handle the key. */
  | null;

const BACKSPACE = new TextEncoder().encode("\x7f");

/** Keys that go to the terminal while the typing area is empty, so shell
 * history, completion and full-screen programs work without the key row. */
const PASS_THROUGH: Record<string, PhoneKey | "backspace"> = {
  Escape: "escape",
  Tab: "tab",
  ArrowUp: "up",
  ArrowDown: "down",
  ArrowLeft: "left",
  ArrowRight: "right",
  Backspace: "backspace",
};

export type ComposerKey = {
  key: string;
  shiftKey: boolean;
  composing: boolean;
};

/**
 * Enter sends the line and presses Enter; Shift+Enter starts a new line.
 * With nothing typed, editing and cursor keys reach the terminal instead,
 * and a latched Ctrl turns the next letter into a control key.
 */
export function composerKeyAction(
  event: ComposerKey,
  text: string,
  ctrl: boolean,
): ComposerAction {
  if (event.composing) return null;
  if (event.key === "Enter") return event.shiftKey ? null : { kind: "submit" };
  if (text.length > 0) return null;
  if (ctrl && /^[a-z]$/i.test(event.key)) {
    return { kind: "ctrl", letter: event.key };
  }
  const named = PASS_THROUGH[event.key];
  if (!named) return null;
  return {
    kind: "bytes",
    bytes: named === "backspace" ? BACKSPACE : PHONE_KEYS[named],
  };
}
