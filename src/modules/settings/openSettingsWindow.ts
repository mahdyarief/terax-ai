import { isRemoteRuntime } from "@/lib/remoteRuntime";
import { invoke } from "@tauri-apps/api/core";

export type SettingsTab =
  | "general"
  | "editor"
  | "themes"
  | "shortcuts"
  | "models"
  | "agents"
  | "cli-agents"
  | "remote"
  | "about";

export async function openSettingsWindow(tab?: SettingsTab): Promise<void> {
  if (isRemoteRuntime()) {
    window.location.hash = tab ? `#settings/${tab}` : "#settings";
    return;
  }
  await invoke("open_settings_window", { tab: tab ?? null });
}
