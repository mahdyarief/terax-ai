import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import type { SourceControlSummary } from "@/modules/source-control/useSourceControl";

vi.mock("@/modules/git", () => ({ git: {} }));

const { GitBranchPill } = await import("./GitBranchPill");

const summary = (over: Partial<SourceControlSummary>): SourceControlSummary =>
  ({
    contextPath: "/repo",
    repo: {
      repoRoot: "/repo",
      branch: "dydev",
      upstream: "origin/dydev",
      isDetached: false,
    },
    status: null,
    changedCount: 0,
    upstream: "origin/dydev",
    ahead: 0,
    behind: 0,
    hasRepo: true,
    isLoading: false,
    localError: null,
    busyAction: null,
    lastRemoteError: null,
    applyStatus: () => {},
    refresh: async () => {},
    runRemoteAction: async () => ({ ok: true }),
    ...over,
  }) as SourceControlSummary;

const render = (s: SourceControlSummary) =>
  renderToStaticMarkup(
    createElement(GitBranchPill, { summary: s, onOpenSourceControl: () => {} }),
  );

describe("GitBranchPill", () => {
  it("shows the branch with commits to push and pull and a changes mark", () => {
    const html = render(summary({ ahead: 2, behind: 1, changedCount: 4 }));
    expect(html).toContain("dydev*");
    expect(html).toContain("↑2");
    expect(html).toContain("↓1");
    expect(html).toContain(
      'title="Branch dydev · 2 to push, 1 to pull (origin/dydev) · 4 changed files"',
    );
  });

  it("shows a clean, up-to-date branch without counts", () => {
    const html = render(summary({}));
    expect(html).toContain(">dydev<");
    expect(html).not.toContain("↑");
    expect(html).not.toContain("↓");
  });

  it("is hidden outside a repository", () => {
    expect(render(summary({ hasRepo: false, repo: null }))).toBe("");
  });
});
