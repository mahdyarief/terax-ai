import { useEffect } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { findLeafCwd } from "@/modules/terminal/lib/panes";
import type { Tab } from "./useTabs";

const APP_NAME = "Terax";

function basename(path: string): string {
  const parts = path.split(/[\\/]/).filter(Boolean);
  return parts.length ? parts[parts.length - 1] : "/";
}

/** Label of the focused tab — for terminals, the active pane's folder. */
function tabLabel(tab: Tab | undefined): string {
  if (!tab) return "";
  if (tab.kind === "terminal") {
    const cwd = findLeafCwd(tab.paneTree, tab.activeLeafId) ?? tab.cwd;
    return cwd ? basename(cwd) : tab.title;
  }
  return tab.title;
}

/**
 * Drives the OS window title from the focused tab + project/space context.
 * Without this the window keeps the build-time default ("Tauri App" on Linux).
 *
 * Main window format: `<project> — <tab>` (e.g. `terax-ai — src`).
 * Space window format: `<space> — <tab>` so detached spaces are identifiable
 * in the OS taskbar/switcher.
 */
export function useWindowTitle(
  activeTab: Tab | undefined,
  explorerRoot: string | null,
  spaceName?: string | null,
): void {
  const project = explorerRoot ? basename(explorerRoot) : "";
  const label = tabLabel(activeTab);
  const scope = spaceName?.trim() || project;

  useEffect(() => {
    let title: string;
    if (scope && label && label !== scope) title = `${scope} — ${label}`;
    else title = scope || label || APP_NAME;

    document.title = title;
    void getCurrentWindow()
      .setTitle(title)
      .catch(() => {});
  }, [scope, label]);
}
