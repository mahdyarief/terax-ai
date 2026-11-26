import { IS_MAC } from "@/lib/platform";
import {
  getBindingTokens,
  type KeyBinding,
  type Shortcut,
  type ShortcutId,
} from "../shortcuts";
import { isShellKeyShortcut } from "./shellKeys";

const chordOf = (b: KeyBinding): string =>
  [!!b.ctrl, !!b.shift, !!b.alt, !!b.meta, b.key.toLowerCase()].join("|");

const spell = (b: KeyBinding): string =>
  getBindingTokens(b).join(IS_MAC ? "" : "+");

/** A plain Ctrl+letter is a control byte that shells and TUIs read. */
const isControlByte = (b: KeyBinding): boolean =>
  !!b.ctrl && !b.shift && !b.alt && !b.meta && /^[a-z]$/i.test(b.key);

/**
 * Notes for the Settings list about bindings the user recorded:
 * - a chord two shortcuts share. The resolver tries recorded bindings first,
 *   then defaults, each in registry order, so the first of those owns it.
 * - a plain Ctrl+letter outside the editor, which terminals then may not get.
 * Defaults are left alone: the ones that share a chord do it on purpose (on
 * macOS Command+K clears a terminal and opens the palette elsewhere).
 */
export function shortcutNotes(
  shortcuts: readonly Shortcut[],
  user: Partial<Record<ShortcutId, KeyBinding[]>>,
): Partial<Record<ShortcutId, string>> {
  const listed = shortcuts.filter((s) => s.id !== "tab.selectByIndex");
  const byPriority = [
    ...listed.filter((s) => user[s.id] !== undefined),
    ...listed.filter((s) => user[s.id] === undefined),
  ];
  const owners = new Map<string, Shortcut[]>();
  for (const s of byPriority)
    for (const b of user[s.id] ?? s.defaultBindings) {
      const key = chordOf(b);
      const list = owners.get(key) ?? [];
      if (!list.includes(s)) list.push(s);
      owners.set(key, list);
    }

  const notes: Partial<Record<ShortcutId, string>> = {};
  for (const s of listed) {
    const bindings = user[s.id] ?? s.defaultBindings;
    for (const b of bindings) {
      const list = owners.get(chordOf(b)) ?? [];
      if (list.length < 2) continue;
      if (!list.some((o) => user[o.id] !== undefined)) continue;
      const owner = list[0];
      notes[s.id] =
        owner === s
          ? `${spell(b)} is also set for “${list[1].label}”, which no longer runs from it.`
          : `“${owner.label}” uses ${spell(b)}, so this one does not run. Pick another key.`;
      break;
    }
    if (notes[s.id] || user[s.id] === undefined) continue;
    if (s.group === "Editor" || isShellKeyShortcut(s.id)) continue;
    const control = bindings.find(isControlByte);
    if (control)
      notes[s.id] =
        `${spell(control)} is also a terminal key (shells and TUIs read it), so terminals may not receive it.`;
  }
  return notes;
}
