import { isRemoteRuntime } from "@/lib/remoteRuntime";
import { cn } from "@/lib/utils";
import { useDiagnosticsStore } from "@/modules/editor/lib/diagnosticsStore";
import {
  Alert02Icon,
  ComputerIcon,
  FolderGitTwoIcon,
  FolderTreeIcon,
} from "@hugeicons/core-free-icons";
import { HugeiconsIcon } from "@hugeicons/react";
import { useEffect, useRef, useState } from "react";
import type { SidebarViewId } from "./types";

const SIDEBAR_RAIL_HEIGHT = 36;

type RailItem = {
  id: SidebarViewId;
  label: string;
  icon: Parameters<typeof HugeiconsIcon>[0]["icon"];
  badge?: number;
};

type Props = {
  activeView: SidebarViewId;
  onSelectView: (view: SidebarViewId) => void;
  changedCount: number;
};

export function SidebarRail({ activeView, onSelectView, changedCount }: Props) {
  const railRef = useRef<HTMLDivElement>(null);
  const [compact, setCompact] = useState(false);

  useEffect(() => {
    const el = railRef.current;
    if (!el) return;

    const update = (width: number) => setCompact(width < 240);
    update(el.getBoundingClientRect().width);

    const observer = new ResizeObserver((entries) => {
      const entry = entries[0];
      if (entry) update(entry.contentRect.width);
    });
    observer.observe(el);
    return () => observer.disconnect();
  }, []);

  const problemCount = useDiagnosticsStore((s) =>
    Object.values(s.byPath).reduce(
      (total, counts) => total + counts.errors + counts.warnings,
      0,
    ),
  );
  const items: RailItem[] = [
    { id: "explorer", label: "Files", icon: FolderTreeIcon },
    {
      id: "source-control",
      label: "Git",
      icon: FolderGitTwoIcon,
      badge: changedCount,
    },
    {
      id: "problems",
      label: "Problems",
      icon: Alert02Icon,
      badge: problemCount,
    },
    ...(isRemoteRuntime()
      ? [{ id: "desktop" as const, label: "Desktop", icon: ComputerIcon }]
      : []),
  ];

  return (
    <div
      ref={railRef}
      style={{ height: SIDEBAR_RAIL_HEIGHT }}
      className="flex shrink-0 items-stretch gap-0.5 border-t border-border/60 bg-foreground/[0.025] px-1 py-1"
    >
      {items.map((item) => {
        const isActive = item.id === activeView;
        const showBadge = !!item.badge && item.badge > 0;
        const badgeText = item.badge! > 99 ? "99+" : String(item.badge);
        return (
          <button
            key={item.id}
            type="button"
            aria-label={item.label}
            aria-pressed={isActive}
            title={item.label}
            onClick={() => onSelectView(item.id)}
            className={cn(
              "group relative flex flex-1 cursor-pointer items-center justify-center rounded-md text-[11px] font-medium outline-none transition-colors duration-[var(--dur-base)]",
              "focus-visible:ring-2 focus-visible:ring-primary/40",
              compact ? "gap-0 px-0" : "gap-1.5 px-1",
              isActive
                ? "bg-foreground/[0.07] text-foreground dark:bg-foreground/[0.09]"
                : "text-muted-foreground hover:bg-foreground/[0.045] hover:text-foreground",
            )}
          >
            <span className="relative inline-flex shrink-0">
              <HugeiconsIcon
                icon={item.icon}
                size={14}
                strokeWidth={isActive ? 2 : 1.75}
                className="shrink-0 transition-[stroke-width] duration-[var(--dur-base)]"
              />
              {showBadge && compact ? (
                <span className="absolute -right-1.5 -top-1 inline-flex h-3.5 min-w-3.5 items-center justify-center rounded-full border border-border/60 bg-card px-0.5 text-[8px] font-semibold leading-none tabular-nums text-muted-foreground/95">
                  {badgeText}
                </span>
              ) : null}
            </span>
            {!compact ? <span className="truncate">{item.label}</span> : null}
            {showBadge && !compact ? (
              <span className="inline-flex h-4 min-w-4 items-center justify-center rounded-full border border-border/60 bg-card px-1 text-[9px] font-semibold leading-none tabular-nums text-muted-foreground/95">
                {badgeText}
              </span>
            ) : null}
          </button>
        );
      })}
    </div>
  );
}
