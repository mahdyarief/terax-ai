import type { GitCommitFileDiffTab, GitDiffTab, Tab } from "@/modules/tabs";
import { slotOf, slotStyle } from "@/modules/tabs/lib/splitLayout";
import { GitDiffPane } from "./GitDiffPane";

type Props = {
  tabs: Tab[];
  activeId: number;
  /** Tab shown on the right of a split view, if any. */
  secondaryId?: number | null;
};

function DiffPane({ tab }: { tab: GitDiffTab | GitCommitFileDiffTab }) {
  if (tab.kind === "git-diff") {
    return (
      <GitDiffPane
        active
        source={{
          kind: "working",
          repoRoot: tab.repoRoot,
          path: tab.path,
          mode: tab.mode,
          originalPath: tab.originalPath,
        }}
      />
    );
  }
  return (
    <GitDiffPane
      active
      source={{
        kind: "commit",
        repoRoot: tab.repoRoot,
        sha: tab.sha,
        path: tab.path,
        originalPath: tab.originalPath,
      }}
      chipLabel={tab.shortSha}
    />
  );
}

export function GitDiffStack({ tabs, activeId, secondaryId = null }: Props) {
  const split = secondaryId !== null;
  const shown = tabs.filter(
    (t): t is GitDiffTab | GitCommitFileDiffTab =>
      (t.kind === "git-diff" || t.kind === "git-commit-file") &&
      slotOf(t.id, activeId, secondaryId) !== null,
  );
  if (shown.length === 0) return null;
  return (
    <div className="relative h-full w-full">
      {shown.map((t) => (
        <div
          key={t.id}
          className="pointer-events-auto"
          style={slotStyle(slotOf(t.id, activeId, secondaryId), split)}
        >
          <DiffPane tab={t} />
        </div>
      ))}
    </div>
  );
}
