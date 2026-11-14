import { MarkdownCode } from "@/components/ai-elements/markdown-code";
import { OnThisPage } from "@/components/markdown/OnThisPage";
import {
  parseHeadings,
  type TocHeading,
} from "@/components/markdown/useMarkdownHeadings";
import { checkReadable } from "@/modules/ai/lib/security";
import { cn } from "@/lib/utils";
import { useAssetGrant } from "@/lib/useAssetGrant";
import {
  memo,
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { defaultRehypePlugins, Streamdown } from "streamdown";
import { previewBoundary, rehypeLocalImages } from "./lib/assetPath";
import { stampHeadingIds } from "./lib/stampHeadingIds";
import { MarkdownLink } from "./MarkdownLink";
import { useMarkdownDocument } from "./useMarkdownDocument";

type Props = {
  path: string;
  /** Images load only from beneath this folder (the document's own folder if it lies outside). */
  workspaceRoot?: string | null;
  visible: boolean;
  /** Omitted when there is no raw editor to switch back to (e.g. AI diff panes). */
  onSetView?: (mode: "rendered" | "raw") => void;
};

const components = { a: MarkdownLink, code: MarkdownCode };

/** Below this pane width the outline would squeeze the text, so it hides. */
const OUTLINE_MIN_WIDTH = 480;
/** A heading within this many px of the top edge counts as the one being read. */
const ACTIVE_HEADING_OFFSET = 48;

export function MarkdownPreviewPane({
  path,
  workspaceRoot = null,
  visible,
  onSetView,
}: Props) {
  const status = useMarkdownDocument(path, visible);
  const scrollRef = useRef<HTMLDivElement>(null);
  const wrapRef = useRef<HTMLDivElement>(null);
  const scrollTopRef = useRef(0);
  const frameRef = useRef(0);
  const content = status.kind === "ready" ? status.content : null;
  const headings = useMemo(
    () => (content ? parseHeadings(content) : []),
    [content],
  );
  const [activeHeadingId, setActiveHeadingId] = useState<string | undefined>();
  const [outlineHidden, setOutlineHidden] = useState(false);
  const [wide, setWide] = useState(true);

  // Prefer the heading currently at the top of the viewport, so the outline
  // highlights what the reader is looking at rather than where they clicked.
  const trackActiveHeading = useCallback(() => {
    const el = scrollRef.current;
    if (!el) return;
    const nodes = el.querySelectorAll<HTMLElement>("[data-heading-id]");
    if (nodes.length === 0) return;
    const edge = el.getBoundingClientRect().top + ACTIVE_HEADING_OFFSET;
    let current = nodes[0].dataset.headingId;
    for (const node of nodes) {
      if (node.getBoundingClientRect().top <= edge) {
        current = node.dataset.headingId;
      } else break;
    }
    setActiveHeadingId((prev) => (prev === current ? prev : current));
  }, []);

  const onScroll = useCallback(() => {
    const el = scrollRef.current;
    scrollTopRef.current = el?.scrollTop ?? 0;
    if (frameRef.current) return;
    frameRef.current = window.requestAnimationFrame(() => {
      frameRef.current = 0;
      trackActiveHeading();
    });
  }, [trackActiveHeading]);

  useEffect(() => () => window.cancelAnimationFrame(frameRef.current), []);

  // The outline only fits when the pane has room for text plus the sidebar.
  useEffect(() => {
    const el = wrapRef.current;
    if (!el) return;
    const observer = new ResizeObserver(([entry]) => {
      setWide(entry.contentRect.width >= OUTLINE_MIN_WIDTH);
    });
    observer.observe(el);
    return () => observer.disconnect();
  }, []);

  // A retargeted preview starts at the top, not at the old document's offset.
  // biome-ignore lint/correctness/useExhaustiveDependencies: path is the trigger.
  useLayoutEffect(() => {
    scrollTopRef.current = 0;
    setActiveHeadingId(undefined);
    if (scrollRef.current) scrollRef.current.scrollTop = 0;
  }, [path]);

  // New text replaces blocks in place; put the view back where the reader was
  // in case the swap collapsed the document for a frame.
  useLayoutEffect(() => {
    const el = scrollRef.current;
    if (el && content !== null && el.scrollTop !== scrollTopRef.current) {
      el.scrollTop = scrollTopRef.current;
    }
  }, [content]);

  // Heading ids are matched against the rendered DOM in document order, so they
  // have to be reapplied whenever Streamdown re-renders a block.
  useLayoutEffect(() => {
    const el = scrollRef.current;
    if (!el || content === null) return;
    let scheduled = 0;
    const stamp = () => {
      if (scheduled) return;
      scheduled = window.requestAnimationFrame(() => {
        scheduled = 0;
        stampHeadingIds(el, headings);
        trackActiveHeading();
      });
    };
    stamp();
    const observer = new MutationObserver(stamp);
    observer.observe(el, { childList: true, subtree: true });
    return () => {
      observer.disconnect();
      window.cancelAnimationFrame(scheduled);
    };
  }, [content, headings, trackActiveHeading]);

  const navigateTo = useCallback((heading: TocHeading) => {
    const target = scrollRef.current?.querySelector<HTMLElement>(
      `[data-heading-id="${CSS.escape(heading.id)}"]`,
    );
    if (target) {
      target.scrollIntoView({ behavior: "smooth", block: "start" });
      setActiveHeadingId(heading.id);
    } else {
      scrollRef.current?.scrollTo({ top: 0, behavior: "smooth" });
    }
  }, []);

  const showOutline = wide && !outlineHidden && headings.length > 0;

  return (
    <div
      ref={wrapRef}
      className={cn(
        "relative flex h-full w-full overflow-hidden bg-background",
        !visible && "pointer-events-none",
      )}
    >
      {showOutline && (
        <aside
          role="navigation"
          aria-label="Document outline"
          className="order-2 w-[12.5rem] shrink-0 overflow-hidden border-s border-border bg-popover/30 px-2.5 py-3"
        >
          <OnThisPage
            headings={headings}
            activeHeadingId={activeHeadingId}
            onNavigate={navigateTo}
            onHide={() => setOutlineHidden(true)}
          />
        </aside>
      )}
      <div
        className="order-1 min-w-0 flex-1 overflow-auto scroll-smooth px-8 py-6 [scroll-padding-top:3rem]"
        ref={scrollRef}
        onScroll={onScroll}
      >
        {(wide && outlineHidden && headings.length > 0) || onSetView ? (
          <div className="sticky top-0 z-10 float-end -me-4 -mt-2 flex items-center gap-1">
            {wide && outlineHidden && headings.length > 0 && (
              <button
                type="button"
                onClick={() => setOutlineHidden(false)}
                className="rounded border border-border bg-popover/90 px-1.5 py-0.5 text-[11px] text-muted-foreground hover:text-foreground"
                title="Show outline"
              >
                Outline
              </button>
            )}
            {onSetView && (
              <button
                type="button"
                onClick={() => onSetView("raw")}
                className="rounded border border-border bg-popover/90 px-1.5 py-0.5 text-[11px] text-muted-foreground hover:text-foreground"
                title="Switch to raw markdown"
              >
                Raw
              </button>
            )}
          </div>
        ) : null}
        {status.kind === "loading" && (
          <p className="text-[12px] text-muted-foreground">Loading…</p>
        )}
        {status.kind === "error" && (
          <p className="text-[12px] text-destructive">
            Failed to read file: {status.message}
          </p>
        )}
        {status.kind === "binary" && (
          <p className="text-[12px] text-muted-foreground">
            Binary file — cannot render as markdown.
          </p>
        )}
        {status.kind === "toolarge" && (
          <p className="text-[12px] text-muted-foreground">
            File is {status.size} bytes; limit {status.limit}.
          </p>
        )}
        {content !== null && (
          <MarkdownBody
            path={path}
            boundary={previewBoundary(path, workspaceRoot)}
            content={content}
          />
        )}
      </div>
    </div>
  );
}

const MarkdownBody = memo(function MarkdownBody({
  path,
  boundary,
  content,
}: {
  path: string;
  boundary: string;
  content: string;
}) {
  const { urlFor: assetUrlFor, error: assetError, retry } = useAssetGrant(boundary);
  const rehypePlugins = useMemo(() => {
    const d = defaultRehypePlugins;
    return [
      d.raw,
      d.sanitize,
      rehypeLocalImages(path, (abs) => assetUrlFor(abs) ?? "data:,", {
        boundary,
        isReadable: (abs) => checkReadable(abs).ok,
      }),
      d.harden,
    ];
  }, [path, boundary, assetUrlFor]);

  return (
    <>
      {assetError && (
        <div className="mb-3 text-xs text-muted-foreground">
          <p role="alert">Local images unavailable: {assetError}</p>
          <button type="button" className="underline" onClick={retry}>
            Retry images
          </button>
        </div>
      )}
      <Streamdown
        className="select-text [&>*:first-child]:mt-0 [&>*:last-child]:mb-0"
        components={components}
        rehypePlugins={rehypePlugins}
        mode="static"
        parseIncompleteMarkdown={false}
      >
        {content}
      </Streamdown>
    </>
  );
});
