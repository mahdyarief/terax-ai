import type { AiDiffTab, Tab } from "@/modules/tabs";
import { slotOf, slotStyle } from "@/modules/tabs/lib/splitLayout";
import { AiDiffPane } from "./AiDiffPane";

type Props = {
  tabs: Tab[];
  activeId: number;
  /** Tab shown on the right of a split view, if any. */
  secondaryId?: number | null;
  onAccept: (approvalId: string) => void;
  onReject: (approvalId: string) => void;
};

export function AiDiffStack({
  tabs,
  activeId,
  secondaryId = null,
  onAccept,
  onReject,
}: Props) {
  const split = secondaryId !== null;
  const shown = tabs.filter(
    (t): t is AiDiffTab =>
      t.kind === "ai-diff" && slotOf(t.id, activeId, secondaryId) !== null,
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
          <AiDiffPane
            path={t.path}
            originalContent={t.originalContent}
            proposedContent={t.proposedContent}
            status={t.status}
            isNewFile={t.isNewFile}
            onAccept={() => onAccept(t.approvalId)}
            onReject={() => onReject(t.approvalId)}
          />
        </div>
      ))}
    </div>
  );
}
