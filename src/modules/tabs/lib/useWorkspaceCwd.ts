import { useCallback, useEffect, useMemo, useRef } from "react";
import type { Tab } from "./useTabs";

type Result = {
  /** The space's root (or the terminal's folder without one): AI, git,
   * search and a server's authorization use it. */
  explorerRoot: string | null;
  /** The folder the file explorer shows: the terminal's folder while it is
   * inside the root, so `cd` moves the explorer; the root otherwise. */
  explorerViewRoot: string | null;
  inheritedCwdForNewTab: () => string | undefined;
};

export function useWorkspaceCwd(
  activeTab: Tab | undefined,
  tabs: Tab[],
  home: string | null,
  spaceRoot?: string | null,
): Result {
  const lastTerminalCwd = useRef<string | null>(null);

  useEffect(() => {
    if (activeTab?.kind === "terminal" && activeTab.cwd) {
      lastTerminalCwd.current = activeTab.cwd;
    }
  }, [activeTab]);

  const explorerRoot = useMemo<string | null>(() => {
    if (spaceRoot) return spaceRoot;
    if (activeTab?.kind === "terminal" && activeTab.cwd) return activeTab.cwd;
    if (lastTerminalCwd.current) return lastTerminalCwd.current;
    const anyTerm = tabs.find((t) => t.kind === "terminal" && t.cwd);
    if (anyTerm?.kind === "terminal" && anyTerm.cwd) return anyTerm.cwd;
    return home;
  }, [spaceRoot, activeTab, tabs, home]);

  const explorerViewRoot = useMemo<string | null>(() => {
    if (!spaceRoot) return explorerRoot;
    const cwd =
      activeTab?.kind === "terminal" && activeTab.cwd
        ? activeTab.cwd
        : lastTerminalCwd.current;
    return cwd && isWithinRoot(cwd, spaceRoot) ? cwd : spaceRoot;
  }, [spaceRoot, explorerRoot, activeTab]);

  const inheritedCwdForNewTab = useCallback((): string | undefined => {
    if (activeTab?.kind === "terminal" && activeTab.cwd) return activeTab.cwd;
    // Editor tabs inherit the last terminal's cwd (or space root / workspace home), not
    // the file's folder — opening a new terminal from a file shouldn't
    // hijack the user's working directory context.
    return spaceRoot ?? lastTerminalCwd.current ?? home ?? undefined;
  }, [activeTab, spaceRoot, home]);

  return { explorerRoot, explorerViewRoot, inheritedCwdForNewTab };
}

function normalize(path: string): string {
  const slashed = path.replace(/\\/g, "/");
  const trimmed = slashed.length > 1 ? slashed.replace(/\/+$/, "") : slashed;
  // Windows paths compare without case: `C:/Repo` is `c:/repo`.
  return /^[a-z]:/i.test(trimmed) ? trimmed.toLowerCase() : trimmed;
}

/** True when `path` is `root` or a folder below it. */
export function isWithinRoot(path: string, root: string): boolean {
  const p = normalize(path);
  const r = normalize(root);
  if (p === r) return true;
  const base = r.endsWith("/") ? r : `${r}/`;
  return p.startsWith(base);
}
