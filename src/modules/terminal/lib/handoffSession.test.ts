import { beforeEach, describe, expect, it } from "vitest";
import {
  matchCheckpointToLeaf,
  queueHandoffReplay,
  takeHandoffReplay,
} from "./handoffSession";
import type {
  PtyHandoffCheckpoint,
  PtyReplayPayload,
} from "./ptyHandoff";

function replay(ptyId: number): PtyReplayPayload {
  return {
    ptyId,
    cols: 80,
    rows: 24,
    attachToken: String(ptyId * 10),
    leaseEpoch: String(ptyId * 100),
    events: [],
  };
}

function checkpoint(ptyId: number): PtyHandoffCheckpoint {
  return {
    ptyId,
    checkpoint: String(ptyId * 1000),
    cols: 80,
    rows: 24,
    attachToken: String(ptyId * 10),
  };
}

describe("handoffSession", () => {
  beforeEach(() => {
    // Drain anything left over from a previous test.
    takeHandoffReplay(1);
    takeHandoffReplay(2);
    takeHandoffReplay(3);
  });

  it("queues a replay until the target leaf consumes it", () => {
    queueHandoffReplay(replay(7));

    expect(takeHandoffReplay(7)?.ptyId).toBe(7);
  });

  it("consumes a queued replay only once", () => {
    queueHandoffReplay(replay(7));

    expect(takeHandoffReplay(7)).not.toBeNull();
    expect(takeHandoffReplay(7)).toBeNull();
  });

  it("returns null when no replay was queued", () => {
    expect(takeHandoffReplay(99)).toBeNull();
  });

  it("pairs checkpoints with leaves by request order", () => {
    const byLeaf = matchCheckpointToLeaf(
      [checkpoint(41), checkpoint(42)],
      [
        { leafId: 11, ptyId: 41 },
        { leafId: 12, ptyId: 42 },
      ],
    );

    expect(byLeaf.get(11)?.ptyId).toBe(41);
    expect(byLeaf.get(12)?.ptyId).toBe(42);
  });

  it("skips a pair whose PTY ids disagree", () => {
    const byLeaf = matchCheckpointToLeaf(
      [checkpoint(41)],
      [{ leafId: 11, ptyId: 42 }],
    );

    expect(byLeaf.size).toBe(0);
  });
});
