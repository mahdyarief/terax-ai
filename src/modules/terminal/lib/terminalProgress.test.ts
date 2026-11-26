import { beforeEach, describe, expect, it } from "vitest";
import {
  decodeTerminalProgress,
  selectTabProgress,
  useTerminalProgressStore,
} from "./terminalProgress";

describe("decodeTerminalProgress", () => {
  it("names the OSC 9;4 states", () => {
    expect(decodeTerminalProgress(0, 255)).toBeNull();
    expect(decodeTerminalProgress(1, 42)).toEqual({
      state: "normal",
      percent: 42,
    });
    expect(decodeTerminalProgress(2, 80)).toEqual({
      state: "error",
      percent: 80,
    });
    expect(decodeTerminalProgress(2, 255)).toEqual({
      state: "error",
      percent: null,
    });
    expect(decodeTerminalProgress(3, 50)).toEqual({
      state: "indeterminate",
      percent: null,
    });
    expect(decodeTerminalProgress(4, 255)).toEqual({
      state: "paused",
      percent: null,
    });
  });

  it("shows a normal progress without a number as indeterminate", () => {
    expect(decodeTerminalProgress(1, 255)).toEqual({
      state: "indeterminate",
      percent: null,
    });
  });

  it("ignores states it does not know", () => {
    expect(decodeTerminalProgress(9, 10)).toBeNull();
  });
});

describe("terminal progress store", () => {
  beforeEach(() => {
    useTerminalProgressStore.setState({ byLeaf: new Map() });
  });

  it("keeps the same map when nothing changes, so tabs do not re-render", () => {
    const { set } = useTerminalProgressStore.getState();
    set(1, { state: "normal", percent: 10 });
    const before = useTerminalProgressStore.getState().byLeaf;
    set(1, { state: "normal", percent: 10 });
    set(2, null);
    expect(useTerminalProgressStore.getState().byLeaf).toBe(before);
    set(1, null);
    expect(useTerminalProgressStore.getState().byLeaf.size).toBe(0);
  });
});

describe("selectTabProgress", () => {
  const byLeaf = new Map([
    [1, { state: "normal", percent: 80 } as const],
    [2, { state: "normal", percent: 30 } as const],
    [3, { state: "indeterminate", percent: null } as const],
    [4, { state: "error", percent: 50 } as const],
    [5, { state: "paused", percent: null } as const],
  ]);

  it("shows an error first, then a pause, then the least done", () => {
    expect(selectTabProgress({ byLeaf }, [1, 2, 3, 4, 5])?.state).toBe("error");
    expect(selectTabProgress({ byLeaf }, [1, 2, 3, 5])?.state).toBe("paused");
    expect(selectTabProgress({ byLeaf }, [1, 2, 3])).toEqual({
      state: "normal",
      percent: 30,
    });
    expect(selectTabProgress({ byLeaf }, [3])?.state).toBe("indeterminate");
  });

  it("is null for a tab whose panes report nothing", () => {
    expect(selectTabProgress({ byLeaf }, [9])).toBeNull();
  });
});
