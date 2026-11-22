import { type ReactNode, useEffect, useState } from "react";
import { renderDocxNodes } from "./lib/docxHtml";
import {
  MAX_OFFICE_PREVIEW_BYTES,
  readBinary,
  validateOfficeArchive,
} from "./lib/externalFormat";
import { useFileRevision } from "./lib/useFileRevision";

type Props = { path: string; size: number };

type State = { path: string; content?: string; error?: string };

// Reading goes through IPC: the webview cannot fetch `asset:`/`http://asset.localhost`
// URLs under the CSP, and convertFileSrc() encodes the path in a way the
// filesystem layer cannot resolve.
export function DocxPreview({ path, size }: Props) {
  const [state, setState] = useState<State>({ path });
  const revision = useFileRevision(path);

  // biome-ignore lint/correctness/useExhaustiveDependencies: revision re-reads the file after it changed on disk.
  useEffect(() => {
    let cancelled = false;
    // A reload after a disk change keeps the current render until it is done.
    setState((prev) => (prev.path === path ? prev : { path }));
    if (size > MAX_OFFICE_PREVIEW_BYTES) {
      setState({ path, error: "DOCX preview is limited to 20 MB." });
      return;
    }

    void (async () => {
      try {
        const buffer = await readBinary(path);
        if (buffer.byteLength > MAX_OFFICE_PREVIEW_BYTES) {
          throw new Error("DOCX preview is limited to 20 MB.");
        }
        validateOfficeArchive(buffer, "DOCX");
        const mammoth = await import("mammoth");
        const converter = mammoth.default;
        // mammoth's browser build reads `input.arrayBuffer` as a value, not a
        // function; handing it a function makes unzip reject with
        // "Could not find file in options".
        const result = await (converter as any).convertToHtml(
          { arrayBuffer: buffer },
          { externalFileAccess: false },
        );
        if (!cancelled) setState({ path, content: result.value });
      } catch (error) {
        if (!cancelled) {
          const message =
            error instanceof Error ? error.message : String(error);
          // A reload can catch the file mid-write; keep the last good render.
          setState((prev) =>
            prev.path === path && prev.content
              ? prev
              : { path, error: message },
          );
        }
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [path, size, revision]);

  if (state.path !== path || (!state.content && !state.error)) {
    return <PreviewMessage>Loading Word document…</PreviewMessage>;
  }
  if (state.error) return <PreviewMessage>{state.error}</PreviewMessage>;

  return (
    <div className="h-full w-full overflow-auto bg-background px-8 py-6">
      <article className="select-text mx-auto max-w-4xl space-y-3 text-sm leading-7 text-foreground [&_h1]:text-2xl [&_h1]:font-semibold [&_h2]:text-xl [&_h2]:font-semibold [&_h3]:text-lg [&_h3]:font-semibold [&_a]:text-primary [&_a]:underline [&_blockquote]:border-l-2 [&_blockquote]:border-border [&_blockquote]:pl-4 [&_li]:ml-6 [&_ol]:list-decimal [&_p]:whitespace-pre-wrap [&_table]:w-full [&_table]:border-collapse [&_td]:border [&_td]:border-border [&_td]:p-2 [&_th]:border [&_th]:border-border [&_th]:bg-muted [&_th]:p-2 [&_ul]:list-disc">
        {state.content ? renderSafeHtml(state.content) : null}
      </article>
    </div>
  );
}

function PreviewMessage({ children }: { children: ReactNode }) {
  return (
    <div className="flex h-full items-center justify-center px-6 text-center text-xs text-muted-foreground">
      {children}
    </div>
  );
}

function renderSafeHtml(html: string): ReactNode[] {
  const doc = new DOMParser().parseFromString(html, "text/html");
  return renderDocxNodes(doc.body.childNodes);
}
