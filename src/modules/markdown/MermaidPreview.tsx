import { useEffect, useId, useReducer, useRef, useState } from "react";
import {
  applyMermaidResult,
  initialMermaidPreview,
  type MermaidRenderResult,
} from "./lib/mermaidPreview";
import { loadMermaid } from "./MermaidBlock";

function isDark(): boolean {
  return document.documentElement.classList.contains("dark");
}

/** Follows the app's light/dark switch, so the diagram is redrawn to match. */
function useDarkMode(): boolean {
  const [dark, setDark] = useState(isDark);
  useEffect(() => {
    const observer = new MutationObserver(() => setDark(isDark()));
    observer.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ["class"],
    });
    return () => observer.disconnect();
  }, []);
  return dark;
}

/**
 * Renders a whole `.mmd` / `.mermaid` file as one diagram, from the live
 * editor buffer. Mermaid runs in strict mode, which sanitizes labels and
 * disables click handlers, as for diagrams inside Markdown.
 */
export function MermaidPreview({ code }: { code: string }) {
  const baseId = `terax-mermaid-file-${useId().replace(/[^a-zA-Z0-9]/g, "")}`;
  const dark = useDarkMode();
  const [state, dispatch] = useReducer(
    applyMermaidResult,
    initialMermaidPreview,
  );
  // Each render gets its own scratch id, so a slow render that is overtaken
  // by a newer one never collides with it in the DOM.
  const seq = useRef(0);

  useEffect(() => {
    if (!code.trim()) {
      dispatch({ kind: "empty" });
      return;
    }
    let alive = true;
    const id = `${baseId}-${++seq.current}`;
    void (async () => {
      let result: MermaidRenderResult;
      try {
        const mermaid = await loadMermaid();
        mermaid.initialize({
          startOnLoad: false,
          securityLevel: "strict",
          suppressErrorRendering: true,
          theme: dark ? "dark" : "default",
        });
        const { svg } = await mermaid.render(id, code);
        result = { kind: "ok", svg };
      } catch (error) {
        // A failed render can leave its scratch element behind.
        document.getElementById(id)?.remove();
        document.getElementById(`d${id}`)?.remove();
        result = {
          kind: "error",
          message: error instanceof Error ? error.message : String(error),
        };
      }
      if (alive) dispatch(result);
    })();
    return () => {
      alive = false;
    };
  }, [code, dark, baseId]);

  return (
    <div className="terax-visible-scrollbar relative h-full min-h-0 overflow-auto bg-background">
      {state.error && (
        <div
          role="alert"
          // Opaque instead of a backdrop blur: a blur promotes a compositing
          // layer (see compositingBudget.test.ts).
          className="sticky top-0 z-10 border-b border-destructive/30 bg-[color-mix(in_srgb,var(--destructive)_10%,var(--background))] px-4 py-1.5 pe-40 text-[11.5px] text-destructive"
        >
          Diagram could not be rendered: {state.error}
          {state.svg && (
            <span className="text-muted-foreground">
              {" "}
              · showing the last version that rendered
            </span>
          )}
        </div>
      )}
      {state.svg ? (
        <div
          className="flex min-h-full items-start justify-center p-6 pt-12 [&_svg]:h-auto [&_svg]:max-w-full"
          // biome-ignore lint/security/noDangerouslySetInnerHtml: SVG produced by mermaid in strict mode, which sanitizes it.
          dangerouslySetInnerHTML={{ __html: state.svg }}
        />
      ) : (
        !state.error && (
          <p className="p-6 pt-12 text-center text-xs text-muted-foreground">
            {state.loading ? "Rendering diagram…" : "Empty diagram"}
          </p>
        )
      )}
    </div>
  );
}
