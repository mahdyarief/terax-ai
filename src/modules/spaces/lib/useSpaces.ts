import { create } from "zustand";
import { usePreferencesStore } from "@/modules/settings/preferences";
import {
  parseWorkspaceScopeKey,
  type WorkspaceEnv,
} from "@/modules/workspace";
import {
  deleteSpaceData,
  newSpaceId,
  saveActiveId,
  saveActiveIdForWindow,
  saveSpacesList,
  type SpaceMeta,
} from "./store";
import {
  SPACE_DELETED_EVENT,
  SPACE_META_CHANGED_EVENT,
} from "./crossWindowSync";

async function emitMetaChanged(): Promise<void> {
  try {
    const { emit } = await import("@tauri-apps/api/event");
    await emit(SPACE_META_CHANGED_EVENT);
  } catch {
    // Non-Tauri/test environments have no event bus.
  }
}

async function emitDeleted(id: string): Promise<void> {
  try {
    const { emit } = await import("@tauri-apps/api/event");
    await emit(SPACE_DELETED_EVENT, { spaceId: id });
  } catch {
    // Non-Tauri/test environments have no event bus.
  }
}

async function persistSpaces(spaces: SpaceMeta[]): Promise<void> {
  await saveSpacesList(spaces);
  await emitMetaChanged();
}

export function selectHostedSpaces(
  spaces: SpaceMeta[],
  windowLabel: string,
): SpaceMeta[] {
  return spaces.filter(
    (s) => (s.hostWindowLabel ?? "main") === windowLabel,
  );
}

type CreateInput = {
  id?: string;
  name: string;
  root: string | null;
  env?: WorkspaceEnv;
};

type State = {
  spaces: SpaceMeta[];
  activeId: string | null;
  hydrated: boolean;
  // Webview label for this window; set on boot. "main" is the default.
  windowLabel: string;
  // Per-space active tab index loaded from disk, so persistence preserves it
  // for spaces the user never visits this session.
  initialActiveIndex: Record<string, number>;
  hydrate: (
    spaces: SpaceMeta[],
    activeId: string | null,
    initialActiveIndex?: Record<string, number>,
  ) => void;
  create: (input: CreateInput) => SpaceMeta;
  rename: (id: string, name: string) => void;
  setEnv: (id: string, env: WorkspaceEnv) => void;
  setColor: (id: string, color: number | undefined) => void;
  reorder: (orderedIds: string[]) => void;
  remove: (id: string) => string | null;
  setActive: (id: string) => void;
  assignSpaceToWindow: (
    id: string,
    hostWindowLabel: string | null,
  ) => Promise<void>;
};

export const useSpaces = create<State>((set, get) => ({
  spaces: [],
  activeId: null,
  hydrated: false,
  windowLabel: "main",
  initialActiveIndex: {},

  hydrate: (spaces, activeId, initialActiveIndex = {}) => {
    set({ spaces, activeId, initialActiveIndex, hydrated: true });
  },

  create: (input) => {
    const now = Date.now();
    const meta: SpaceMeta = {
      id: input.id ?? newSpaceId(),
      name: input.name,
      root: input.root,
      env:
        input.env ??
        parseWorkspaceScopeKey(
          usePreferencesStore.getState().defaultWorkspaceEnv,
        ),
      hostWindowLabel: get().windowLabel || "main",
      createdAt: now,
      updatedAt: now,
    };
    const spaces = [...get().spaces, meta];
    set({ spaces });
    persistSpaces(spaces);
    return meta;
  },

  rename: (id, name) => {
    const spaces = get().spaces.map((s) =>
      s.id === id ? { ...s, name, updatedAt: Date.now() } : s,
    );
    set({ spaces });
    persistSpaces(spaces);
  },

  setEnv: (id, env) => {
    const spaces = get().spaces.map((s) =>
      s.id === id ? { ...s, env, updatedAt: Date.now() } : s,
    );
    set({ spaces });
    persistSpaces(spaces);
  },

  setColor: (id, color) => {
    const spaces = get().spaces.map((s) =>
      s.id === id ? { ...s, color, updatedAt: Date.now() } : s,
    );
    set({ spaces });
    persistSpaces(spaces);
  },

  reorder: (orderedIds) => {
    const byId = new Map(get().spaces.map((s) => [s.id, s]));
    const next: SpaceMeta[] = [];
    for (const id of orderedIds) {
      const s = byId.get(id);
      if (s) next.push(s);
    }
    for (const s of get().spaces) {
      if (!next.includes(s)) next.push(s);
    }
    if (next.length !== get().spaces.length) return;
    set({ spaces: next });
    persistSpaces(next);
  },

  remove: (id) => {
    const prev = get();
    const spaces = prev.spaces.filter((s) => s.id !== id);
    let activeId = prev.activeId;
    if (activeId === id) activeId = spaces[0]?.id ?? null;
    set({ spaces, activeId });
    persistSpaces(spaces);
    void deleteSpaceData(id);
    void emitDeleted(id);
    if (activeId !== prev.activeId) void saveActiveId(activeId);
    return activeId;
  },

  setActive: (id) => {
    if (get().activeId === id) return;
    set({ activeId: id });
    const label = get().windowLabel || "main";
    void saveActiveId(id);
    void saveActiveIdForWindow(label, id);
  },

  assignSpaceToWindow: async (id, hostWindowLabel) => {
    const spaces = get().spaces.map((s) =>
      s.id === id
        ? { ...s, hostWindowLabel: hostWindowLabel ?? "main", updatedAt: Date.now() }
        : s,
    );
    set({ spaces });
    await persistSpaces(spaces);
  },
}));
