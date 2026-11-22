import { useEffect, useId, useState } from "react";

type Mermaid = typeof import("mermaid").default;

let mermaidPromise: Promise<Mermaid> | null = null;

/** Mermaid is large, so it loads only when a document has a diagram. */
export function loadMermaid(): Promise<Mermaid> {
  mermaidPromise ??= import("mermaid").then((module) => module.default);
  return mermaidPromise;
}

type State =
  | { kind: "loading" }
  | { kind: "ready"; svg: string }
  | { kind: "error"; message: string };

/** Renders a ```mermaid fence as a diagram, or as its source if it is invalid. */
export function MermaidBlock({ code }: { code: string }) {
  const id = `terax-mermaid-${useId().replace(/[^a-zA-Z0-9]/g, "")}`;
  const [state, setState] = useState<State>({ kind: "loading" });

  useEffect(() => {
    let alive = true;
    void (async () => {
      try {
        const mermaid = await loadMermaid();
        mermaid.initialize({
          startOnLoad: false,
          // "strict" sanitizes labels and disables click handlers.
          securityLevel: "strict",
          suppressErrorRendering: true,
          theme: document.documentElement.classList.contains("dark")
            ? "dark"
            : "default",
        });
        const { svg } = await mermaid.render(id, code);
        if (alive) setState({ kind: "ready", svg });
      } catch (error) {
        // A failed render can leave its scratch element behind.
        document.getElementById(id)?.remove();
        document.getElementById(`d${id}`)?.remove();
        if (alive) {
          setState({
            kind: "error",
            message: error instanceof Error ? error.message : String(error),
          });
        }
      }
    })();
    return () => {
      alive = false;
    };
  }, [code, id]);

  if (state.kind === "ready") {
    return (
      <div
        className="my-4 flex justify-center overflow-x-auto [&_svg]:h-auto [&_svg]:max-w-full"
        // biome-ignore lint/security/noDangerouslySetInnerHtml: SVG produced by mermaid in strict mode, which sanitizes it.
        dangerouslySetInnerHTML={{ __html: state.svg }}
      />
    );
  }
  return (
    <div className="my-4 rounded-md border border-border/60 bg-muted/30 p-3 text-[12px]">
      {state.kind === "error" ? (
        <p role="alert" className="mb-2 text-destructive">
          Diagram could not be rendered: {state.message.split("\n")[0]}
        </p>
      ) : (
        <p className="mb-2 text-muted-foreground">Rendering diagram…</p>
      )}
      <pre className="overflow-x-auto font-mono text-[11.5px] text-muted-foreground">
        {code}
      </pre>
    </div>
  );
}
