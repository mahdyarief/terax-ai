import { invoke } from "@tauri-apps/api/core";
import { create } from "zustand";
import {
  currentWorkspaceEnv,
  sameWorkspaceEnv,
  type WorkspaceEnv,
} from "@/modules/workspace";

/** A folder Terax serves at `http://localhost:<port>/` (modules/live_server). */
export type LiveServerInfo = {
  id: number;
  /** The served folder, canonical, in its workspace's spelling. */
  root: string;
  port: number;
  url: string;
  /** Addresses for other devices, when shared on the local network. */
  lanUrls: string[];
  /** Changes made outside Terax reload the page too (a folder on this machine). */
  watching: boolean;
};

export type LiveServer = LiveServerInfo & { workspace: WorkspaceEnv };

type LiveServerStore = {
  servers: readonly LiveServer[];
  /** Serves `path` (a folder, or the folder of an HTML file). */
  start: (
    path: string,
    opts?: { lan?: boolean; workspace?: WorkspaceEnv },
  ) => Promise<LiveServer>;
  stop: (id: number) => Promise<void>;
};

export const useLiveServerStore = create<LiveServerStore>((set) => ({
  servers: [],
  start: async (path, opts = {}) => {
    const workspace = opts.workspace ?? currentWorkspaceEnv();
    const info = await invoke<LiveServerInfo>("live_server_start", {
      path,
      workspace,
      lan: opts.lan ?? false,
    });
    const server: LiveServer = { ...info, workspace };
    set((s) => ({
      // Sharing a folder again replaces its server (it has a new id).
      servers: [
        ...s.servers.filter(
          (o) =>
            o.id !== server.id &&
            !(
              o.root === server.root && sameWorkspaceEnv(o.workspace, workspace)
            ),
        ),
        server,
      ],
    }));
    return server;
  },
  stop: async (id) => {
    await invoke("live_server_stop", { id });
    set((s) => ({ servers: s.servers.filter((o) => o.id !== id) }));
  },
}));

/** Files a live server can open directly; other files serve their folder. */
export function isLiveServerPage(path: string): boolean {
  return /\.html?$/i.test(path);
}

/**
 * The address of `path` on `server`: the file itself when it is inside the
 * served folder, otherwise the folder's own address.
 */
export function liveServerUrl(server: LiveServerInfo, path: string): string {
  const norm = (p: string) => p.replace(/\\/g, "/").replace(/\/+$/, "");
  const root = norm(server.root);
  const target = norm(path);
  const windows = /^[a-z]:\//i.test(root);
  const inside = windows
    ? target.toLowerCase().startsWith(`${root.toLowerCase()}/`)
    : target.startsWith(`${root}/`);
  if (!inside || !isLiveServerPage(target)) return server.url;
  const rel = target.slice(root.length + 1);
  return server.url + rel.split("/").map(encodeURIComponent).join("/");
}

/** The served folder's name, for labels. */
export function liveServerName(server: LiveServerInfo): string {
  return (
    server.root
      .replace(/[\\/]+$/, "")
      .split(/[\\/]/)
      .pop() || server.root
  );
}
