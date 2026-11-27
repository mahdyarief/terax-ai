import { TeraxGhostty } from "@terax/ghostty-core/adapted";
import type { GhosttyTerminalEvent } from "@terax/ghostty-core/protocol";
import { AdaptedGhosttyTerminalModel } from "@/modules/terminal/ghostty/AdaptedGhosttyTerminalModel";
import { resolveCursorText } from "@/modules/terminal/ghostty/gpu/cursorText";
import type {
  TerminalFontSpec,
  TerminalGpuTheme,
} from "@/modules/terminal/ghostty/gpu/terminalVisuals";
import type { Rgb as PackedRgb } from "@/modules/terminal/ghostty/core/packedCells";
import { readTerminalGpuTheme } from "@/modules/terminal/ghostty/gpu/terminalVisuals";
import type { RemoteTheme } from "./feed";

/** The core is compiled once and shared; every terminal gets its own memory. */
let core: Promise<TeraxGhostty> | null = null;

export function loadGhosttyCore(): Promise<TeraxGhostty> {
  core ??= TeraxGhostty.load();
  return core;
}

/** The stack the desktop's default theme resolves to, for the web page. */
export const WEB_TERMINAL_FONT: TerminalFontSpec = {
  family:
    '"JetBrains Mono NL", "JetBrains Mono", ui-monospace, SFMono-Regular, Menlo, Consolas, monospace',
  size: 13,
  lineHeight: 1.3,
  letterSpacing: 0,
  weight: "400",
};

export function createRemoteModel(
  ghostty: TeraxGhostty,
  cols: number,
  rows: number,
  onEvent?: (event: GhosttyTerminalEvent) => void,
): AdaptedGhosttyTerminalModel {
  return new AdaptedGhosttyTerminalModel(ghostty, {
    backend: "ghostty-webgl",
    cols,
    rows,
    onEvent,
  });
}

/** The desktop's colours when the session has none to send. */
export function fallbackTheme(): TerminalGpuTheme {
  try {
    return readTerminalGpuTheme();
  } catch {
    return {
      background: [20, 20, 20],
      foreground: [220, 220, 220],
      cursor: [220, 220, 220],
      cursorText: [20, 20, 20],
      selection: { color: [80, 120, 200], alpha: 0.3 },
      palette: [
        [0, 0, 0],
        [205, 49, 49],
        [13, 188, 121],
        [229, 229, 16],
        [36, 114, 200],
        [188, 63, 188],
        [17, 168, 205],
        [229, 229, 229],
        [102, 102, 102],
        [241, 76, 76],
        [35, 209, 139],
        [245, 245, 67],
        [59, 142, 234],
        [214, 112, 214],
        [41, 184, 219],
        [229, 229, 229],
      ],
    };
  }
}

function rgb(color: readonly [number, number, number]): PackedRgb {
  return [color[0], color[1], color[2]];
}

/**
 * The PTY's own theme, which is what the terminal in front of the desktop
 * shows (a custom theme may differ from this page's CSS tokens).
 */
export function gpuThemeFromRemote(
  theme: RemoteTheme,
  fallback: TerminalGpuTheme,
): TerminalGpuTheme {
  if (theme.palette.length < 16) return fallback;
  const background = rgb(theme.background);
  const cursor = rgb(theme.cursor);
  return {
    background,
    foreground: rgb(theme.foreground),
    cursor,
    cursorText: resolveCursorText(cursor, null, background),
    selection: { color: rgb(theme.palette[4]), alpha: 0.3 },
    palette: theme.palette.slice(0, 16).map(rgb),
  };
}
