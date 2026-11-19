import { revealInFinder } from "@/modules/explorer/lib/contextActions";
import { currentWorkspaceEnv } from "@/modules/workspace";
import { invoke } from "@tauri-apps/api/core";
import { useEffect, useRef, useState } from "react";

type HeadResult = {
  content: string;
  truncated: boolean;
  size: number;
  nextOffset: number | null;
};

type State =
  | { path: string; status: "loading" }
  | {
      path: string;
      status: "ready";
      content: string;
      size: number;
      truncated: boolean;
      nextOffset: number | null;
    }
  | { path: string; status: "error"; message: string };

type Props = {
  path: string;
  /** Formatted size of the whole file. */
  sizeLabel: string;
  /** Opens the whole file in the editor, when it is small enough to force. */
  onOpenAnyway?: () => void;
};

/**
 * How much of an oversized file is read per request. Minified JSON is a single
 * line, and laying out much more than this as one wrapped line stalls the
 * webview.
 */
export const TEXT_HEAD_BYTES = 256 * 1024;

export function formatHeadSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

/** What the bar above the text says about how much of the file is shown. */
export function headSummary(
  shownBytes: number,
  sizeLabel: string,
  truncated: boolean,
): string {
  return truncated
    ? `Showing the first ${formatHeadSize(shownBytes)} of ${sizeLabel} · read-only`
    : `${sizeLabel} · read-only`;
}

function readChunk(
  path: string,
  offset: number,
  maxBytes: number,
): Promise<HeadResult> {
  return invoke<HeadResult>("fs_read_head", {
    path,
    workspace: currentWorkspaceEnv(),
    maxBytes,
    partialLine: true,
    offset,
  });
}

/**
 * The beginning of a text file that is too large for the editor, read-only.
 * Pages are fetched as the reader scrolls, so the size of the file does not
 * matter and neither the editor nor the webview ever holds the whole file.
 */
export function TextHeadPreview({ path, sizeLabel, onOpenAnyway }: Props) {
  const [state, setState] = useState<State>({ path, status: "loading" });
  const scrollRef = useRef<HTMLPreElement>(null);
  const loadingRef = useRef(false);
  const offsetRef = useRef(0);

  useEffect(() => {
    let cancelled = false;
    setState({ path, status: "loading" });
    offsetRef.current = 0;
    loadingRef.current = false;

    readChunk(path, 0, TEXT_HEAD_BYTES)
      .then((head) => {
        if (cancelled) return;
        offsetRef.current = head.nextOffset ?? head.size;
        setState({
          path,
          status: "ready",
          content: head.content,
          size: head.size,
          truncated: head.truncated,
          nextOffset: head.nextOffset,
        });
      })
      .catch((error: unknown) => {
        if (cancelled) return;
        setState({
          path,
          status: "error",
          message: error instanceof Error ? error.message : String(error),
        });
      });

    return () => {
      cancelled = true;
    };
  }, [path]);

  const loadMore = async () => {
    if (loadingRef.current) return;
    const el = scrollRef.current;
    const start = offsetRef.current;
    loadingRef.current = true;
    try {
      const head = await readChunk(path, start, TEXT_HEAD_BYTES);
      offsetRef.current = head.nextOffset ?? start + TEXT_HEAD_BYTES;
      setState((prev) => {
        if (prev.status !== "ready" || prev.path !== path) return prev;
        return {
          ...prev,
          content: prev.content + head.content,
          truncated: head.truncated,
          nextOffset: head.nextOffset,
        };
      });
    } catch {
      // A failed page leaves the visible text alone; scrolling back in tries
      // the same offset again.
    } finally {
      loadingRef.current = false;
      requestAnimationFrame(() => {
        if (el) el.scrollTop = el.scrollHeight;
      });
    }
  };

  const onScroll = () => {
    const el = scrollRef.current;
    if (!el || state.status !== "ready" || !state.truncated) return;
    if (el.scrollHeight - el.scrollTop - el.clientHeight > 64) return;
    if (state.nextOffset === null) return;
    void loadMore();
  };

  const actions = (
    <div className="flex shrink-0 items-center gap-1.5">
      <button
        type="button"
        onClick={() => void revealInFinder(path)}
        className="rounded border border-border bg-background px-2 py-0.5 text-foreground hover:bg-accent"
      >
        Reveal in folder
      </button>
      {onOpenAnyway && (
        <button
          type="button"
          onClick={onOpenAnyway}
          title="Loads the whole file; syntax features stay off"
          className="rounded border border-border bg-background px-2 py-0.5 text-foreground hover:bg-accent"
        >
          Open anyway
        </button>
      )}
    </div>
  );

  if (state.path !== path || state.status === "loading") {
    return (
      <div className="flex h-full items-center justify-center text-xs text-muted-foreground">
        Loading the start of the file…
      </div>
    );
  }
  if (state.status === "error") {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-2 px-6 text-center text-xs">
        <div className="text-sm text-foreground">File too large</div>
        <div className="text-muted-foreground">
          {sizeLabel} · no preview ({state.message})
        </div>
        {actions}
      </div>
    );
  }

  return (
    <div className="flex h-full min-h-0 flex-col bg-background">
      <div className="flex shrink-0 items-center justify-between gap-3 border-b border-border bg-muted/60 px-3 py-1.5 text-xs text-muted-foreground">
        <span>
          {headSummary(
            new TextEncoder().encode(state.content).length,
            sizeLabel,
            state.truncated,
          )}
        </span>
        {actions}
      </div>
      <pre
        ref={scrollRef}
        onScroll={onScroll}
        className="select-text min-h-0 flex-1 overflow-auto whitespace-pre-wrap break-all px-4 py-3 font-mono text-[12px] leading-5 text-foreground"
      >
        {state.content}
        {state.truncated && (
          <span className="select-none text-muted-foreground">
            {"\n"}… the rest of the file is not shown
          </span>
        )}
      </pre>
    </div>
  );
}