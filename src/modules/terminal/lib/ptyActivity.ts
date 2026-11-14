import { create } from "zustand";

/**
 * How long a pty stays "running" after its last output chunk before it is
 * considered idle. The debounce keeps the indicator from blinking on every
 * keystroke of output.
 */
export const PTY_INACTIVITY_MS = 1200;

type PtyActivityStore = {
  /** pty ids that are currently producing output. */
  running: Set<number>;
  /** Record that a pty produced a chunk; resets the inactivity timer. */
  markRunning: (ptyId: number) => void;
  /** Force a pty idle immediately, cancelling its pending timer. */
  markIdle: (ptyId: number) => void;
  /** Remove a pty's activity tracking (e.g. on session close/exit). */
  clear: (ptyId: number) => void;
};

const idleTimers = new Map<number, ReturnType<typeof setTimeout>>();

function cancelIdle(ptyId: number): void {
  const t = idleTimers.get(ptyId);
  if (t !== undefined) {
    clearTimeout(t);
    idleTimers.delete(ptyId);
  }
}

type Setter = (
  partial: (state: PtyActivityStore) => Partial<PtyActivityStore>,
) => void;

function removeRunning(set: Setter, ptyId: number): void {
  set((s) => {
    if (!s.running.has(ptyId)) return s;
    const next = new Set(s.running);
    next.delete(ptyId);
    return { running: next };
  });
}

export const usePtyActivityStore = create<PtyActivityStore>((set, get) => ({
  running: new Set<number>(),
  markRunning: (ptyId) => {
    cancelIdle(ptyId);
    if (!get().running.has(ptyId)) {
      set((s) => {
        const next = new Set(s.running);
        next.add(ptyId);
        return { running: next };
      });
    }
    idleTimers.set(
      ptyId,
      setTimeout(() => {
        idleTimers.delete(ptyId);
        removeRunning(set, ptyId);
      }, PTY_INACTIVITY_MS),
    );
  },
  markIdle: (ptyId) => {
    cancelIdle(ptyId);
    removeRunning(set, ptyId);
  },
  clear: (ptyId) => {
    cancelIdle(ptyId);
    removeRunning(set, ptyId);
  },
}));

/**
 * True if any of the given pty ids is currently considered running. Designed
 * to be used as a zustand selector; it returns a primitive boolean so unrelated
 * tabs don't re-render when some other pty's activity changes.
 */
export function isPtyRunning(
  state: { running: Set<number> },
  ptyIds: readonly number[],
): boolean {
  const { running } = state;
  for (const id of ptyIds) {
    if (running.has(id)) return true;
  }
  return false;
}
