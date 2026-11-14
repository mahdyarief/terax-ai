import { cn } from "@/lib/utils";
import {
  HoverCard,
  HoverCardContent,
  HoverCardTrigger,
} from "@/components/ui/hover-card";
import { memo, type ReactNode } from "react";
import { explorerGitTextClass } from "./lib/gitStatusColor";
import type { GitStatusCode } from "./lib/gitStatusUtils";

const ELLIPSIS = "\u2026";

// Pure: middle-truncate so both the path root and its leaf stay visible. The
// card's path line uses this instead of CSS end-truncation, which would hide
// the filename — the part a user looking at a file row actually cares about.
export function truncateMiddle(value: string, maxChars: number): string {
  if (maxChars <= 0) return "";
  if (value.length <= maxChars) return value;
  if (maxChars === 1) return ELLIPSIS;
  const keep = maxChars - 1;
  const head = Math.ceil(keep / 2);
  const tail = keep - head;
  return tail > 0
    ? `${value.slice(0, head)}${ELLIPSIS}${value.slice(-tail)}`
    : `${value.slice(0, head)}${ELLIPSIS}`;
}

const GIT_STATUS_LABEL: Record<GitStatusCode, string> = {
  M: "Modified",
  A: "Added",
  D: "Deleted",
  U: "Untracked",
  R: "Renamed",
};

// Pure: human label for a git status code, or null when there is none.
export function gitStatusLabel(
  code: GitStatusCode | null | undefined,
): string | null {
  return code ? GIT_STATUS_LABEL[code] : null;
}

export type FileRowHoverCardProps = {
  path: string;
  name: string;
  gitStatusCode?: GitStatusCode | null;
  gitignored?: boolean;
  /** Opens the file. Same identity as the row's own click handler, so the
   * card is a strict alternative entry point and adds no new I/O. */
  onOpenFile: (path: string) => void;
  /** The real row (a plain button) cloned by `asChild`, so hover-card props
   * land on the existing element — no wrapper node, no layout change. */
  children: ReactNode;
};

// Wraps an existing entry row so a hover card reveals read-only metadata from
// props only (path, git status) plus a single "Open" action. No async work.
//
// Anti-flicker decisions:
// - Trigger is `asChild` on the row's own button: hover is tracked on a plain,
//   stable element whose geometry never changes, so a reveal can't move the
//   hitbox out from under the cursor (failure mode 2).
// - The card lives in a portal (Radix), and Radix's safe-zone bridge keeps the
//   pointer inside trigger/card/bridge from closing it (failure mode 3).
// - A generous openDelay avoids opening on incidental passes; closeDelay
//   absorbs the cursor crossing the gap toward the card.
const OPEN_DELAY_MS = 350;
const CLOSE_DELAY_MS = 120;

function FileRowHoverCardImpl({
  path,
  name,
  gitStatusCode,
  gitignored = false,
  onOpenFile,
  children,
}: FileRowHoverCardProps) {
  const status = gitStatusLabel(gitStatusCode);

  return (
    <HoverCard openDelay={OPEN_DELAY_MS} closeDelay={CLOSE_DELAY_MS}>
      <HoverCardTrigger asChild>{children}</HoverCardTrigger>
      <HoverCardContent
        align="start"
        side="right"
        sideOffset={8}
        collisionPadding={8}
        className="w-72 max-w-[min(18rem,80vw)] rounded-lg p-2.5"
      >
        <div className="flex min-w-0 flex-col gap-1">
          <span className="truncate text-[13px] font-medium text-popover-foreground">
            {name}
          </span>
          <span
            className="truncate font-mono text-[11px] text-muted-foreground"
            title={path}
          >
            {truncateMiddle(path, 44)}
          </span>
          <div className="flex items-center gap-2 pt-0.5 text-[11px]">
            {status && gitStatusCode ? (
              <span
                className={cn(
                  "inline-flex items-center gap-1",
                  explorerGitTextClass(gitStatusCode),
                )}
              >
                <span className="size-1.5 rounded-full bg-current" aria-hidden />
                {status}
              </span>
            ) : null}
            {gitignored ? (
              <span className="text-muted-foreground/70">Ignored</span>
            ) : null}
            <button
              type="button"
              onClick={() => onOpenFile(path)}
              className="ml-auto shrink-0 rounded-sm border border-border px-2 py-0.5 text-[11px] text-popover-foreground hover:bg-accent"
            >
              Open
            </button>
          </div>
        </div>
      </HoverCardContent>
    </HoverCard>
  );
}

export const FileRowHoverCard = memo(FileRowHoverCardImpl);
