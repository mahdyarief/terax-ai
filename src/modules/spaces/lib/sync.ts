import type { SpaceMeta } from "./store";

export function mergeSpacesByUpdatedAt(
  local: SpaceMeta[],
  remote: SpaceMeta[],
): SpaceMeta[] {
  const remoteById = new Map(remote.map((s) => [s.id, s]));
  const seen = new Set<string>();
  const merged = local.map((l) => {
    seen.add(l.id);
    const r = remoteById.get(l.id);
    if (!r) return l;
    return r.updatedAt > l.updatedAt ? r : l;
  });
  for (const r of remote) {
    if (!seen.has(r.id)) merged.push(r);
  }
  return merged;
}

export function applyDeletedSpace<T extends { id: string }>(
  spaces: T[],
  id: string,
): T[] {
  return spaces.filter((s) => s.id !== id);
}
