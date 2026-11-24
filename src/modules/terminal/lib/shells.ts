import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";

/** A shell installed on this machine, as `pty_list_shells` reports it. */
export type ShellInfo = {
  name: string;
  path: string;
  /** Terax adds command blocks and directory tracking to it. */
  integrated: boolean;
};

let cached: Promise<ShellInfo[]> | null = null;

/** The installed shells, asked once per window (a failure asks again). */
export function listShells(): Promise<ShellInfo[]> {
  if (!cached) {
    cached = invoke<ShellInfo[]>("pty_list_shells").catch((e: unknown) => {
      cached = null;
      throw e;
    });
  }
  return cached;
}

/** For tests: forget what was listed. */
export function resetShellsForTest() {
  cached = null;
}

export function useShells(): ShellInfo[] {
  const [shells, setShells] = useState<ShellInfo[]>([]);
  useEffect(() => {
    let live = true;
    listShells()
      .then((list) => live && setShells(list))
      .catch(() => {});
    return () => {
      live = false;
    };
  }, []);
  return shells;
}

/**
 * The shell a new tab gets without a choice: the setting's, or, on Auto,
 * the first listed (the list is in the order Auto tries them on Windows).
 */
export function defaultShellPath(
  shells: ShellInfo[],
  setting: string,
): string | null {
  if (setting && shells.some((s) => s.path === setting)) return setting;
  return shells[0]?.path ?? null;
}
