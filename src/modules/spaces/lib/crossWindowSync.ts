import type { SerializedTab } from "./serialize";
import type { SpaceMeta } from "./store";
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

export type HostChangedPayload = {
  spaceId: string;
  hostWindowLabel: string;
  /** Optional explicit timestamp; falls back to now when omitted. */
  updatedAt?: number;
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
  /** Serialized tab snapshot from the source window. */
  tabs: SerializedTab[];
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
  if (windowLabel !== "main") return null;
  const host = space?.hostWindowLabel ?? "main";
  return host === windowLabel ? null : host;
}

export function applyHostChanged(
  spaces: SpaceMeta[],
  payload: HostChangedPayload,
): SpaceMeta[] {
  let changed = false;
  const next = spaces.map((s) => {
    if (s.id !== payload.spaceId) return s;
    changed = true;
    return {
      ...s,
      hostWindowLabel: payload.hostWindowLabel,
      updatedAt: payload.updatedAt ?? Date.now(),
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
