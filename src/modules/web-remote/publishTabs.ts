import { useAgentActivityStore } from "@/modules/terminal/lib/agentActivity";
import { ptyIdForLeaf } from "@/modules/terminal/lib/terminalSessionApi";
import type { Tab } from "@/modules/tabs";
import { useSpaces } from "@/modules/spaces";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useEffect, useRef } from "react";
import { buildRemoteTabs } from "./tabPayload";

/** Tabs are published at most this often while remote access is on. */
export const PUBLISH_INTERVAL_MS = 250;

type Listener = () => void;
const listeners = new Set<Listener>();
let enabled = false;
/** Last render's tabs, so a store change (agent state, space labels) can
 * publish without waiting for a re-render. */
let latest: { tabs: readonly Tab[] } | null = null;
let scheduled: ReturnType<typeof setTimeout> | null = null;

export function remotePublishingEnabled(): boolean {
  return enabled;
}

export function subscribeRemotePublishing(listener: Listener): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

export function setRemotePublishingEnabled(next: boolean): void {
  if (enabled === next) return;
  enabled = next;
  if (!next && scheduled !== null) {
    clearTimeout(scheduled);
    scheduled = null;
  }
  for (const listener of listeners) listener();
}

function schedulePublish(): void {
  if (!enabled || scheduled !== null || !latest) return;
  scheduled = setTimeout(() => {
    scheduled = null;
    void publishNow();
  }, PUBLISH_INTERVAL_MS);
}

async function publishNow(): Promise<void> {
  if (!enabled || !latest) return;
  const activity = useAgentActivityStore.getState();
  const tabs = buildRemoteTabs({
    tabs: latest.tabs,
    spaces: useSpaces.getState().spaces,
    ptyIdForLeaf,
    phases: activity.phases,
    agents: activity.agents,
  });
  try {
    await invoke("remote_publish_tabs", { tabs });
  } catch {
    // Remote access off or the window is closing: publishing is best effort.
  }
}

/**
 * Mirrors this window's tabs to the native registry while remote access is
 * on. Re-runs at most every 250 ms, and only when something changed: tabs,
 * agent activity or a space's workspace label.
 */
export function usePublishTabs(tabs: readonly Tab[]): void {
  latest = { tabs };
  useEffect(() => {
    latest = { tabs };
    if (!enabled) return;
    schedulePublish();
    const unsubscribeActivity = useAgentActivityStore.subscribe(schedulePublish);
    const unsubscribeSpaces = useSpaces.subscribe(schedulePublish);
    return () => {
      unsubscribeActivity();
      unsubscribeSpaces();
    };
  }, [tabs]);
}

/** A tab action the browser asked this window to run. */
export type RemoteTabAction =
  | { action: "new"; spaceId: string; mode: string }
  | { action: "close"; tabId: number };

type TabActionHandlers = {
  openTab: (spaceId: string, mode: string) => void;
  closeTab: (tabId: number) => void;
};

/**
 * Runs the tab actions a paired browser asked for. The browser never creates
 * or closes a tab itself; it asks the window that hosts the space, so the
 * result is the same as the desktop's own New Tab and close buttons.
 */
export function useRemoteTabActions(handlers: TabActionHandlers): void {
  const latest = useRef(handlers);
  latest.current = handlers;
  useEffect(() => {
    let unlisten: (() => void) | null = null;
    let disposed = false;
    void listen<RemoteTabAction>("remote:tab-action", (event) => {
      const action = event.payload;
      if (action.action === "new") {
        latest.current.openTab(action.spaceId, action.mode);
      } else if (action.action === "close") {
        latest.current.closeTab(action.tabId);
      }
    }).then((stop) => {
      if (disposed) stop();
      else unlisten = stop;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);
}

/** Reads the backend once so a page reload keeps publishing. */
export function useRemotePublishingStatus(): void {
  const asked = useRef(false);
  useEffect(() => {
    if (asked.current) return;
    asked.current = true;
    void invoke<{ running: boolean }>("remote_access_status")
      .then((status) => setRemotePublishingEnabled(status.running))
      .catch(() => setRemotePublishingEnabled(false));
  }, []);
}
