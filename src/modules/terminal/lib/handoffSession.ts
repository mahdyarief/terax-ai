import type {
  PtyHandoffCheckpoint,
  PtyReplayPayload,
} from "@/modules/terminal/lib/ptyHandoff";

const pendingByPty = new Map<number, PtyReplayPayload>();

/** Queue a replay for a handed-off PTY until its target leaf mounts. */
export function queueHandoffReplay(replay: PtyReplayPayload): void {
  pendingByPty.set(replay.ptyId, replay);
}

/** Consume a queued replay once, for the leaf that now owns the PTY. */
export function takeHandoffReplay(ptyId: number): PtyReplayPayload | null {
  const replay = pendingByPty.get(ptyId) ?? null;
  if (replay) pendingByPty.delete(ptyId);
  return replay;
}

/**
 * Pair serialized checkpoints with the leaves that own their PTYs. Checkpoint
 * order matches the `pty_prepare_handoff` request order, so the i-th checkpoint
 * belongs to the i-th live terminal leaf.
 */
export function matchCheckpointToLeaf(
  checkpoints: PtyHandoffCheckpoint[],
  ptyIdsByLeaf: Array<{ leafId: number; ptyId: number }>,
): Map<number, PtyHandoffCheckpoint> {
  const byLeaf = new Map<number, PtyHandoffCheckpoint>();
  checkpoints.forEach((checkpoint, i) => {
    const match = ptyIdsByLeaf[i];
    if (match && match.ptyId === checkpoint.ptyId) {
      byLeaf.set(match.leafId, checkpoint);
    }
  });
  return byLeaf;
}
