import type { AdaptedGhosttyTerminalModel } from "@/modules/terminal/ghostty/AdaptedGhosttyTerminalModel";
import { KEY_MAP } from "@/modules/terminal/ghostty/input/keyTable";
import { buildKeyEvent } from "@/modules/terminal/ghostty/input/keyRouting";
import { encodeTerminalPaste } from "@/modules/terminal/ghostty/input/terminalInputEncoding";
import { KeyAction } from "@terax/ghostty-core/protocol";

const encoder = new TextEncoder();

/** Bracketed paste is the mode applications ask for by number. */
const MODE_BRACKETED_PASTE = 2004;

/**
 * A key press, encoded by Ghostty's own encoder so a browser sends exactly
 * what the desktop would: arrows in the application cursor mode, Ctrl+C as
 * an interrupt, a Kitty-protocol application seeing key releases binary.
 *
 * A soft keyboard often reports `Unidentified` with no code; its text is sent
 * as plain bytes instead.
 */
export function encodeKeydown(
  model: AdaptedGhosttyTerminalModel,
  event: KeyboardEvent,
  options: { ctrl?: boolean; alt?: boolean } = {},
): Uint8Array | null {
  const key = KEY_MAP[event.code];
  if (key === undefined) {
    return event.key.length === 1 ? encoder.encode(event.key) : null;
  }
  const source: KeyboardEvent = {
    ...event,
    ctrlKey: event.ctrlKey || options.ctrl === true,
    altKey: event.altKey || options.alt === true,
  };
  const flags = model.kittyKeyboardFlags();
  const data = buildKeyEvent(
    source,
    key,
    KeyAction.PRESS,
    flags,
    event.key.length === 1 ? event.key : undefined,
  );
  const encoded = model.encodeKey(data);
  return encoded.byteLength > 0 ? encoded : null;
}

/** The Escape sequence for a named key, for the phone's key row. */
export const PHONE_KEYS = {
  escape: encoder.encode("\x1b"),
  tab: encoder.encode("\t"),
  enter: encoder.encode("\r"),
  up: encoder.encode("\x1b[A"),
  down: encoder.encode("\x1b[B"),
  right: encoder.encode("\x1b[C"),
  left: encoder.encode("\x1b[D"),
} as const;

export type PhoneKey = keyof typeof PHONE_KEYS;

/** A paste as the application expects it, bracketed when it asked for that. */
export function encodePaste(
  model: AdaptedGhosttyTerminalModel,
  text: string,
): Uint8Array {
  return encoder.encode(
    encodeTerminalPaste(text, model.mode(MODE_BRACKETED_PASTE)),
  );
}

export function encodeCtrlLetter(letter: string): Uint8Array {
  return encoder.encode(
    String.fromCharCode(letter.toUpperCase().charCodeAt(0) & 0x1f),
  );
}
