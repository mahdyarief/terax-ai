import { LazyStore } from "@tauri-apps/plugin-store";
import type { WorkspaceEnv } from "@/modules/workspace";
import type { SerializedTab } from "./serialize";

export type SpaceMeta = {
  id: string;
  name: string;
  root: string | null;
  env: WorkspaceEnv;
  /** Opt-in accent, index into SPACE_COLORS. Undefined = theme primary. */
  color?: number;
  /** Webview label hosting this space. undefined/null => "main" for compat. */
  hostWindowLabel?: string | null;
  createdAt: number;
  updatedAt: number;
};

export type SpaceState = {
  tabs: SerializedTab[];
  activeTabIndex: number;
};

const STORE_PATH = "terax-spaces.json";
const KEY_SPACES = "spaces";
const KEY_ACTIVE = "activeId";
const KEY_ACTIVE_BY_WINDOW = "activeIdByWindow";
const STATE_PREFIX = "state:";
const stateKey = (id: string) => `${STATE_PREFIX}${id}`;

const store = new LazyStore(STORE_PATH, { defaults: {}, autoSave: 500 });

export type ActiveSpaceByWindow = Record<string, string | null>;

export function getActiveSpaceIdForWindow(
  map: ActiveSpaceByWindow | null | undefined,
  windowLabel: string,
): string | null {
  return map?.[windowLabel] ?? null;
}

export type LoadedSpaces = {
  spaces: SpaceMeta[];
  activeId: string | null;
  activeIdByWindow: ActiveSpaceByWindow;
  states: Map<string, SpaceState>;
};

export function migrateSpaceMeta(space: SpaceMeta): SpaceMeta {
  return {
    ...space,
    hostWindowLabel: space.hostWindowLabel ?? "main",
  };
}

export async function loadAll(): Promise<LoadedSpaces> {
  const entries = await store.entries();
  let spaces: SpaceMeta[] = [];
  let activeId: string | null = null;
  let activeIdByWindow: ActiveSpaceByWindow = {};
  const states = new Map<string, SpaceState>();
  for (const [k, v] of entries) {
    if (k === KEY_SPACES) spaces = ((v as SpaceMeta[]) ?? []).map(migrateSpaceMeta);
    else if (k === KEY_ACTIVE) activeId = (v as string | null) ?? null;
    else if (k === KEY_ACTIVE_BY_WINDOW) activeIdByWindow = (v as ActiveSpaceByWindow) ?? {};
    else if (k.startsWith(STATE_PREFIX)) {
      states.set(k.slice(STATE_PREFIX.length), v as SpaceState);
    }
  }
  return { spaces, activeId, activeIdByWindow, states };
}

export async function saveActiveIdForWindow(
  windowLabel: string,
  id: string | null,
): Promise<void> {
  const current = (await store.get<ActiveSpaceByWindow>(KEY_ACTIVE_BY_WINDOW)) ?? {};
  await store.set(KEY_ACTIVE_BY_WINDOW, { ...current, [windowLabel]: id });
  await store.save();
}

export async function saveSpacesList(spaces: SpaceMeta[]): Promise<void> {
  await store.set(KEY_SPACES, spaces);
  await store.save();
}

export async function saveActiveId(id: string | null): Promise<void> {
  await store.set(KEY_ACTIVE, id);
  await store.save();
}

export async function saveState(id: string, state: SpaceState): Promise<void> {
  await store.set(stateKey(id), state);
  await store.save();
}

export async function deleteSpaceData(id: string): Promise<void> {
  await store.delete(stateKey(id));
  await store.save();
}

export function newSpaceId(): string {
  return `sp-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`;
}
