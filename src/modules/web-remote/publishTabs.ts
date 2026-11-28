import { useAgentActivityStore } from "@/modules/terminal/lib/agentActivity";
import { subscribePtyChanges } from "@/modules/terminal/lib/ptyChanges";
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
  // Turned on after the window loaded: publish what is open now.
  if (next) schedulePublish();
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
 * agent activity, a space's workspace label or a pane's PTY.
 */
export function usePublishTabs(tabs: readonly Tab[]): void {
  latest = { tabs };
  useEffect(() => {
    latest = { tabs };
    // Subscribed even while off: schedulePublish does nothing until remote
    // access is on, and turning it on later must not wait for a tab change.
    schedulePublish();
    const unsubscribeActivity = useAgentActivityStore.subscribe(schedulePublish);
    const unsubscribeSpaces = useSpaces.subscribe(schedulePublish);
    // A pane's PTY opens after the tab exists; without this the browser
    // would keep the first publish, which had no PTY yet.
    const unsubscribePty = subscribePtyChanges(schedulePublish);
    return () => {
      unsubscribeActivity();
      unsubscribeSpaces();
      unsubscribePty();
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

/**
 * Follows the listener: read once so a page reload keeps publishing, then on
 * every start or stop, from Settings or the status bar.
 */
export function useRemotePublishingStatus(): void {
  useEffect(() => {
    let disposed = false;
    void invoke<{ running: boolean }>("remote_access_status")
      .then((status) => {
        if (!disposed) setRemotePublishingEnabled(status.running);
      })
      .catch(() => undefined);
    const unlisten = listen<{ running: boolean }>(
      "terax:remote-access",
      (event) => setRemotePublishingEnabled(event.payload.running),
    );
    return () => {
      disposed = true;
      void unlisten.then((stop) => stop());
    };
  }, []);
}
