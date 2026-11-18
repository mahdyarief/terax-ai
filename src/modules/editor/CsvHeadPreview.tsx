import { currentWorkspaceEnv } from "@/modules/workspace";
import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";
import { CsvPreview } from "./CsvPreview";
import { delimitedTextKind } from "./lib/csvPreview";

type HeadResult = { content: string; truncated: boolean; size: number };

type State =
  | { path: string; status: "loading" }
  | { path: string; status: "ready"; head: HeadResult }
  | { path: string; status: "error"; message: string };

type Props = {
  path: string;
  /** Formatted file size for the summary line. */
  sizeLabel: string;
  /** Opens the whole file as text, when it is small enough to force. */
  onOpenAnyway?: () => void;
};

/**
 * The first rows of a delimited file that is too large for the editor. Only
 * the start of the file is read, so the size of the file does not matter.
 */
export function CsvHeadPreview({ path, sizeLabel, onOpenAnyway }: Props) {
  const [state, setState] = useState<State>({ path, status: "loading" });

  useEffect(() => {
    let cancelled = false;
    setState({ path, status: "loading" });
    invoke<HeadResult>("fs_read_head", {
      path,
      workspace: currentWorkspaceEnv(),
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

  if (state.path !== path || state.status === "loading") {
    return <Message>Loading the first rows…</Message>;
  }
  if (state.status === "error") {
    return <Message>Preview unavailable: {state.message}</Message>;
  }
  return (
    <CsvPreview
      content={state.head.content}
      delimiter={delimitedTextKind(path, state.head.content) ?? ","}
      partialOf={state.head.truncated ? sizeLabel : undefined}
      onEdit={onOpenAnyway}
      editLabel="Open whole file as text"
    />
  );
}

function Message({ children }: { children: React.ReactNode }) {
  return (
    <div className="flex h-full items-center justify-center px-6 text-center text-xs text-muted-foreground">
      {children}
    </div>
  );
}
