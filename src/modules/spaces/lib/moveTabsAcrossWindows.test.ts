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
    const emitTo = vi.fn(async () => {
      calls.push("emit");
    });
    const result = await moveTabsAcrossWindows({
      handoffId: "h1",
      tabs,
      ptyIds: [1],
      leafPtyIds: [1, null],
      targetWindow: "space-a",
      spaceId: "s",
      invoke,
      emitTo,
      waitForAttach: vi.fn(async () => {
        calls.push("attached");
        return { handoffId: "h1", replays };
      }),
    });
    expect(emitTo).toHaveBeenCalledWith(
      "space-a",
      "spaces:tabs-moved",
      expect.objectContaining({ leafPtyIds: [1, null] }),
    );
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
      ptyIds: [1],
      leafPtyIds: [1],
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

  it("closes PTYs explicitly when rollback cannot restore the source", async () => {
    const invoke = vi.fn(async (cmd: string) => {
      if (cmd === "pty_prepare_handoff") return checkpoints;
      if (cmd === "pty_detach") throw new Error("source window gone");
      if (cmd === "pty_rollback_handoff") throw new Error("source unavailable");
      return undefined;
    });
    const result = await moveTabsAcrossWindows({
      handoffId: "h4",
      tabs,
      ptyIds: [1],
      leafPtyIds: [1],
      targetWindow: "space-a",
      spaceId: "s",
      invoke,
      emitTo: vi.fn(),
      waitForAttach: vi.fn(),
    });
    expect(result).toEqual({ ok: false, error: "source-unavailable" });
    expect(invoke).toHaveBeenCalledWith("pty_close", { id: 1 });
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
      ptyIds: [1],
      leafPtyIds: [1],
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
    expect(invoke).toHaveBeenCalledWith("pty_rollback_handoff", {
      handoffId: "h3",
      ids: [1],
    });
    expect(emitTo).toHaveBeenCalledTimes(2);
  });

  it("hands rollback replays to the caller so the source can reattach", async () => {
    const invoke = vi.fn(async (cmd: string) => {
      if (cmd === "pty_prepare_handoff") return checkpoints;
      if (cmd === "pty_rollback_handoff") return replays;
      return undefined;
    });
    const onRollback = vi.fn();
    const result = await moveTabsAcrossWindows({
      handoffId: "h3",
      tabs,
      ptyIds: [1],
      leafPtyIds: [1],
      targetWindow: "space-a",
      spaceId: "s",
      invoke,
      emitTo: vi.fn(),
      waitForAttach: vi.fn(async () => ({
        handoffId: "h3",
        replays: [],
        error: "attach failed",
      })),
      onRollback,
    });
    expect(result.ok).toBe(false);
    expect(onRollback).toHaveBeenCalledWith(replays);
  });
});
