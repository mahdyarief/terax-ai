/** What the diagram file preview shows. */
export type MermaidPreviewState = {
  /** Last diagram that rendered, kept on screen while the source is broken. */
  svg: string | null;
  /** First line of the latest render error; null once the source renders. */
  error: string | null;
  /** A render is in flight and nothing has rendered yet. */
  loading: boolean;
};

export type MermaidRenderResult =
  | { kind: "ok"; svg: string }
  | { kind: "error"; message: string }
  | { kind: "empty" };

export const initialMermaidPreview: MermaidPreviewState = {
  svg: null,
  error: null,
  loading: true,
};

/**
 * Folds one render result into the preview. A broken edit keeps the last good
 * diagram instead of replacing it with the error, so typing through an
 * invalid intermediate state does not flash the whole pane.
 */
export function applyMermaidResult(
  prev: MermaidPreviewState,
  result: MermaidRenderResult,
): MermaidPreviewState {
  switch (result.kind) {
    case "ok":
      return { svg: result.svg, error: null, loading: false };
    case "empty":
      return { svg: null, error: null, loading: false };
    case "error":
      return {
        svg: prev.svg,
        error: result.message.split("\n")[0] || "Unknown error",
        loading: false,
      };
  }
}
