import type { CSSProperties } from "react";

/**
 * Where the tab stacks lay out the two visible tabs of a split. Dependency
 * free, so every stack can import it without pulling in the tab store.
 */
export type SplitSlot = "primary" | "secondary";

/** Where a tab is shown, or null when it is hidden. */
export function slotOf(
  id: number,
  activeId: number,
  secondaryId: number | null | undefined,
): SplitSlot | null {
  if (id === activeId) return "primary";
  if (secondaryId != null && id === secondaryId) return "secondary";
  return null;
}

/** Half of the gap between the panes, either side of the divider. */
const HALF_GAP = "3px";

const FULL: CSSProperties = { position: "absolute", inset: 0 };
const PRIMARY: CSSProperties = {
  position: "absolute",
  top: 0,
  bottom: 0,
  left: 0,
  width: `calc(var(--terax-split, 50%) - ${HALF_GAP})`,
};
const SECONDARY: CSSProperties = {
  position: "absolute",
  top: 0,
  bottom: 0,
  right: 0,
  left: `calc(var(--terax-split, 50%) + ${HALF_GAP})`,
};

/**
 * Layout of a tab's wrapper inside its stack. The split ratio comes from the
 * `--terax-split` variable on the surface, so dragging the divider restyles
 * the panes without re-rendering any stack.
 */
export function slotStyle(
  slot: SplitSlot | null,
  split: boolean,
): CSSProperties {
  if (!split || slot === null) return FULL;
  return slot === "primary" ? PRIMARY : SECONDARY;
}
