import { describe, expect, it, vi } from "vitest";
import { moveTabsAcrossWindows } from "./moveTabsAcrossWindows";

const tabs = [{ kind: "terminal" as const, tree: { kind: "leaf" as const, active: true } }];
const checkpoints = [
  { ptyId: 1, checkpoint: "7", cols: 80, rows: 24, attachToken: "1" },
];
const replays = [
  {
    ptyId: 1,
    cols: 80,
    rows: 24,
    attachToken: "2",
    leaseEpoch: "1",
    events: [],
  },
];

describe("moveTabsAcrossWindows", () => {
  it("prepares, detaches, emits, waits for attach, then commits", async () => {
    const calls: string[] = [];
    const invoke = vi.fn(async (cmd: string) => {
      calls.push(cmd);
      if (cmd === "pty_prepare_handoff") return checkpoints;
      return undefined;
    });
    const result = await moveTabsAcrossWindows({
      handoffId: "h1",
      tabs,
      targetWindow: "space-a",
      spaceId: "s",
      invoke,
      emitTo: vi.fn(async () => {
        calls.push("emit");
      }),
      waitForAttach: vi.fn(async () => {
        calls.push("attached");
        return { handoffId: "h1", replays };
      }),
    });
    expect(result.ok).toBe(true);
    expect(calls).toEqual([
      "pty_prepare_handoff",
      "pty_detach",
      "emit",
      "attached",
      "pty_commit_handoff",
    ]);
  });

  it("rolls back and does not emit when detach fails", async () => {
    const invoke = vi.fn(async (cmd: string) => {
      if (cmd === "pty_prepare_handoff") return checkpoints;
      if (cmd === "pty_detach") throw new Error("nope");
      return undefined;
    });
    const emitTo = vi.fn();
    const result = await moveTabsAcrossWindows({
      handoffId: "h2",
      tabs,
      targetWindow: "space-a",
      spaceId: "s",
      invoke,
      emitTo,
      waitForAttach: vi.fn(),
    });
    expect(result.ok).toBe(false);
    expect(emitTo).not.toHaveBeenCalled();
    expect(invoke).toHaveBeenCalledWith("pty_rollback_handoff", expect.anything());
  });

  it("rolls back and sends a correction when attach reports an error", async () => {
    const invoke = vi.fn(async (cmd: string) => {
      if (cmd === "pty_prepare_handoff") return checkpoints;
      return undefined;
    });
    const emitTo = vi.fn();
    const result = await moveTabsAcrossWindows({
      handoffId: "h3",
      tabs,
      targetWindow: "space-a",
      spaceId: "s",
      invoke,
      emitTo,
      waitForAttach: vi.fn(async () => ({
        handoffId: "h3",
        replays: [],
        error: "attach failed",
      })),
    });
    expect(result).toEqual({ ok: false, error: "attach failed" });
    expect(invoke).toHaveBeenCalledWith("pty_rollback_handoff", { handoffId: "h3" });
    expect(emitTo).toHaveBeenCalledTimes(2);
  });
});
