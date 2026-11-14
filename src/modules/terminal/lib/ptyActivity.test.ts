import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  isPtyRunning,
  PTY_INACTIVITY_MS,
  usePtyActivityStore,
} from "./ptyActivity";

const state = () => usePtyActivityStore.getState();
const runningFor = (ptyIds: readonly number[]) =>
  isPtyRunning(state(), ptyIds);

const tracked = [1, 2, 3];

beforeEach(() => {
  vi.useFakeTimers();
  for (const id of tracked) state().clear(id);
  usePtyActivityStore.setState({ running: new Set<number>() });
});

afterEach(() => {
  for (const id of tracked) state().clear(id);
  vi.useRealTimers();
});

describe("pty activity", () => {
  it("starts null (no pty running)", () => {
    expect(runningFor([1])).toBe(false);
    expect(state().running.size).toBe(0);
  });

  it("goes running on output, then idle after the inactivity timeout", () => {
    state().markRunning(1);
    expect(runningFor([1])).toBe(true);

    vi.advanceTimersByTime(PTY_INACTIVITY_MS - 1);
    expect(runningFor([1])).toBe(true);

    vi.advanceTimersByTime(1);
    expect(runningFor([1])).toBe(false);
  });

  it("debounces: each new chunk restarts the timer without blinking", () => {
    state().markRunning(1);
    vi.advanceTimersByTime(PTY_INACTIVITY_MS - 1);
    state().markRunning(1);
    vi.advanceTimersByTime(PTY_INACTIVITY_MS - 1);
    expect(runningFor([1])).toBe(true);
    vi.advanceTimersByTime(1);
    expect(runningFor([1])).toBe(false);
  });

  it("tracks ptys independently and reports a tab running if any pty is", () => {
    state().markRunning(1);
    state().markRunning(2);
    expect(runningFor([3, 2])).toBe(true);
    expect(runningFor([3])).toBe(false);

    state().markIdle(1);
    expect(runningFor([1])).toBe(false);
    expect(runningFor([1, 2])).toBe(true);
  });

  it("clear cancels the pending timer, so the pty never becomes running again", () => {
    state().markRunning(1);
    expect(runningFor([1])).toBe(true);

    state().clear(1);
    expect(runningFor([1])).toBe(false);

    vi.advanceTimersByTime(PTY_INACTIVITY_MS * 2);
    expect(runningFor([1])).toBe(false);
    expect(vi.getTimerCount()).toBe(0);
  });
});
