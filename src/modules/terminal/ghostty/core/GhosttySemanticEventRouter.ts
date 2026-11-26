import type { GhosttyTerminalEvent } from "@terax/ghostty-core/protocol";
import { parseOsc7, parseOsc52Clipboard } from "../../lib/osc-handlers";
import {
  decodeTerminalProgress,
  type TerminalProgress,
} from "../../lib/terminalProgress";

export type GhosttySemanticEventCallbacks = {
  readonly onBell?: () => void;
  readonly onClipboard?: (text: string) => void;
  readonly onCommandState?: (running: boolean) => void;
  /** A prompt ended; false when its shell cannot report that a command started. */
  readonly onPromptEnd?: (reportsCommandStart: boolean) => void;
  readonly onCwd?: (cwd: string) => void;
  readonly onNotification?: (title: string, body: string) => void;
  readonly onOverflow?: (dropped: number) => void;
  /** OSC 9;4 progress; null when it is removed or the prompt is back. */
  readonly onProgress?: (progress: TerminalProgress | null) => void;
  readonly onTitle?: (title: string) => void;
};

/**
 * Routes parser-owned semantic events into Terax product callbacks.
 *
 * OSC 7 is accepted only between commands. Output produced by a command is
 * untrusted and must not be allowed to change the workspace directory.
 */
export class GhosttySemanticEventRouter {
  private inCommand = false;
  private hasProgress = false;

  /** `remote`: the terminal's shell runs on a server (see `parseOsc7`). */
  constructor(
    private readonly callbacks: GhosttySemanticEventCallbacks,
    private readonly options: { readonly remote?: boolean } = {},
  ) {}

  handle(event: GhosttyTerminalEvent): void {
    switch (event.type) {
      case "bell":
        this.callbacks.onBell?.();
        break;
      case "title":
        this.callbacks.onTitle?.(event.title);
        break;
      case "pwd": {
        if (this.inCommand) break;
        const cwd = parseOsc7(event.uri, this.options);
        if (cwd) this.callbacks.onCwd?.(cwd);
        break;
      }
      case "clipboard": {
        const text = parseOsc52Clipboard(`${event.selection};${event.data}`);
        if (text !== null) this.callbacks.onClipboard?.(text);
        break;
      }
      case "notification":
        this.callbacks.onNotification?.(event.title, event.body);
        break;
      case "prompt-start":
        this.inCommand = false;
        this.callbacks.onCommandState?.(false);
        // A program that exits (or crashes) mid progress leaves it set.
        this.clearProgress();
        break;
      case "prompt-continuation":
        break;
      case "prompt-end":
        this.inCommand = true;
        this.callbacks.onPromptEnd?.(event.blockInput !== false);
        break;
      case "end-of-input":
        this.inCommand = true;
        this.callbacks.onCommandState?.(true);
        break;
      case "end-of-command":
        this.inCommand = false;
        this.callbacks.onCommandState?.(false);
        this.clearProgress();
        break;
      case "progress": {
        const progress = decodeTerminalProgress(
          event.state,
          event.progress ?? 255,
        );
        this.hasProgress = progress !== null;
        this.callbacks.onProgress?.(progress);
        break;
      }
      case "overflow":
        this.callbacks.onOverflow?.(event.dropped);
        break;
    }
  }

  reset(): void {
    this.inCommand = false;
    this.clearProgress();
  }

  private clearProgress(): void {
    if (!this.hasProgress) return;
    this.hasProgress = false;
    this.callbacks.onProgress?.(null);
  }
}
