/** How long a finger must rest on a row before moving it starts a drag. */
export const TOUCH_HOLD_MS = 250;

export type TouchHold = {
  /** True once the finger has rested long enough for a drag to begin. */
  readonly armed: boolean;
  cancel: () => void;
};

type TouchTarget = Pick<HTMLElement, "addEventListener" | "removeEventListener">;

/**
 * Lets a finger both scroll a list and drag its rows. A touch that moves
 * straight away scrolls as usual; one that rests for `holdMs` first becomes a
 * drag, and from then on the browser's own panning is blocked so it does not
 * cancel the pointer mid-drag.
 */
export function startTouchHold(
  target: TouchTarget,
  holdMs = TOUCH_HOLD_MS,
): TouchHold {
  let armed = false;
  const blockPan = (event: Event) => {
    if (armed && event.cancelable) event.preventDefault();
  };
  // Must be non-passive, or preventDefault is ignored.
  target.addEventListener("touchmove", blockPan, { passive: false });
  const timer = setTimeout(() => {
    armed = true;
  }, holdMs);
  return {
    get armed() {
      return armed;
    },
    cancel: () => {
      clearTimeout(timer);
      armed = false;
      target.removeEventListener("touchmove", blockPan);
    },
  };
}
