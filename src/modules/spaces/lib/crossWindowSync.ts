import type { SpaceMeta } from "./store";
import { loadAll } from "./store";
import { mergeSpacesByUpdatedAt } from "./sync";

export { applyDeletedSpace } from "./sync";

export const SPACE_META_CHANGED_EVENT = "terax:space-meta-changed";
export const SPACE_HOST_CHANGED_EVENT = "spaces:host-changed";
export const SPACE_DELETED_EVENT = "terax:space-deleted";

export type HostChangedPayload = {
  spaceId: string;
  hostWindowLabel: string;
  /** Optional explicit timestamp; falls back to now when omitted. */
  updatedAt?: number;
};

/**
 * Pure re-map of the registry: only the matching space takes the new host.
 * Returns the same array reference when the space is unknown so callers can
 * detect a no-op and skip the disk write.
 */
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
