import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  SPACE_TRANSFER_TIMEOUT_MS,
  useSpaceTransfers,
} from "./spaceTransfers";

describe("useSpaceTransfers", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => {
    useSpaceTransfers.getState().end("a");
    useSpaceTransfers.getState().end("b");
    vi.useRealTimers();
  });

  it("tracks a transfer until it ends", () => {
    useSpaceTransfers.getState().begin("a", "opening");
    useSpaceTransfers.getState().begin("b", "returning");
    expect(useSpaceTransfers.getState().pending).toEqual({
      a: "opening",
      b: "returning",
    });
    useSpaceTransfers.getState().end("a");
    expect(useSpaceTransfers.getState().pending).toEqual({ b: "returning" });
  });

  it("gives up on a transfer nobody answered", () => {
    useSpaceTransfers.getState().begin("a", "returning");
    vi.advanceTimersByTime(SPACE_TRANSFER_TIMEOUT_MS - 1);
    expect(useSpaceTransfers.getState().pending.a).toBe("returning");
    vi.advanceTimersByTime(1);
    expect(useSpaceTransfers.getState().pending.a).toBeUndefined();
  });

  it("reports a timeout once, and not after the transfer ended", () => {
    const late = vi.fn();
    useSpaceTransfers.getState().begin("a", "returning", late);
    vi.advanceTimersByTime(SPACE_TRANSFER_TIMEOUT_MS);
    expect(late).toHaveBeenCalledTimes(1);
    const done = vi.fn();
    useSpaceTransfers.getState().begin("b", "returning", done);
    useSpaceTransfers.getState().end("b");
    vi.advanceTimersByTime(SPACE_TRANSFER_TIMEOUT_MS);
    expect(done).not.toHaveBeenCalled();
  });

  it("restarts the timeout when the same space begins again", () => {
    useSpaceTransfers.getState().begin("a", "opening");
    vi.advanceTimersByTime(SPACE_TRANSFER_TIMEOUT_MS - 10);
    useSpaceTransfers.getState().begin("a", "returning");
    vi.advanceTimersByTime(20);
    expect(useSpaceTransfers.getState().pending.a).toBe("returning");
  });
});
