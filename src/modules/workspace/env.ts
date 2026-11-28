import { invoke } from "@tauri-apps/api/core";
import { create } from "zustand";
import { isRemoteRuntime } from "@/lib/remoteRuntime";
import { setLastWslDistro } from "@/modules/settings/store";
import { useBrowserFolderPicker } from "./folderPickerStore";

export type WorkspaceEnv =
  | { kind: "local" }
  | { kind: "wsl"; distro: string }
  // A server over SSH. Its paths are the server's; nothing on this machine
  // (home directory, reveal in Explorer, local shell) applies to it.
  | { kind: "ssh"; host: string };

export type WslDistro = {
  name: string;
  default: boolean;
  running: boolean;
};

type State = {
  env: WorkspaceEnv;
  distros: WslDistro[];
  loading: boolean;
  error: string | null;
  setEnv: (env: WorkspaceEnv) => void;
  refreshDistros: () => Promise<WslDistro[]>;
};

export const LOCAL_WORKSPACE: WorkspaceEnv = { kind: "local" };

export const useWorkspaceEnvStore = create<State>((set) => ({
  env: LOCAL_WORKSPACE,
  distros: [],
  loading: false,
  error: null,
  setEnv: (env) => {
    set({ env });
    if (env.kind === "wsl") void setLastWslDistro(env.distro);
  },
  refreshDistros: async () => {
    set({ loading: true, error: null });
    try {
      const distros = await invoke<WslDistro[]>("wsl_list_distros");
      set({ distros, loading: false });
      return distros;
    } catch (e) {
      set({ distros: [], loading: false, error: String(e) });
      return [];
    }
  },
}));

export function currentWorkspaceEnv(): WorkspaceEnv {
  return useWorkspaceEnvStore.getState().env;
}

export function workspaceScopeKey(env: WorkspaceEnv): string {
  switch (env.kind) {
    case "wsl":
      return `wsl:${env.distro}`;
    case "ssh":
      return `ssh:${env.host}`;
    case "local":
      return "local";
  }
}

export function parseWorkspaceScopeKey(key: string): WorkspaceEnv {
  if (key.startsWith("wsl:")) return { kind: "wsl", distro: key.slice("wsl:".length) };
  if (key.startsWith("ssh:")) return { kind: "ssh", host: key.slice("ssh:".length) };
  return LOCAL_WORKSPACE;
}

export function sameWorkspaceEnv(a: WorkspaceEnv, b: WorkspaceEnv): boolean {
  return workspaceScopeKey(a) === workspaceScopeKey(b);
}

/** A short name for the status bar and titles. */
export function workspaceEnvLabel(env: WorkspaceEnv, localName: string): string {
  switch (env.kind) {
    case "wsl":
      return `WSL: ${env.distro}`;
    case "ssh":
      return `SSH: ${env.host}`;
    case "local":
      return localName;
  }
}

export function currentWorkspaceScopeKey(): string {
  return workspaceScopeKey(currentWorkspaceEnv());
}

export async function getWslHome(distro: string): Promise<string> {
  return invoke<string>("wsl_home", { distro });
}

export async function pickWorkspaceFolder(
  defaultPath?: string | null,
): Promise<string | null> {
  // A browser session cannot show the host's native dialog (it is Native in
  // the bridge policy): pick in the app instead.
  if (isRemoteRuntime()) {
    return useBrowserFolderPicker.getState().open(defaultPath ?? null);
  }
  return invoke<string | null>("workspace_pick_folder", {
    defaultPath: defaultPath ?? null,
  });
}
