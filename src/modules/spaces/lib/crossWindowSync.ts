import type { PtyHandoffCheckpoint, PtyReplayPayload } from "@/modules/terminal/lib/ptyHandoff";
import type { SerializedTab } from "./serialize";
import type { SpaceMeta } from "./store";
import { isHomeWindow } from "./handoff";
import { loadAll } from "./store";
import { mergeSpacesByUpdatedAt } from "./sync";

export { applyDeletedSpace } from "./sync";

export const SPACE_META_CHANGED_EVENT = "terax:space-meta-changed";
export const SPACE_HOST_CHANGED_EVENT = "spaces:host-changed";
export const SPACE_DELETED_EVENT = "terax:space-deleted";
/** A window asks the current host to hand the space back. */
export const SPACE_RECLAIM_REQUEST_EVENT = "spaces:reclaim-request";
/** Serialized tabs are handed from one window to another. */
export const SPACE_TABS_MOVED_EVENT = "spaces:tabs-moved";
export const SPACE_TABS_ATTACHED_EVENT = "spaces:tabs-attached";
/** A space window announces that its cross-window listeners are mounted. */
export const SPACE_WINDOW_READY_EVENT = "spaces:window-ready";

export type WindowReadyPayload = {
  window: string;
  /** Set on the answer to another window's announcement, never answered. */
  reply?: boolean;
};

/**
 * A window announces readiness once, at boot, so a window that opens later
 * (a browser session joining a running host) would never learn about the
 * windows already there. Each of them answers a fresh announcement directly;
 * answers are not answered, so it never loops.
 */
export function shouldAnswerWindowReady(
  payload: WindowReadyPayload | undefined,
  windowLabel: string,
): boolean {
  return Boolean(payload?.window) && payload?.window !== windowLabel && !payload?.reply;
}

export type TabsAttachedPayload = {
  handoffId: string;
  replays: PtyReplayPayload[];
  error?: string;
};

export const HOST_CHANGED_REASONS = {
  windowClosed: "window-closed",
  handoff: "handoff",
} as const;

export type HostChangedReason =
  (typeof HOST_CHANGED_REASONS)[keyof typeof HOST_CHANGED_REASONS];

export type HostChangedPayload = {
  spaceId: string;
  hostWindowLabel: string;
  /** Optional explicit timestamp; falls back to now when omitted. */
  updatedAt?: number;
  /** Only window closure needs tabs restored from disk; handoffs carry tabs separately. */
  reason?: HostChangedReason;
};

export type ReclaimRequestPayload = {
  spaceId: string;
  /** Window that should become the new host / receive the tabs. */
  toWindow: string;
};

export type TabsMovedPayload = {
  spaceId: string;
  fromWindow: string;
  toWindow: string;
  handoffId: string;
  /** Serialized tab snapshot from the source window. */
  tabs: SerializedTab[];
  /** PTY id per serialized leaf, in serialization order; null for cold leaves. */
  leafPtyIds: Array<number | null>;
  /** Live-PTY checkpoints; empty for a rollback/correction. */
  checkpoints: PtyHandoffCheckpoint[];
};

/**
 * "Bring back" is only valid from `main`: the backend's Destroyed handler
 * always reassigns a closed space window back to `main`, so reclaiming into a
 * non-main window would be immediately overwritten.
 */
export function reclaimHost(
  space: Pick<SpaceMeta, "hostWindowLabel"> | undefined,
  windowLabel: string,
): string | null {
  if (!isHomeWindow(windowLabel)) return null;
  const host = space?.hostWindowLabel ?? "main";
  return host === windowLabel ? null : host;
}

export function applyHostChanged(
  spaces: SpaceMeta[],
  payload: HostChangedPayload,
): SpaceMeta[] {
  let changed = false;
  const updatedAt = payload.updatedAt ?? Date.now();
  const next = spaces.map((s) => {
    if (
      s.id !== payload.spaceId ||
      ((s.hostWindowLabel ?? "main") === payload.hostWindowLabel &&
        s.updatedAt === updatedAt)
    ) {
      return s;
    }
    changed = true;
    return {
      ...s,
      hostWindowLabel: payload.hostWindowLabel,
      updatedAt,
    };
  });
  return changed ? next : spaces;
}

export async function loadMergedRegistry(
  remote: SpaceMeta[],
): Promise<SpaceMeta[]> {
  const { spaces } = await loadAll();
  return mergeSpacesByUpdatedAt(spaces, remote);
}
