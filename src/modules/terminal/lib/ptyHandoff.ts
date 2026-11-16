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
