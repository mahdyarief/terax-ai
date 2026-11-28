import { pendingClipboardWrite, writeClipboardText } from "@/lib/clipboard";
import { isTauri } from "@tauri-apps/api/core";

export async function readTerminalClipboard(): Promise<string> {
  try {
    if (isTauri()) {
      // A copy still in progress goes first; if it failed, nothing is pasted
      // rather than the clipboard from before it.
      const pending = pendingClipboardWrite();
      if (pending) await pending;
      const { readText } = await import("@tauri-apps/plugin-clipboard-manager");
      return await readText();
    }
    if (navigator.clipboard?.readText) {
      return await navigator.clipboard.readText();
    }
    if (typeof window !== "undefined") {
      return window.prompt("Paste text into terminal:") ?? "";
    }
    return "";
  } catch {
    return "";
  }
}

export async function writeTerminalClipboard(text: string): Promise<void> {
  await writeClipboardText(text);
}
