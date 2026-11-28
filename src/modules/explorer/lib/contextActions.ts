import { isRemoteRuntime } from "@/lib/remoteRuntime";
import type { WorkspaceEnv } from "@/modules/workspace";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { revealItemInDir } from "@tauri-apps/plugin-opener";

export async function copyToClipboard(text: string): Promise<void> {
  try {
    await navigator.clipboard.writeText(text);
  } catch {
    // Best-effort; ignore in environments without clipboard permission.
  }
}

export function relativePath(rootPath: string, path: string): string {
  if (path === rootPath) return ".";
  if (path.startsWith(`${rootPath}/`)) return path.slice(rootPath.length + 1);
  return path;
}

export async function revealInFinder(path: string): Promise<void> {
  if (isRemoteRuntime()) return;
  try {
    await revealItemInDir(path);
  } catch (e) {
    console.error("revealItemInDir failed:", e);
  }
}

export async function downloadRemoteFile(
  path: string,
  workspace?: WorkspaceEnv,
): Promise<void> {
  if (!isRemoteRuntime()) return;
  const token = await invoke<string>("asset_grant", { path, workspace });
  const url = `${convertFileSrc(path, "download")}?token=${encodeURIComponent(token)}`;
  const a = document.createElement("a");
  a.href = url;
  a.download = path.split(/[\\/]/).pop() || "download";
  a.style.display = "none";
  document.body.appendChild(a);
  a.click();
  document.body.removeChild(a);
}
