import { useMemo } from "react";
import { useShallow } from "zustand/react/shallow";
import { Alert02Icon, CancelCircleIcon } from "@hugeicons/core-free-icons";
import { HugeiconsIcon } from "@hugeicons/react";
import type { DiagnosticItem } from "./lib/diagnosticsStore";
import { useDiagnosticsStore } from "./lib/diagnosticsStore";

type Props = {
  onOpenAt: (path: string, line: number) => void;
};

type FileGroup = {
  path: string;
  errors: number;
  warnings: number;
  items: DiagnosticItem[];
};

function basename(path: string): string {
  const clean = path.replace(/[\\/]+$/, "");
  const idx = Math.max(clean.lastIndexOf("/"), clean.lastIndexOf("\\"));
  return idx >= 0 ? clean.slice(idx + 1) : clean;
}

export function ProblemsPanel({ onOpenAt }: Props) {
  const byPath = useDiagnosticsStore(useShallow((s) => s.byPath));

  const groups = useMemo<FileGroup[]>(() => {
    const all: FileGroup[] = [];
    for (const [path, counts] of Object.entries(byPath)) {
      const items = counts.items ?? [];
      if (items.length === 0 && counts.errors === 0 && counts.warnings === 0)
        continue;
      all.push({
        path,
        errors: counts.errors,
        warnings: counts.warnings,
        items: items.filter((i) => i.severity !== "info"),
      });
    }
    all.sort((a, b) => {
      const sev = b.errors - a.errors || b.warnings - a.warnings;
      return sev !== 0 ? sev : a.path.localeCompare(b.path);
    });
    return all;
  }, [byPath]);

  if (groups.length === 0) {
    return (
      <div className="flex h-full min-h-0 flex-col items-center justify-center gap-2 text-center text-[11px] text-muted-foreground">
        <HugeiconsIcon
          icon={CancelCircleIcon}
          size={22}
          strokeWidth={1.5}
          className="opacity-40"
        />
        <span>No problems detected</span>
      </div>
    );
  }

  return (
    <div className="h-full min-h-0 overflow-auto px-1.5 py-1 text-[11px]">
      {groups.map((group) => (
        <div key={group.path} className="mb-1">
          <div className="flex items-center gap-1.5 rounded px-1 py-0.5 font-medium text-foreground/90">
            <span className="truncate">{basename(group.path)}</span>
            <span className="shrink-0 truncate text-[10px] font-normal text-muted-foreground/80">
              {group.path}
            </span>
          </div>
          {group.items.map((item, index) => (
            <button
              key={`${item.line}:${item.column}:${index}`}
              type="button"
              onClick={() => onOpenAt(group.path, item.line)}
              className="flex w-full items-start gap-1.5 rounded px-2 py-1 text-left text-muted-foreground outline-none transition-colors hover:bg-foreground/[0.05] focus-visible:bg-foreground/[0.06] focus-visible:ring-1 focus-visible:ring-primary/40"
            >
              <HugeiconsIcon
                icon={
                  item.severity === "error" ? CancelCircleIcon : Alert02Icon
                }
                size={12}
                strokeWidth={2}
                className={
                  item.severity === "error"
                    ? "mt-0.5 shrink-0 text-destructive"
                    : "mt-0.5 shrink-0 text-amber-700 dark:text-amber-400"
                }
              />
              <span className="min-w-0 flex-1">
                <span className="line-clamp-2 leading-snug">{item.message}</span>
                <span className="text-[10px] tabular-nums text-muted-foreground/70">
                  Line {item.line}:{item.column}
                </span>
              </span>
            </button>
          ))}
        </div>
      ))}
    </div>
  );
}
