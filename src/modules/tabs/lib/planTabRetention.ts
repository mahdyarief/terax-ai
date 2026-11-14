import type { Tab } from "./useTabs";

/** Most-recently-used tabs the host keeps mounted, alongside modified tabs. */
export const MRU_TAB_BUDGET = 3;

/**
 * Plans which tabs fall outside the MRU budget and should be marked `cold`
 * (unmounted placeholder). Returns a new array only when a `cold` flag would
 * change; returns `null` to preserve array identity and avoid render loops.
 *
 * - `keep` = the first `MRU_TAB_BUDGET` of `recentIds`, plus `activeId`, plus
 *   every `editor` tab with `dirty === true`.
 * - A tab becomes cold when it is not kept AND it has been used before
 *   (`recentIds` includes it) OR it was already cold (e.g. a restored tab never
 *   activated). A never-used warm tab is left alone.
 * - A `terminal` tab is never cold: `TerminalStack` ignores `cold`, and cooling
 *   it would drop a live session.
 */
export function planTabRetention(
  tabs: readonly Tab[],
  recentIds: readonly number[],
  activeId: number,
): Tab[] | null {
  const keep = new Set<number>(recentIds.slice(0, MRU_TAB_BUDGET));
  keep.add(activeId);
  for (const t of tabs) {
    if (t.kind === "editor" && t.dirty) keep.add(t.id);
  }

  let changed = false;
  const next = tabs.map((t) => {
    const isCold = t.cold === true;
    const eligible =
      t.kind === "editor" || t.kind === "markdown" || t.kind === "preview";
    const shouldBeCold =
      eligible && !keep.has(t.id) && (recentIds.includes(t.id) || isCold);
    if (shouldBeCold !== isCold) {
      changed = true;
      return { ...t, cold: shouldBeCold };
    }
    return t;
  });

  return changed ? next : null;
}
