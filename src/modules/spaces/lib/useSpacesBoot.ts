import { native } from "@/modules/ai/lib/native";
import { usePreferencesStore } from "@/modules/settings/preferences";
import type { Tab } from "@/modules/tabs";
import { DEFAULT_SPACE_ID } from "@/modules/tabs/lib/useTabs";
import { isLeaf, type PaneNode } from "@/modules/terminal/lib/panes";
import { parseWorkspaceScopeKey, type WorkspaceEnv } from "@/modules/workspace";
import { useEffect, useRef } from "react";
import { activeSpaceEnv, freshTabCwd } from "./activeSpace";
import { spaceIdFromWindowLabel } from "./handoff";
import { freshTerminalTab, hydrateTabs } from "./serialize";
import { loadAll, type SpaceMeta, saveActiveId, saveSpacesList } from "./store";
import { selectHostedSpaces, useSpaces } from "./useSpaces";

export function spacesForWindow(
  spaces: SpaceMeta[],
  windowLabel: string,
): SpaceMeta[] {
  return selectHostedSpaces(spaces, windowLabel);
}

/**
 * Which spaces this window should render on boot, plus whether their tabs are
 * still in flight from another window.
 *
 * A space window is created before its source publishes the new host, so the
 * persisted registry can still list the space as hosted by "main". In that
 * case the window label is authoritative for which space it exists to show,
 * and `awaitingHandoff` tells boot to wait for SPACE_TABS_MOVED_EVENT instead
 * of restoring the same tabs from disk (or spawning a duplicate shell).
 */
export function resolveBootTargets(
  spaces: SpaceMeta[],
  windowLabel: string,
): { hosted: SpaceMeta[]; awaitingHandoff: boolean } {
  const hosted = spacesForWindow(spaces, windowLabel);
  if (hosted.length > 0) return { hosted, awaitingHandoff: false };

  const labelSpaceId = spaceIdFromWindowLabel(windowLabel);
  if (!labelSpaceId) return { hosted, awaitingHandoff: false };

  const target = spaces.find((s) => s.id === labelSpaceId);
  if (!target) return { hosted, awaitingHandoff: false };

  return { hosted: [target], awaitingHandoff: true };
}

/**
 * Move spaces hosted by a window that no longer exists back to "main". A cold
 * start only reopens "main", so without this a full restart leaves those
 * spaces invisible with no window to show them. Returns the same array when
 * nothing was orphaned so callers can skip the registry write.
 */
export function reclaimOrphanSpaces(
  spaces: SpaceMeta[],
  isAlive: (label: string) => boolean,
  now: number,
): SpaceMeta[] {
  let changed = false;
  const next = spaces.map((s) => {
    const host = s.hostWindowLabel ?? "main";
    if (host === "main" || isAlive(host)) return s;
    changed = true;
    return { ...s, hostWindowLabel: "main", updatedAt: now };
  });
  return changed ? next : spaces;
}

/**
 * Which of these webview labels currently exist. Returns null when the window
 * layer is unavailable (tests, non-Tauri) so callers never reclaim on a guess.
 */
async function liveWindowLabels(
  labels: string[],
): Promise<Set<string> | null> {
  if (labels.length === 0) return new Set();
  try {
    const { WebviewWindow } = await import("@tauri-apps/api/webviewWindow");
    const alive = new Set<string>();
    for (const label of labels) {
      if (await WebviewWindow.getByLabel(label)) alive.add(label);
    }
    return alive;
  } catch {
    return null;
  }
}

type Params = {
  ready: boolean;
  launchCwd: string | null;
  home: string | null;
  windowLabel: string;
  allocId: () => number;
  replaceTabs: (tabs: Tab[], activeId: number) => void;
  markBooted: () => void;
  setActiveSpaceForNewTabs: (id: string) => void;
  adoptWorkspaceEnv: (env: WorkspaceEnv) => Promise<string | null>;
};

function uniqueCwds(tabs: Tab[]): string[] {
  const set = new Set<string>();
  const walk = (n: PaneNode) => {
    if (isLeaf(n)) {
      if (n.cwd) set.add(n.cwd);
      return;
    }
    for (const c of n.children) walk(c);
  };
  for (const t of tabs) if (t.kind === "terminal") walk(t.paneTree);
  return [...set];
}

export function useSpacesBoot({
  ready,
  launchCwd,
  home,
  windowLabel,
  allocId,
  replaceTabs,
  markBooted,
  setActiveSpaceForNewTabs,
  adoptWorkspaceEnv,
}: Params) {
  const done = useRef(false);

  useEffect(() => {
    if (!ready || done.current) return;
    done.current = true;

    void (async () => {
      try {
        const { spaces, activeIdByWindow, states } = await loadAll();
        let registry = spaces;

        useSpaces.setState({ windowLabel });

        if (registry.length === 0) {
          // First run: there is no persisted space at all. Create the default
          // space for this window (home window is "main").
          const meta: SpaceMeta = {
            id: DEFAULT_SPACE_ID,
            name: "Default",
            root: launchCwd ?? home ?? null,
            env: parseWorkspaceScopeKey(
              usePreferencesStore.getState().defaultWorkspaceEnv,
            ),
            hostWindowLabel: windowLabel || "main",
            createdAt: Date.now(),
            updatedAt: Date.now(),
          };
          await saveSpacesList([meta]);
          await saveActiveId(DEFAULT_SPACE_ID);
          setActiveSpaceForNewTabs(DEFAULT_SPACE_ID);
          useSpaces.getState().hydrate([meta], DEFAULT_SPACE_ID);
          return;
        }

        if (windowLabel === "main") {
          // Cold start only creates "main". Reclaim spaces assigned to space
          // windows that no longer exist so hosting survives a full restart,
          // while leaving spaces in live windows (main reload) untouched.
          const hosts = [
            ...new Set(
              registry
                .map((s) => s.hostWindowLabel ?? "main")
                .filter((label) => label !== "main"),
            ),
          ];
          const alive = await liveWindowLabels(hosts);
          if (alive) {
            const reclaimed = reclaimOrphanSpaces(
              registry,
              (label) => alive.has(label),
              Date.now(),
            );
            if (reclaimed !== registry) {
              registry = reclaimed;
              await saveSpacesList(registry);
            }
          }
        }

        const { hosted, awaitingHandoff } = resolveBootTargets(
          registry,
          windowLabel,
        );

        if (hosted.length === 0) {
          // No space is hosted here yet. Keep the full registry for the
          // switcher, but render an empty tab list until a handoff arrives.
          useSpaces.getState().hydrate(registry, null, {});
          setActiveSpaceForNewTabs("");
          return;
        }

        const restored: Tab[] = [];
        if (!awaitingHandoff) {
          for (const space of hosted) {
            const st = states.get(space.id);
            if (!st) continue;
            restored.push(...hydrateTabs(st.tabs, space.id, allocId));
          }
        }

        const perWindow = activeIdByWindow[windowLabel] ?? null;
        const saveActive = registry.some((s) => s.id === perWindow)
          ? perWindow!
          : hosted[0].id;
        setActiveSpaceForNewTabs(saveActive);

        // Apply the space's env+home before the fresh-tab fallback and spawns
        // below; env is set synchronously so cwd resolution picks WSL vs local.
        const env = activeSpaceEnv(hosted, saveActive);
        const restoredHome = await adoptWorkspaceEnv(env);

        if (awaitingHandoff) {
          // The source window still owns this space's tabs and is about to
          // hand them over. Select the space now so the window shows the right
          // workspace immediately, and let SPACE_TABS_MOVED_EVENT supply the
          // tabs; hydrating from disk or spawning a fresh shell here would
          // duplicate the tab that is already in flight.
          useSpaces.getState().hydrate(registry, saveActive, {});
          return;
        }

        // Active space must never be empty, else its tab list shows nothing.
        if (!restored.some((t) => t.spaceId === saveActive)) {
          const activeSpaceRoot = hosted.find((s) => s.id === saveActive)?.root;
          const cwd = freshTabCwd(env, restoredHome, launchCwd, home, activeSpaceRoot);
          restored.push(freshTerminalTab(saveActive, cwd, allocId));
        }

        await Promise.allSettled(
          uniqueCwds(restored).map((cwd) => native.workspaceAuthorize(cwd)),
        );

        const initialActiveIndex: Record<string, number> = {};
        for (const space of hosted) {
          const st = states.get(space.id);
          if (st) initialActiveIndex[space.id] = st.activeTabIndex;
        }
        // Registry keeps every space (the switcher needs to show spaces hosted
        // by other windows); only this window's tabs were restored above.
        useSpaces.getState().hydrate(registry, saveActive, initialActiveIndex);

        const inActive = restored.filter((t) => t.spaceId === saveActive);
        const idx = states.get(saveActive)?.activeTabIndex ?? 0;
        const activeTab = inActive[idx] ?? inActive[0] ?? restored[0];
        replaceTabs(restored, activeTab.id);
      } catch (e) {
        console.error("[terax] spaces boot failed:", e);
      } finally {
        markBooted();
      }
    })();
  }, [
    ready,
    launchCwd,
    home,
    windowLabel,
    allocId,
    replaceTabs,
    markBooted,
    setActiveSpaceForNewTabs,
    adoptWorkspaceEnv,
  ]);
}
