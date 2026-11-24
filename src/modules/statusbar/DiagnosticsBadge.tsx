import { useDiagnosticsStore } from "@/modules/editor";
import { Alert02Icon, CancelCircleIcon } from "@hugeicons/core-free-icons";
import { HugeiconsIcon } from "@hugeicons/react";

type Props = {
  filePath: string | null;
  onOpen?: () => void;
};

export function DiagnosticsBadge({ filePath, onOpen }: Props) {
  const counts = useDiagnosticsStore((s) =>
    filePath ? s.byPath[filePath] : undefined,
  );
  if (!counts || (counts.errors === 0 && counts.warnings === 0)) return null;

  return (
    <button
      type="button"
      onClick={onOpen}
      aria-label="Open problems panel"
      className="terax-pill-in flex shrink-0 cursor-pointer items-center gap-2 rounded text-[11px] font-medium tabular-nums outline-none transition-colors hover:bg-foreground/[0.06] focus-visible:ring-1 focus-visible:ring-primary/40"
    >
      {counts.errors > 0 ? (
        <span className="flex items-center gap-0.5 text-destructive">
          <HugeiconsIcon icon={CancelCircleIcon} size={12} strokeWidth={2} />
          {counts.errors}
        </span>
      ) : null}
      {counts.warnings > 0 ? (
        <span className="flex items-center gap-0.5 text-amber-700 dark:text-amber-400">
          <HugeiconsIcon icon={Alert02Icon} size={12} strokeWidth={2} />
          {counts.warnings}
        </span>
      ) : null}
    </button>
  );
}
