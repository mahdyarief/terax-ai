import { nextActiveInSpace, type Tab } from "./useTabs";

export { type SplitSlot, slotOf, slotStyle } from "./splitLayout";

/**
 * Side-by-side view: the active tab on the left, one more tab of the same
 * space on the right.
 *
 * Every tab stays mounted where it always is; a split only changes where the
 * two visible tabs are laid out. Moving a tab into a second React subtree
 * would remount it, which closes a terminal's shell and drops an editor's
 * unsaved text and undo history.
 */
export type SplitState = { activeId: number; secondaryId: number | null };

/** Share of the width the left pane takes, kept away from the edges. */
export const SPLIT_MIN = 0.2;
export const SPLIT_MAX = 0.8;
export const SPLIT_DEFAULT = 0.5;

export function clampSplitRatio(ratio: number): number {
  if (!Number.isFinite(ratio)) return SPLIT_DEFAULT;
  return Math.min(SPLIT_MAX, Math.max(SPLIT_MIN, ratio));
}

/**
 * Opens `tabId` on the right. The active tab stays on the left; when the tab
 * being split is the active one, its neighbour in the space takes the left.
 * Null when there is no second tab to put beside it.
 */
export function planSplitRight(
  tabs: Tab[],
  activeId: number,
  tabId: number,
): SplitState | null {
  const tab = tabs.find((t) => t.id === tabId);
  if (!tab) return null;
  if (tabId !== activeId) {
    const active = tabs.find((t) => t.id === activeId);
    if (!active || active.spaceId !== tab.spaceId) return null;
    return { activeId, secondaryId: tabId };
  }
  const left = nextActiveInSpace(tabs, tabId);
  return left === null ? null : { activeId: left, secondaryId: tabId };
}

/**
 * The right-hand tab that may still be shown, or null. It has to exist, be
 * in the active tab's space, and not be the active tab itself (which happens
 * when closing the left tab hands the focus to the right one).
 */
export function reconcileSecondary(
  tabs: Tab[],
  activeId: number,
  secondaryId: number | null,
): number | null {
  if (secondaryId === null || secondaryId === activeId) return null;
  const secondary = tabs.find((t) => t.id === secondaryId);
  const active = tabs.find((t) => t.id === activeId);
  if (!secondary || !active || secondary.spaceId !== active.spaceId)
    return null;
  return secondaryId;
}
