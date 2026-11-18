import { useEffect, useState } from "react";

type Katex = typeof import("katex").default;

let katexPromise: Promise<Katex> | null = null;

/** KaTeX and its stylesheet load only when a document contains math. */
function loadKatex(): Promise<Katex> {
  katexPromise ??= Promise.all([
    import("katex"),
    import("katex/dist/katex.min.css"),
  ]).then(([module]) => module.default);
  return katexPromise;
}

/** Typesets TeX; shows the source until KaTeX has loaded or if it fails. */
export function MathSpan({ tex, display }: { tex: string; display: boolean }) {
  const [html, setHtml] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    loadKatex()
      .then((katex) => {
        if (!alive) return;
        setHtml(
          katex.renderToString(tex, {
            displayMode: display,
            // Invalid TeX renders in red in place instead of throwing.
            throwOnError: false,
          }),
        );
      })
      .catch(() => {
        if (alive) setHtml(null);
      });
    return () => {
      alive = false;
    };
  }, [tex, display]);

  const Tag = display ? "div" : "span";
  if (html === null) {
    return (
      <Tag className={display ? "my-4 text-center font-mono" : "font-mono"}>
        {tex}
      </Tag>
    );
  }
  return (
    <Tag
      className={display ? "my-4 overflow-x-auto" : undefined}
      // biome-ignore lint/security/noDangerouslySetInnerHtml: KaTeX output; it escapes the TeX it is given.
      dangerouslySetInnerHTML={{ __html: html }}
    />
  );
}
