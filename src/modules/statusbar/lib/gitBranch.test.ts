import { describe, expect, it } from "vitest";
import type { GitBranchEntry } from "@/modules/git/types";
import {
  gitBranchTitle,
  gitBranchView,
  newBranchName,
  switchableBranches,
} from "./gitBranch";

const branch = (
  name: string,
  over: Partial<GitBranchEntry> = {},
): GitBranchEntry => ({
  name,
  kind: "local",
  worktreePath: null,
  isHead: false,
  isDetached: false,
  ...over,
});

describe("gitBranchView", () => {
  it("shows the branch, or the short commit when HEAD is detached", () => {
    const base = { ahead: 0, behind: 0, changed: 0 };
    expect(
      gitBranchView({ ...base, branch: "main", isDetached: false }).label,
    ).toBe("main");
    expect(
      gitBranchView({ ...base, branch: "4c9b7d6a1f", isDetached: true }).label,
    ).toBe("4c9b7d6");
  });

  it("never shows negative counts", () => {
    const view = gitBranchView({
      branch: "main",
      isDetached: false,
      ahead: -1,
      behind: 2,
      changed: -3,
    });
    expect([view.ahead, view.behind, view.changed]).toEqual([0, 2, 0]);
  });
});

describe("gitBranchTitle", () => {
  const view = (over: Partial<ReturnType<typeof gitBranchView>>) => ({
    label: "dydev",
    detached: false,
    ahead: 0,
    behind: 0,
    changed: 0,
    ...over,
  });

  it("says how far the branch is from its upstream", () => {
    expect(gitBranchTitle(view({}), "origin/dydev")).toBe(
      "Branch dydev · up to date with origin/dydev",
    );
    expect(
      gitBranchTitle(view({ ahead: 2, behind: 1, changed: 1 }), "origin/dydev"),
    ).toBe(
      "Branch dydev · 2 to push, 1 to pull (origin/dydev) · 1 changed file",
    );
  });

  it("names a branch without an upstream as not published", () => {
    expect(gitBranchTitle(view({ changed: 3 }), null)).toBe(
      "Branch dydev · not published · 3 changed files",
    );
    expect(
      gitBranchTitle(view({ detached: true, label: "4c9b7d6" }), null),
    ).toBe("Detached at 4c9b7d6");
  });
});

describe("switchableBranches", () => {
  it("lists this worktree's branches, the current one first", () => {
    const list = switchableBranches([
      branch("feature"),
      branch("other-tree", { kind: "worktree", worktreePath: "/w" }),
      branch("main", { isHead: true }),
    ]);
    expect(list.map((b) => b.name)).toEqual(["main", "feature"]);
  });
});

describe("newBranchName", () => {
  const existing = [branch("main"), branch("feat/x")];

  it("offers a name that does not exist yet, with spaces as dashes", () => {
    expect(newBranchName("  fix tab  progress ", existing)).toBe(
      "fix-tab-progress",
    );
    expect(newBranchName("feat/y", existing)).toBe("feat/y");
  });

  it("offers nothing for an empty filter, an existing branch or an option", () => {
    expect(newBranchName("   ", existing)).toBeNull();
    expect(newBranchName("main", existing)).toBeNull();
    expect(newBranchName("--force", existing)).toBeNull();
  });
});
