import { IS_WINDOWS } from "@/lib/platform";
import type { ShellDialect } from "@/lib/shellQuote";
import type { WorkspaceEnv } from "@/modules/workspace";
import { invoke } from "@tauri-apps/api/core";
import { toast } from "sonner";
import { formatDroppedPaths } from "./quoteShellPath";

/**
 * Pastes the clipboard's image into a terminal by path: the backend saves it
 * as a PNG in `terax-image` (this machine's temp folder, WSL through
 * `/mnt/<drive>`, or `~/.cache/terax-image` on the server), and the quoted
 * path is what gets pasted, as a dropped file is. Null when the clipboard
 * holds no image.
 */
export async function pasteClipboardImage(
  workspace: WorkspaceEnv,
  dialect: () => ShellDialect | null | Promise<ShellDialect | null>,
  save: (workspace: WorkspaceEnv) => Promise<string | null> = (ws) =>
    invoke<string | null>("pty_paste_image", { workspace: ws }),
): Promise<string | null> {
  let path: string | null;
  try {
    path = await save(workspace);
  } catch (error) {
    toast.error("Couldn't paste the image", { description: String(error) });
    return null;
  }
  if (!path) return null;
  return formatDroppedPaths([path], await dialectOr(workspace, dialect));
}

async function dialectOr(
  workspace: WorkspaceEnv,
  dialect: () => ShellDialect | null | Promise<ShellDialect | null>,
): Promise<ShellDialect> {
  try {
    const known = await dialect();
    if (known) return known;
  } catch {
    // A shell without a quoting dialect still gets a usable path.
  }
  return workspace.kind === "local" && IS_WINDOWS ? "powershell" : "posix";
}
