import type { GitHistoryTab, Tab } from "@/modules/tabs";
import { slotOf, slotStyle } from "@/modules/tabs/lib/splitLayout";
import { GitHistoryPane, type GitHistorySearchHandle } from "./GitHistoryPane";

type CommitFileDiffOpenInput = {
  repoRoot: string;
  sha: string;
  shortSha: string;
  subject: string;
  path: string;
  originalPath: string | null;
};

type Props = {
  tabs: Tab[];
  activeId: number;
  /** Tab shown on the right of a split view, if any. */
  secondaryId?: number | null;
  onOpenCommitFile: (input: CommitFileDiffOpenInput) => void;
  onSearchHandle?: (handle: GitHistorySearchHandle | null) => void;
};

export function GitHistoryStack({
  tabs,
  activeId,
  secondaryId = null,
  onOpenCommitFile,
  onSearchHandle,
}: Props) {
  const split = secondaryId !== null;
  const shown = tabs.filter(
    (t): t is GitHistoryTab =>
      t.kind === "git-history" && slotOf(t.id, activeId, secondaryId) !== null,
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
          <GitHistoryPane
            repoRoot={t.repoRoot}
            onOpenCommitFile={onOpenCommitFile}
            // Find in the header searches the active (left) history only.
            onSearchHandle={t.id === activeId ? onSearchHandle : undefined}
          />
        </div>
      ))}
    </div>
  );
}
