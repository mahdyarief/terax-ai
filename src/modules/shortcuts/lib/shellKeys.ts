import type { ShortcutId } from "../shortcuts";

/**
 * Shortcuts that stand for a key of the shell itself. Their default is the
 * shell's own chord, which the terminal already sends (readline's Ctrl+P and
 * Ctrl+N). They exist so that chord can move: a user who gives Ctrl+P to Go to
 * file binds history to another chord, and that chord sends the same byte.
 */
export const SHELL_KEYS = {
  "terminal.historyPrev": { key: "p", data: "\x10" },
  "terminal.historyNext": { key: "n", data: "\x0e" },
} as const satisfies Partial<Record<ShortcutId, { key: string; data: string }>>;

export type ShellKeyShortcutId = keyof typeof SHELL_KEYS;

export function isShellKeyShortcut(id: ShortcutId): id is ShellKeyShortcutId {
  return id in SHELL_KEYS;
}

/**
 * The event is the shell's own chord. The app lets it through untouched, so
 * the terminal encodes it as usual (the Kitty keyboard protocol included).
 */
export function isTheShellsOwnChord(
  id: ShellKeyShortcutId,
  e: Pick<KeyboardEvent, "key" | "ctrlKey" | "shiftKey" | "altKey" | "metaKey">,
): boolean {
  return (
    e.ctrlKey &&
    !e.shiftKey &&
    !e.altKey &&
    !e.metaKey &&
    e.key.toLowerCase() === SHELL_KEYS[id].key
  );
}
