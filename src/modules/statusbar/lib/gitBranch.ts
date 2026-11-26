import type { GitBranchEntry } from "@/modules/git/types";

/** What the status bar's git item shows for a repository. */
export type GitBranchView = {
  /** The branch, or the short commit for a detached HEAD. */
  label: string;
  detached: boolean;
  ahead: number;
  behind: number;
  /** Files with staged, unstaged or untracked changes. */
  changed: number;
};

export function gitBranchView(input: {
  branch: string;
  isDetached: boolean;
  ahead: number;
  behind: number;
  changed: number;
}): GitBranchView {
  return {
    label: input.isDetached ? input.branch.slice(0, 7) : input.branch,
    detached: input.isDetached,
    ahead: Math.max(0, input.ahead),
    behind: Math.max(0, input.behind),
    changed: Math.max(0, input.changed),
  };
}

/** The item's tooltip: the branch, how far it is from its upstream, and changes. */
export function gitBranchTitle(view: GitBranchView, upstream: string | null) {
  const parts = [
    view.detached ? `Detached at ${view.label}` : `Branch ${view.label}`,
  ];
  if (upstream) {
    if (view.ahead === 0 && view.behind === 0)
      parts.push(`up to date with ${upstream}`);
    else {
      const sync: string[] = [];
      if (view.ahead > 0) sync.push(`${view.ahead} to push`);
      if (view.behind > 0) sync.push(`${view.behind} to pull`);
      parts.push(`${sync.join(", ")} (${upstream})`);
    }
  } else if (!view.detached) {
    parts.push("not published");
  }
  if (view.changed > 0)
    parts.push(`${view.changed} changed file${view.changed === 1 ? "" : "s"}`);
  return parts.join(" · ");
}

/** Branches the switcher offers: this worktree's own, the current one first. */
export function switchableBranches(
  branches: readonly GitBranchEntry[],
): GitBranchEntry[] {
  return branches
    .filter((b) => b.kind === "local")
    .sort((a, b) => Number(b.isHead) - Number(a.isHead));
}

/**
 * The branch name typed in the switcher's filter, when it names a branch that
 * does not exist yet: spaces become dashes, as most git UIs do. Git checks the
 * name itself when the branch is created.
 */
export function newBranchName(
  query: string,
  branches: readonly GitBranchEntry[],
): string | null {
  const name = query.trim().replace(/\s+/g, "-");
  if (!name || name.startsWith("-")) return null;
  if (branches.some((b) => b.name === name)) return null;
  return name;
}
