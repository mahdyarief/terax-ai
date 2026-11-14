import { ArrowDown01Icon, Cancel01Icon } from "@hugeicons/core-free-icons";
import { HugeiconsIcon } from "@hugeicons/react";
import { cn } from "@/lib/utils";
import {
  memo,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type FC,
  type KeyboardEvent,
} from "react";
import {
  buildOutlineModel,
  formatDepthLabel,
  type OutlineRow,
  type TocHeading,
} from "@/components/markdown/useMarkdownHeadings";

const MAX_LEVELS = [1, 2, 3, 4, 5, 6];

/** 6px base plus 12px per level below the shallowest heading. */
function rowPaddingLeft(depth: number): number {
  return 6 + depth * 12;
}

/** Top-level rows read as body text; deeper rows are muted. */
function depthTone(depth: number): string {
  return depth === 0 ? "font-medium text-foreground" : "text-muted-foreground";
}

interface OutlineListProps {
  rows: OutlineRow[];
  activeRowId?: string;
  onHeadingClick: (heading: TocHeading) => void;
  onToggleCollapsed: (key: string) => void;
}

const OutlineList: FC<OutlineListProps> = memo(function OutlineList({
  rows,
  activeRowId,
  onHeadingClick,
  onToggleCollapsed,
}: OutlineListProps) {
  const rowRefs = useRef(new Map<string, HTMLButtonElement>());
  const activeIndex = rows.findIndex((row) => row.heading.id === activeRowId);
  const [focusIndex, setFocusIndex] = useState(Math.max(0, activeIndex));
  const hasFocusRef = useRef(false);

  // Keep the roving tab stop on the active row while focus is outside the list.
  useEffect(() => {
    if (!hasFocusRef.current && activeIndex >= 0) setFocusIndex(activeIndex);
  }, [activeIndex]);

  const safeIndex = Math.min(focusIndex, Math.max(0, rows.length - 1));

  const focusRow = useCallback(
    (index: number) => {
      const row = rows[index];
      if (!row) return;
      setFocusIndex(index);
      rowRefs.current.get(row.key)?.focus();
    },
    [rows],
  );

  const handleKeyDown = useCallback(
    (event: KeyboardEvent<HTMLDivElement>) => {
      const row = rows[safeIndex];
      if (!row) return;
      switch (event.key) {
        case "ArrowDown":
          event.preventDefault();
          focusRow(Math.min(rows.length - 1, safeIndex + 1));
          break;
        case "ArrowUp":
          event.preventDefault();
          focusRow(Math.max(0, safeIndex - 1));
          break;
        case "ArrowRight":
          if (row.hasChildren && row.isCollapsed) {
            event.preventDefault();
            onToggleCollapsed(row.key);
          }
          break;
        case "ArrowLeft":
          if (row.hasChildren && !row.isCollapsed) {
            event.preventDefault();
            onToggleCollapsed(row.key);
          }
          break;
        case "Enter":
        case " ":
          event.preventDefault();
          onHeadingClick(row.heading);
          break;
        default:
          break;
      }
    },
    [rows, safeIndex, focusRow, onToggleCollapsed, onHeadingClick],
  );

  return (
    <div
      className="flex min-h-0 flex-1 flex-col gap-px overflow-y-auto py-1"
      role="tree"
      aria-label="Headings"
      onKeyDown={handleKeyDown}
      onFocus={() => {
        hasFocusRef.current = true;
      }}
      onBlur={() => {
        hasFocusRef.current = false;
      }}
    >
      {rows.map((row, index) => (
        <Row
          key={row.key}
          row={row}
          tabbable={index === safeIndex}
          active={activeIndex === index}
          ref={(el) => {
            if (el) rowRefs.current.set(row.key, el);
            else rowRefs.current.delete(row.key);
          }}
          onClick={onHeadingClick}
          onToggle={onToggleCollapsed}
        />
      ))}
    </div>
  );
});

interface RowProps {
  row: OutlineRow;
  tabbable: boolean;
  active: boolean;
  /** React 19 passes refs as plain props; no forwardRef needed. */
  ref: (el: HTMLButtonElement | null) => void;
  onClick: (heading: TocHeading) => void;
  onToggle: (key: string) => void;
}

function Row({ row, tabbable, active, ref, onClick, onToggle }: RowProps) {
  return (
    <div
      className={cn("group/row flex items-stretch rounded-md pr-1")}
      style={{ paddingLeft: rowPaddingLeft(row.depth) }}
      role="none"
    >
      {row.hasChildren ? (
        <button
          type="button"
          tabIndex={-1}
          aria-hidden="true"
          onClick={() => onToggle(row.key)}
          className="me-1 flex w-4 shrink-0 items-center justify-center rounded text-muted-foreground/70 hover:bg-accent hover:text-accent-foreground"
        >
          <HugeiconsIcon
            icon={ArrowDown01Icon}
            size={12}
            strokeWidth={2}
            className={cn(
              "transition-transform duration-150",
              row.isCollapsed && "-rotate-90",
            )}
          />
        </button>
      ) : (
        <span className="me-1 w-4 shrink-0" aria-hidden="true" />
      )}
      <button
        ref={ref}
        type="button"
        tabIndex={tabbable ? 0 : -1}
        onClick={() => onClick(row.heading)}
        title={row.heading.text}
        role="treeitem"
        aria-selected={active}
        aria-expanded={row.hasChildren ? !row.isCollapsed : undefined}
        aria-current={active ? "location" : undefined}
        className={cn(
          "min-w-0 flex-1 truncate rounded px-1 py-1 text-left text-[13px] leading-5 outline-none transition-colors",
          "focus-visible:ring-1 focus-visible:ring-ring",
          active
            ? "bg-primary/10 font-medium text-primary"
            : cn(depthTone(row.depth), "hover:bg-accent hover:text-accent-foreground"),
        )}
      >
        {row.heading.text}
        {row.isCollapsed && row.hiddenCount > 0 && (
          <span className="ms-1 text-[11px] text-muted-foreground/70">
            +{row.hiddenCount}
          </span>
        )}
      </button>
    </div>
  );
}

const DepthMenu: FC<{
  maxHeadingLevel: number;
  onChange: (level: number) => void;
  canCollapseAll: boolean;
  canExpandAll: boolean;
  onCollapseAll: () => void;
  onExpandAll: () => void;
  onHide: () => void;
}> = memo(function DepthMenu({
  maxHeadingLevel,
  onChange,
  canCollapseAll,
  canExpandAll,
  onCollapseAll,
  onExpandAll,
  onHide,
}) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const close = (event: MouseEvent) => {
      if (!ref.current?.contains(event.target as Node)) setOpen(false);
    };
    document.addEventListener("mousedown", close);
    return () => document.removeEventListener("mousedown", close);
  }, [open]);

  return (
    <div ref={ref} className="relative shrink-0">
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        aria-haspopup="menu"
        aria-expanded={open}
        title="Outline options"
        className="flex items-center gap-1 rounded px-1.5 py-0.5 text-[11px] font-medium uppercase tracking-wide text-muted-foreground/70 hover:bg-accent hover:text-accent-foreground"
      >
        {formatDepthLabel(maxHeadingLevel)}
        <HugeiconsIcon icon={ArrowDown01Icon} size={10} strokeWidth={2.5} />
      </button>
      {open && (
        <div
          role="menu"
          className="absolute end-0 top-full z-20 mt-1 w-44 rounded-md border border-border bg-popover p-1 shadow-md"
        >
          {MAX_LEVELS.map((level) => (
            <button
              key={level}
              type="button"
              role="menuitemradio"
              aria-checked={level === maxHeadingLevel}
              onClick={() => {
                onChange(level);
                setOpen(false);
              }}
              className={cn(
                "flex w-full items-center rounded px-2 py-1 text-left text-[12px] hover:bg-accent hover:text-accent-foreground",
                level === maxHeadingLevel
                  ? "font-medium text-foreground"
                  : "text-muted-foreground",
              )}
            >
              {formatDepthLabel(level)}
            </button>
          ))}
          <div className="my-1 h-px bg-border" />
          <button
            type="button"
            role="menuitem"
            disabled={!canCollapseAll}
            onClick={() => {
              onCollapseAll();
              setOpen(false);
            }}
            className="w-full rounded px-2 py-1 text-left text-[12px] hover:bg-accent hover:text-accent-foreground disabled:opacity-40"
          >
            Collapse all
          </button>
          <button
            type="button"
            role="menuitem"
            disabled={!canExpandAll}
            onClick={() => {
              onExpandAll();
              setOpen(false);
            }}
            className="w-full rounded px-2 py-1 text-left text-[12px] hover:bg-accent hover:text-accent-foreground disabled:opacity-40"
          >
            Expand all
          </button>
          <div className="my-1 h-px bg-border" />
          <button
            type="button"
            role="menuitem"
            onClick={() => {
              onHide();
              setOpen(false);
            }}
            className="w-full rounded px-2 py-1 text-left text-[12px] hover:bg-accent hover:text-accent-foreground"
          >
            Hide outline
          </button>
        </div>
      )}
    </div>
  );
});

interface OnThisPageProps {
  headings: TocHeading[];
  activeHeadingId?: string;
  onNavigate: (heading: TocHeading) => void;
  /** Rendered when the panel is hidden, so it can be reopened. */
  onHide?: () => void;
}

export const OnThisPage: FC<OnThisPageProps> = memo(function OnThisPage({
  headings,
  activeHeadingId,
  onNavigate,
  onHide,
}: OnThisPageProps) {
  const [collapsedKeys, setCollapsedKeys] = useState<Set<string>>(() => new Set());
  const [maxHeadingLevel, setMaxHeadingLevel] = useState(3);

  const visible = useMemo(
    () => headings.filter((heading) => heading.level <= maxHeadingLevel),
    [headings, maxHeadingLevel],
  );
  const model = useMemo(
    () => buildOutlineModel(visible, collapsedKeys),
    [visible, collapsedKeys],
  );
  const activeRowId =
    (activeHeadingId && model.visibleIdFor.get(activeHeadingId)) || activeHeadingId;

  const toggleCollapsed = useCallback((key: string) => {
    setCollapsedKeys((prev) => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  }, []);

  const collapsedParentCount = model.parentKeys.filter((key) =>
    collapsedKeys.has(key),
  ).length;
  const hiddenCount = headings.length - visible.length;
  const showCollapseAll = model.parentKeys.length > 0;

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex items-center justify-between gap-1 border-b border-border/60 pb-1.5">
        <span className="truncate text-[10px] font-semibold uppercase tracking-[0.08em] text-muted-foreground/80">
          On this page
        </span>
        <div className="flex items-center gap-0.5">
          <DepthMenu
            maxHeadingLevel={maxHeadingLevel}
            onChange={setMaxHeadingLevel}
            canCollapseAll={showCollapseAll && collapsedParentCount < model.parentKeys.length}
            canExpandAll={collapsedParentCount > 0}
            onCollapseAll={() => setCollapsedKeys(new Set(model.parentKeys))}
            onExpandAll={() => setCollapsedKeys(new Set())}
            onHide={() => onHide?.()}
          />
          {onHide && (
            <button
              type="button"
              onClick={onHide}
              title="Hide outline"
              className="rounded p-0.5 text-muted-foreground/70 hover:bg-accent hover:text-accent-foreground"
            >
              <HugeiconsIcon icon={Cancel01Icon} size={11} strokeWidth={2.5} />
            </button>
          )}
        </div>
      </div>

      {visible.length === 0 ? (
        <div className="px-2 py-3 text-[12px] text-muted-foreground">
          {hiddenCount > 0 ? (
            <button
              type="button"
              className="underline hover:text-foreground"
              onClick={() => setMaxHeadingLevel(6)}
            >
              {hiddenCount} deeper heading{hiddenCount === 1 ? "" : "s"} hidden — show all
            </button>
          ) : (
            "No headings"
          )}
        </div>
      ) : (
        <OutlineList
          rows={model.rows}
          activeRowId={activeRowId}
          onHeadingClick={onNavigate}
          onToggleCollapsed={toggleCollapsed}
        />
      )}
    </div>
  );
});
