import { describe, expect, it, vi } from "vitest";
import { notifyPtyChange, subscribePtyChanges } from "./ptyChanges";

describe("ptyChanges", () => {
  it("tells each subscriber until it unsubscribes", () => {
    const first = vi.fn();
    const second = vi.fn();
    const stopFirst = subscribePtyChanges(first);
    const stopSecond = subscribePtyChanges(second);
    notifyPtyChange();
    stopFirst();
    notifyPtyChange();
    stopSecond();
    notifyPtyChange();
    expect(first).toHaveBeenCalledTimes(1);
    expect(second).toHaveBeenCalledTimes(2);
  });
});
