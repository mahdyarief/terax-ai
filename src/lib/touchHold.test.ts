import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { startTouchHold, TOUCH_HOLD_MS } from "./touchHold";

function fakeTarget() {
  const listeners = new Set<(event: Event) => void>();
  return {
    addEventListener: vi.fn((_type: string, fn: (event: Event) => void) => {
      listeners.add(fn);
    }),
    removeEventListener: vi.fn((_type: string, fn: (event: Event) => void) => {
      listeners.delete(fn);
    }),
    /** Dispatch a touchmove and report whether panning was blocked. */
    move() {
      const event = { cancelable: true, preventDefault: vi.fn() };
      for (const fn of listeners) fn(event as unknown as Event);
      return event.preventDefault.mock.calls.length > 0;
    },
    count: () => listeners.size,
  };
}

describe("startTouchHold", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("lets the list scroll until the finger has rested, then blocks panning", () => {
    const target = fakeTarget();
    const hold = startTouchHold(target as never);
    expect(target.addEventListener).toHaveBeenCalledWith(
      "touchmove",
      expect.any(Function),
      { passive: false },
    );
    vi.advanceTimersByTime(TOUCH_HOLD_MS - 1);
    expect(hold.armed).toBe(false);
    expect(target.move()).toBe(false);
    vi.advanceTimersByTime(1);
    expect(hold.armed).toBe(true);
    expect(target.move()).toBe(true);
  });

  it("never arms once cancelled, and stops listening", () => {
    const target = fakeTarget();
    const hold = startTouchHold(target as never);
    hold.cancel();
    vi.advanceTimersByTime(TOUCH_HOLD_MS * 2);
    expect(hold.armed).toBe(false);
    expect(target.count()).toBe(0);
  });
});
