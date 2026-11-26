import { create } from "zustand";

/**
 * Progress a program reports with OSC 9;4 (ConEmu, Windows Terminal), for
 * example winget, PowerShell 7.5 or systemd tools. `percent` is null when the
 * program reports no number (indeterminate, or error/pause without one).
 */
export type TerminalProgress = {
  readonly state: "normal" | "error" | "indeterminate" | "paused";
  readonly percent: number | null;
};

const STATES = [null, "normal", "error", "indeterminate", "paused"] as const;

/**
 * The core's progress event: state 0 removes the progress, 1 to 4 are normal,
 * error, indeterminate and paused; percent 255 means none was given.
 */
export function decodeTerminalProgress(
  state: number,
  percent: number,
): TerminalProgress | null {
  const name = STATES[state];
  if (!name) return null;
  const known = percent >= 0 && percent <= 100;
  // A normal progress without a number shows as indeterminate.
  if (name === "normal" && !known)
    return { state: "indeterminate", percent: null };
  return {
    state: name,
    percent: known && name !== "indeterminate" ? percent : null,
  };
}

type TerminalProgressStore = {
  byLeaf: ReadonlyMap<number, TerminalProgress>;
  set: (leafId: number, progress: TerminalProgress | null) => void;
};

const same = (a: TerminalProgress | undefined, b: TerminalProgress | null) =>
  a === undefined
    ? b === null
    : b !== null && a.state === b.state && a.percent === b.percent;

export const useTerminalProgressStore = create<TerminalProgressStore>(
  (set, get) => ({
    byLeaf: new Map(),
    set: (leafId, progress) => {
      if (same(get().byLeaf.get(leafId), progress)) return;
      set((s) => {
        const next = new Map(s.byLeaf);
        if (progress) next.set(leafId, progress);
        else next.delete(leafId);
        return { byLeaf: next };
      });
    },
  }),
);

const RANK: Record<TerminalProgress["state"], number> = {
  error: 3,
  paused: 2,
  normal: 1,
  indeterminate: 0,
};

/**
 * The progress a tab shows for its panes: an error first, then a paused one,
 * then the one with the least done, so the bar never runs ahead of the work.
 */
export function selectTabProgress(
  s: Pick<TerminalProgressStore, "byLeaf">,
  leafIds: readonly number[],
): TerminalProgress | null {
  let best: TerminalProgress | null = null;
  for (const id of leafIds) {
    const p = s.byLeaf.get(id);
    if (!p) continue;
    if (
      !best ||
      RANK[p.state] > RANK[best.state] ||
      (RANK[p.state] === RANK[best.state] &&
        (p.percent ?? 101) < (best.percent ?? 101))
    )
      best = p;
  }
  return best;
}
