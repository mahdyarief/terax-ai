import { type RefObject, useEffect, useRef, useState } from "react";
import { findMatches, stepMatch } from "./lib/findInPreview";

const ALL = "terax-md-find";
const CURRENT = "terax-md-find-current";

/** The CSS Custom Highlight API paints matches without touching the DOM. */
function highlights(): HighlightRegistry | null {
  return typeof Highlight !== "undefined" && typeof CSS !== "undefined"
    ? (CSS.highlights ?? null)
    : null;
}

function textNodesOf(root: HTMLElement): Text[] {
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
  const nodes: Text[] = [];
  for (let node = walker.nextNode(); node; node = walker.nextNode()) {
    if (node.nodeValue) nodes.push(node as Text);
  }
  return nodes;
}

type Props = {
  /** The rendered document to search. */
  root: RefObject<HTMLElement | null>;
  /** Changes whenever the document re-renders, so matches are recomputed. */
  revision: unknown;
  onClose: () => void;
};

export function MarkdownFindBar({ root, revision, onClose }: Props) {
  const [query, setQuery] = useState("");
  const [total, setTotal] = useState(0);
  const [current, setCurrent] = useState(-1);
  const ranges = useRef<Range[]>([]);
  const input = useRef<HTMLInputElement>(null);

  useEffect(() => {
    input.current?.focus();
    return () => {
      highlights()?.delete(ALL);
      highlights()?.delete(CURRENT);
    };
  }, []);

  // biome-ignore lint/correctness/useExhaustiveDependencies: revision re-runs the search after the document re-renders.
  useEffect(() => {
    // Wait a frame so a re-rendered document is in the DOM before it is walked.
    const frame = requestAnimationFrame(() => {
      const el = root.current;
      const nodes = el ? textNodesOf(el) : [];
      const found = findMatches(
        nodes.map((node) => node.nodeValue ?? ""),
        query,
      );
      ranges.current = found.map((match) => {
        const range = document.createRange();
        range.setStart(nodes[match.node], match.start);
        range.setEnd(nodes[match.node], match.end);
        return range;
      });
      const registry = highlights();
      if (registry) {
        if (ranges.current.length > 0) {
          registry.set(ALL, new Highlight(...ranges.current));
        } else {
          registry.delete(ALL);
        }
      }
      setTotal(ranges.current.length);
      setCurrent(ranges.current.length > 0 ? 0 : -1);
    });
    return () => cancelAnimationFrame(frame);
  }, [query, revision, root]);

  // biome-ignore lint/correctness/useExhaustiveDependencies: total changes when the ranges were rebuilt.
  useEffect(() => {
    const range = ranges.current[current];
    const registry = highlights();
    if (!range) {
      registry?.delete(CURRENT);
      return;
    }
    if (registry) {
      registry.set(CURRENT, new Highlight(range));
    } else {
      // No highlight support: select the match so it is still visible.
      const selection = window.getSelection();
      selection?.removeAllRanges();
      selection?.addRange(range);
    }
    range.startContainer.parentElement?.scrollIntoView({
      block: "center",
      inline: "nearest",
    });
  }, [current, total]);

  const step = (delta: 1 | -1) =>
    setCurrent((index) => stepMatch(index, ranges.current.length, delta));

  return (
    <search
      className="absolute right-3 top-3 z-20 flex items-center gap-1 rounded-md border border-border bg-popover px-1.5 py-1 text-[12px] shadow-md"
    >
      <input
        ref={input}
        value={query}
        onChange={(event) => setQuery(event.target.value)}
        onKeyDown={(event) => {
          if (event.key === "Enter") {
            event.preventDefault();
            step(event.shiftKey ? -1 : 1);
          } else if (event.key === "Escape") {
            event.preventDefault();
            onClose();
          }
        }}
        placeholder="Find in document"
        aria-label="Find in document"
        spellCheck={false}
        className="h-6 w-44 rounded bg-transparent px-1.5 outline-none placeholder:text-muted-foreground/60"
      />
      <span
        aria-live="polite"
        className="min-w-[3.25rem] text-center tabular-nums text-muted-foreground"
      >
        {query ? `${total === 0 ? 0 : current + 1}/${total}` : ""}
      </span>
      <button
        type="button"
        onClick={() => step(-1)}
        disabled={total === 0}
        title="Previous match (Shift+Enter)"
        aria-label="Previous match"
        className="rounded px-1.5 py-0.5 text-muted-foreground hover:bg-accent hover:text-foreground disabled:opacity-40"
      >
        ↑
      </button>
      <button
        type="button"
        onClick={() => step(1)}
        disabled={total === 0}
        title="Next match (Enter)"
        aria-label="Next match"
        className="rounded px-1.5 py-0.5 text-muted-foreground hover:bg-accent hover:text-foreground disabled:opacity-40"
      >
        ↓
      </button>
      <button
        type="button"
        onClick={onClose}
        title="Close (Esc)"
        aria-label="Close find"
        className="rounded px-1.5 py-0.5 text-muted-foreground hover:bg-accent hover:text-foreground"
      >
        ✕
      </button>
    </search>
  );
}
