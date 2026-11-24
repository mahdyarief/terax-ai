import { isTauri } from "@tauri-apps/api/core";

/** The native copy still in progress: copies run in call order, and a paste waits for it. */
let pendingNativeWrite: Promise<void> | null = null;

/**
 * The native copy in progress, if any. A paste awaits it so it never reads
 * the clipboard from before the copy; it rejects when that copy failed.
 */
export function pendingClipboardWrite(): Promise<void> | null {
  return pendingNativeWrite;
}

/** Puts `text` on the system clipboard (the app's plugin in Tauri). */
export async function writeClipboardText(text: string): Promise<void> {
  if (isTauri()) {
    const write = async () => {
      const { writeText } = await import("@tauri-apps/plugin-clipboard-manager");
      await writeText(text);
    };
    // Two quick copies must not land in reverse order.
    const pending = pendingNativeWrite
      ? pendingNativeWrite.catch(() => {}).then(write)
      : write();
    pendingNativeWrite = pending;
    try {
      await pending;
    } finally {
      if (pendingNativeWrite === pending) pendingNativeWrite = null;
    }
    return;
  }
  if (!navigator.clipboard) throw new Error("Clipboard is unavailable");
  await navigator.clipboard.writeText(text);
}
