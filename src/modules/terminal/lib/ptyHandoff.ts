export type PtyHandoffCheckpoint = {
  ptyId: number;
  checkpoint: string;
  cols: number;
  rows: number;
  attachToken: string;
};

export type PtyReplayEvent =
  | { kind: "output"; data: string }
  | { kind: "resize"; cols: number; rows: number };

export type PtyReplayPayload = {
  ptyId: number;
  cols: number;
  rows: number;
  attachToken: string;
  leaseEpoch: string;
  /**
   * Stream offset of the first live data frame after `events`. The backend
   * rebases the PTY's output stream here when it snapshots the history, so the
   * receiver must start counting from it, not from zero.
   */
  streamOffset?: number;
  events: PtyReplayEvent[];
};

export type PtyAttachmentRef = {
  ptyId: number;
  checkpoint: string;
  cols: number;
  rows: number;
};

export type PtyLease = {
  ptyId: number;
  attachToken: string;
  leaseEpoch: string;
};

type IdPayload = { handoffId: string; ids: number[] };

// Per-window random start so two windows handing off in the same millisecond
// do not mint the same id.
let handoffSeq = Math.floor(Math.random() * 1000);

/**
 * A fresh handoff id. The backend parses it as a `u64` (`parse_handoff`), so
 * it must be a plain decimal integer: `ms * 1000 + seq` stays well inside
 * `Number.MAX_SAFE_INTEGER` and increases within a window.
 */
export function nextHandoffId(now: number = Date.now()): string {
  handoffSeq = (handoffSeq + 1) % 1000;
  return String(Math.trunc(now) * 1000 + handoffSeq);
}

export function buildPrepareHandoff(
  handoffId: string,
  ids: number[],
  to: string,
): IdPayload & { to: string } {
  return { handoffId, ids, to };
}

export function buildDetachHandoff(
  handoffId: string,
  ids: number[],
): IdPayload {
  return { handoffId, ids };
}

export function buildAttachHandoff(
  handoffId: string,
  ids: number[],
): IdPayload {
  return { handoffId, ids };
}

export function buildCommitHandoff(
  handoffId: string,
  leases: PtyLease[],
): { handoffId: string; leases: PtyLease[] } {
  return { handoffId, leases };
}

export function buildRollbackHandoff(
  handoffId: string,
  ids: number[],
): IdPayload {
  return { handoffId, ids };
}

export function decodeReplayOutput(data: string): Uint8Array {
  const binary = atob(data);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
  return bytes;
}

export function applyReplayEvents(
  model: {
    write(bytes: Uint8Array): void;
    resize(cols: number, rows: number): void;
  },
  events: PtyReplayEvent[],
): void {
  for (const event of events) {
    if (event.kind === "output") {
      model.write(decodeReplayOutput(event.data));
      continue;
    }
    if (
      !Number.isSafeInteger(event.cols) ||
      !Number.isSafeInteger(event.rows) ||
      event.cols <= 0 ||
      event.rows <= 0
    ) {
      throw new Error("invalid replay resize");
    }
    model.resize(event.cols, event.rows);
  }
}
