import { revealInFinder } from "@/modules/explorer/lib/contextActions";
import { currentWorkspaceEnv } from "@/modules/workspace";
import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";

type HeadResult = { content: string; truncated: boolean; size: number };

type State =
  | { path: string; status: "loading" }
  | { path: string; status: "ready"; head: HeadResult }
  | { path: string; status: "error"; message: string };

type Props = {
  path: string;
  /** Formatted size of the whole file. */
  sizeLabel: string;
  /** Opens the whole file in the editor, when it is small enough to force. */
  onOpenAnyway?: () => void;
};

/**
 * How much of an oversized file is shown. Minified JSON is a single line, and
 * laying out much more than this as one wrapped line stalls the webview.
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

/**
 * The beginning of a text file that is too large for the editor, read-only.
 * Only the start is read, so the size of the file does not matter.
 */
export function TextHeadPreview({ path, sizeLabel, onOpenAnyway }: Props) {
  const [state, setState] = useState<State>({ path, status: "loading" });

  useEffect(() => {
    let cancelled = false;
    setState({ path, status: "loading" });
    invoke<HeadResult>("fs_read_head", {
      path,
      workspace: currentWorkspaceEnv(),
      maxBytes: TEXT_HEAD_BYTES,
      partialLine: true,
    })
      .then((head) => {
        if (!cancelled) setState({ path, status: "ready", head });
      })
      .catch((error: unknown) => {
        if (!cancelled) {
          setState({
            path,
            status: "error",
            message: error instanceof Error ? error.message : String(error),
          });
        }
      });
    return () => {
      cancelled = true;
    };
  }, [path]);

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

  const { head } = state;
  // Encoded length, since that is what "the first N" of a file means.
  const shown = new TextEncoder().encode(head.content).length;
  return (
    <div className="flex h-full min-h-0 flex-col bg-background">
      <div className="flex shrink-0 items-center justify-between gap-3 border-b border-border bg-muted/60 px-3 py-1.5 text-xs text-muted-foreground">
        <span>{headSummary(shown, sizeLabel, head.truncated)}</span>
        {actions}
      </div>
      <pre className="select-text min-h-0 flex-1 overflow-auto whitespace-pre-wrap break-all px-4 py-3 font-mono text-[12px] leading-5 text-foreground">
        {head.content}
        {head.truncated && (
          <span className="select-none text-muted-foreground">
            {"\n"}… the rest of the file is not shown
          </span>
        )}
      </pre>
    </div>
  );
}
