import { create } from "zustand";

/** A space whose tabs are on their way to or from its own window. */
export type SpaceTransferKind = "opening" | "returning";

/**
 * A return is answered by the other window, so this window may never hear
 * back (that window refused, or closed). The marker gives up after this long
 * so a row cannot spin forever.
 */
export const SPACE_TRANSFER_TIMEOUT_MS = 30_000;

type SpaceTransfersState = {
  pending: Record<string, SpaceTransferKind>;
  /** `onTimeout` runs if the transfer is still pending when it gives up. */
  begin: (
    spaceId: string,
    kind: SpaceTransferKind,
    onTimeout?: () => void,
  ) => void;
  end: (spaceId: string) => void;
};

const timers = new Map<string, ReturnType<typeof setTimeout>>();

export const useSpaceTransfers = create<SpaceTransfersState>((set) => ({
  pending: {},
  begin: (spaceId, kind, onTimeout) => {
    const previous = timers.get(spaceId);
    if (previous) clearTimeout(previous);
    timers.set(
      spaceId,
      setTimeout(
        () => {
          useSpaceTransfers.getState().end(spaceId);
          onTimeout?.();
        },
        SPACE_TRANSFER_TIMEOUT_MS,
      ),
    );
    set((s) => ({ pending: { ...s.pending, [spaceId]: kind } }));
  },
  end: (spaceId) => {
    const timer = timers.get(spaceId);
    if (timer) clearTimeout(timer);
    timers.delete(spaceId);
    set((s) => {
      if (!(spaceId in s.pending)) return s;
      const { [spaceId]: _done, ...rest } = s.pending;
      return { pending: rest };
    });
  },
}));

/** Toast id shared by a transfer's loading, success and failure toasts. */
export function spaceTransferToastId(spaceId: string): string {
  return `space-transfer-${spaceId}`;
}
