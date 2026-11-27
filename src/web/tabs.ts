import { useEffect, useState } from "react";

export type RemotePane = {
  leafId: number;
  ptyId: number | null;
  active: boolean;
};

export type RemoteTab = {
  window: string;
  id: number;
  title: string;
  kind: string;
  spaceId?: string;
  workspace?: { key: string; label: string };
  mode?: "terminal" | "blocks";
  panes?: RemotePane[];
  agent?: { kind: string | null; state: string } | null;
};

export type TabSnapshot = { version: number; tabs: RemoteTab[] };

export type TabsStatus = "connecting" | "live" | "reconnecting";

/**
 * The registry's whole snapshot, pushed on connect and on every change. A
 * heartbeat keeps the stream alive; EventSource reconnects on its own.
 */
export function useTabsSnapshot(): {
  snapshot: TabSnapshot | null;
  status: TabsStatus;
} {
  const [snapshot, setSnapshot] = useState<TabSnapshot | null>(null);
  const [status, setStatus] = useState<TabsStatus>("connecting");

  useEffect(() => {
    const source = new EventSource("/api/tabs");
    source.addEventListener("tabs", (event) => {
      const message = event as MessageEvent<string>;
      try {
        setSnapshot(JSON.parse(message.data) as TabSnapshot);
        setStatus("live");
      } catch {
        // A malformed frame is skipped; the next one replaces it.
      }
    });
    source.addEventListener("open", () => setStatus("live"));
    source.addEventListener("error", () => setStatus("reconnecting"));
    return () => source.close();
  }, []);

  return { snapshot, status };
}

export function panes(tab: RemoteTab): RemotePane[] {
  return tab.panes ?? [];
}

export function activePane(tab: RemoteTab): RemotePane | null {
  const list = panes(tab);
  return list.find((pane) => pane.active) ?? list[0] ?? null;
}
