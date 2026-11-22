import {
  MarkdownCode,
  markdownCodeText,
} from "@/components/ai-elements/markdown-code";
import { OnThisPage } from "@/components/markdown/OnThisPage";
import {
  parseHeadings,
  type TocHeading,
} from "@/components/markdown/useMarkdownHeadings";
import { openExternalUrl } from "@/lib/external-link";
import { useAssetGrant } from "@/lib/useAssetGrant";
import { cn } from "@/lib/utils";
import { checkReadable } from "@/modules/ai/lib/security";
import {
  type ComponentProps,
  createContext,
  forwardRef,
  Fragment,
  memo,
  type ReactNode,
  useCallback,
  useContext,
  useEffect,
  useImperativeHandle,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import {
  defaultRehypePlugins,
  defaultRemarkPlugins,
  Streamdown,
  type StreamdownProps,
} from "streamdown";
import {
  previewBoundary,
  rehypeAllowBareRelativeLinks,
  rehypeLocalImages,
  rehypeRestoreDocHrefs,
} from "./lib/assetPath";
import { rehypeHeadingAnchors } from "./lib/headingAnchors";
import { MARKDOWN_FIND_EVENT } from "./lib/findEvent";
import { type Frontmatter, splitFrontmatter } from "./lib/frontmatter";
import {
  classifyLink,
  headingForFragment,
  rehypeKeepHref,
} from "./lib/links";
import {
  escapeLooseDollars,
  type MathSyntax,
  mathSyntaxOf,
  rehypeMarkMath,
} from "./lib/math";
import { readOutlineHidden, writeOutlineHidden } from "./lib/outlinePref";
import {
  type Anchor,
  buildAnchors,
  lineForOffset,
  offsetForLine,
} from "./lib/sourceMap";
import { stampHeadingIds } from "./lib/stampHeadingIds";
import { MarkdownFindBar } from "./MarkdownFindBar";
import { MathSpan } from "./MathSpan";
import { MermaidBlock } from "./MermaidBlock";
import {
  type MarkdownStatus,
  useMarkdownDocument,
} from "./useMarkdownDocument";

export function buildPreviewRehypePlugins({
  path,
  boundary,
  assetUrlFor,
}: {
  path: string;
  boundary: string;
  assetUrlFor: (abs: string) => string | undefined;
}) {
  const d = defaultRehypePlugins;
  const sanitize = d.sanitize;
  const sanitizeForPreview = Array.isArray(sanitize)
    ? ([
        sanitize[0],
        {
          ...sanitize[1],
          protocols: {
            ...sanitize[1]?.protocols,
            src: [
              ...(sanitize[1]?.protocols?.src ?? ["http", "https"]),
              "data",
            ],
          },
        },
      ] satisfies typeof sanitize)
    : sanitize;
  const harden = d.harden;
  const hardenForPreview = Array.isArray(harden)
    ? ([
        harden[0],
        {
          ...harden[1],
          defaultOrigin: `https://markdown-preview.invalid${path.startsWith("/") ? path : `/${path}`}`,
          allowDefaultOrigin: true,
          allowedLinkPrefixes: [...(harden[1]?.allowedLinkPrefixes ?? []), "#", "./", "../"],
          allowedImagePrefixes: [
            ...(harden[1]?.allowedImagePrefixes ?? []),
            "asset://",
            "./",
            "../",
          ],
          allowDataImages: true,
        },
      ] satisfies typeof harden)
    : harden;
  return [
    d.raw,
    sanitizeForPreview,
    rehypeHeadingAnchors,
    rehypeLocalImages(path, (abs) => assetUrlFor(abs) ?? "data:,", {
      boundary,
      isReadable: (abs) => checkReadable(abs).ok,
    }),
    rehypeMarkMath,
    rehypeAllowBareRelativeLinks(path),
    rehypeKeepHref(),
    hardenForPreview,
    rehypeRestoreDocHrefs(),
  ] satisfies StreamdownProps["rehypePlugins"];
}

export type MarkdownViewHandle = {
  /** Put this 1-based source line at the top of the preview. */
  scrollToSourceLine: (line: number) => void;
  /** The source line currently at the top of the preview. */
  sourceLineAtTop: () => number;
  openFind: () => void;
};

type ViewProps = {
  path: string;
  status: MarkdownStatus;
  /** Images load only from beneath this folder (the document's own folder if it lies outside). */
  workspaceRoot?: string | null;
  visible: boolean;
  /**
   * Switch to the source editor, at the line being read. Omitted when the
   * preview has no editor of its own to switch to.
   */
  onSetView?: (mode: "raw" | "split", line: number) => void;
  /** Open another file that a relative link points at. */
  onOpenFile?: (path: string, pin?: boolean, fragment?: string) => void;
  scrollToFragment?: string | null;
  onFragmentConsumed?: () => void;
  /** Reports the source line at the top as the reader scrolls. */
  onSourceLineChange?: (line: number) => void;
  /** Keep the top-right corner clear for controls drawn over the pane. */
  reserveCorner?: boolean;
};

type Props = Omit<ViewProps, "status">;

/** Below this pane width the outline would squeeze the text, so it hides. */
const OUTLINE_MIN_WIDTH = 480;
/** A heading within this many px of the top edge counts as the one being read. */
const ACTIVE_HEADING_OFFSET = 48;

/** The preview the reader last touched; it answers the find shortcut. */
let lastTouched: HTMLElement | null = null;

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

function samePath(a: string, b: string): boolean {
  const normalize = (path: string) => path.replace(/\\/g, "/").toLowerCase();
  return normalize(a) === normalize(b);
}

/** Handles a click on a link in the rendered document. */
const LinkContext = createContext<(href: string) => void>(() => {});

type LinkProps = ComponentProps<"a"> & {
  node?: unknown;
  /** The address as written, before hardening rewrote it. */
  "data-href"?: string;
};

function PreviewLink({ children, href, node: _node, ...props }: LinkProps) {
  const follow = useContext(LinkContext);
  const written = props["data-href"] ?? href;
  return (
    <a
      {...props}
      href={href}
      rel="noreferrer"
      // Every link is handled here: left alone, a relative one would navigate
      // the app's own webview away.
      onClick={(event) => {
        event.preventDefault();
        if (written) follow(written);
      }}
      onAuxClick={(event) => event.preventDefault()}
    >
      {children}
    </a>
  );
}

type CodeProps = {
  className?: string;
  children?: ReactNode;
  "data-math"?: string;
};

function PreviewCode(props: CodeProps) {
  const math = props["data-math"];
  if (math) {
    return (
      <MathSpan
        tex={markdownCodeText(props.children).trim()}
        display={math === "display"}
      />
    );
  }
  if (/\blanguage-mermaid\b/.test(props.className ?? "")) {
    return (
      <MermaidBlock code={markdownCodeText(props.children).replace(/\n$/, "")} />
    );
  }
  return <MarkdownCode {...props} />;
}

const components = { a: PreviewLink, code: PreviewCode };

/** Reads a markdown file from disk, keeps it current, and renders it. */
export function MarkdownPreviewPane(props: Props) {
  const status = useMarkdownDocument(props.path, props.visible);
  return <MarkdownDocumentView {...props} status={status} />;
}

/** Renders markdown that the caller already holds, e.g. an editor buffer. */
export const MarkdownDocumentView = forwardRef<MarkdownViewHandle, ViewProps>(
  function MarkdownDocumentView(
    {
      path,
      status,
      workspaceRoot = null,
      visible,
      onSetView,
      onOpenFile,
      scrollToFragment: fragmentToScroll,
      onFragmentConsumed,
      onSourceLineChange,
      reserveCorner = false,
    },
    ref,
  ) {
    const scrollRef = useRef<HTMLDivElement>(null);
    const wrapRef = useRef<HTMLDivElement>(null);
    const bodyRef = useRef<HTMLDivElement>(null);
    const scrollTopRef = useRef(0);
    const frameRef = useRef(0);
    const pendingLineRef = useRef<number | null>(null);
    const content = status.kind === "ready" ? status.content : null;
    const { frontmatter, body, lineOffset } = useMemo(
      () => splitFrontmatter(content ?? ""),
      [content],
    );
    const headings = useMemo(() => parseHeadings(body), [body]);
    const totalLines = useMemo(
      () => (content ?? "").split("\n").length,
      [content],
    );
    const [activeHeadingId, setActiveHeadingId] = useState<
      string | undefined
    >();
    const [outlineHidden, setHidden] = useState(readOutlineHidden);
    const [wide, setWide] = useState(true);
    const [finding, setFinding] = useState(false);

    const setOutlineHidden = (hidden: boolean) => {
      setHidden(hidden);
      writeOutlineHidden(hidden);
    };

    const headingsRef = useRef(headings);
    headingsRef.current = headings;
    const layoutRef = useRef({ lineOffset, totalLines });
    layoutRef.current = { lineOffset, totalLines };
    const onSourceLineChangeRef = useRef(onSourceLineChange);
    onSourceLineChangeRef.current = onSourceLineChange;

    /** Where each rendered heading sits, paired with its source line. */
    const anchors = useCallback((): Anchor[] => {
      const el = scrollRef.current;
      if (!el) return [];
      const byId = new Map(headingsRef.current.map((h) => [h.id, h]));
      const top = el.getBoundingClientRect().top - el.scrollTop;
      const found: Anchor[] = [];
      for (const node of el.querySelectorAll<HTMLElement>(
        "[data-heading-id]",
      )) {
        const heading = byId.get(node.dataset.headingId ?? "");
        if (!heading) continue;
        found.push({
          line: heading.line + layoutRef.current.lineOffset,
          offset: node.getBoundingClientRect().top - top,
        });
      }
      return buildAnchors(found, layoutRef.current.totalLines, el.scrollHeight);
    }, []);

    const sourceLineAtTop = useCallback(() => {
      const el = scrollRef.current;
      return el ? lineForOffset(anchors(), el.scrollTop) : 1;
    }, [anchors]);

    const applyPendingLine = useCallback(() => {
      const el = scrollRef.current;
      const line = pendingLineRef.current;
      if (!el || line === null) return;
      pendingLineRef.current = null;
      const top = offsetForLine(anchors(), line);
      scrollTopRef.current = top;
      el.scrollTop = top;
    }, [anchors]);

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
        onSourceLineChangeRef.current?.(sourceLineAtTop());
      });
    }, [trackActiveHeading, sourceLineAtTop]);

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
          applyPendingLine();
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
    }, [content, headings, trackActiveHeading, applyPendingLine]);

    useImperativeHandle(
      ref,
      () => ({
        scrollToSourceLine: (line) => {
          pendingLineRef.current = line;
          // Before the document has rendered, the stamping pass applies it.
          if (bodyRef.current?.childElementCount) {
            applyPendingLine();
          }
        },
        sourceLineAtTop,
        openFind: () => setFinding(true),
      }),
      [applyPendingLine, sourceLineAtTop],
    );

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

    const scrollToFragment = useCallback(
      (fragment: string) => {
        if (!fragment) {
          scrollRef.current?.scrollTo({ top: 0, behavior: "smooth" });
          return;
        }
        const heading = headingForFragment(headingsRef.current, fragment);
        if (heading) {
          navigateTo(heading);
          return;
        }
        // Not a heading: an explicit id or a named anchor in raw HTML.
        const id = CSS.escape(fragment);
        scrollRef.current
          ?.querySelector<HTMLElement>(
            `[id="${id}"], [id="user-content-${id}"], a[name="${id}"]`,
          )
          ?.scrollIntoView({ behavior: "smooth", block: "start" });
      },
      [navigateTo],
    );

    const onOpenFileRef = useRef(onOpenFile);
    onOpenFileRef.current = onOpenFile;
    const followLink = useCallback(
      (href: string) => {
        const target = classifyLink(href, path);
        if (target.kind === "external") void openExternalUrl(href);
        else if (target.kind === "anchor") scrollToFragment(target.fragment ?? "");
        else if (target.kind === "file") {
          if (samePath(target.path, path)) {
            scrollToFragment(target.fragment ?? "");
          } else
            onOpenFileRef.current?.(
              target.path,
              true,
              target.fragment ?? undefined,
            );
        }
      },
      [path, scrollToFragment],
    );

    useEffect(() => {
      if (!visible || fragmentToScroll === undefined || content === null) return;
      const frame = window.requestAnimationFrame(() => {
        scrollToFragment(fragmentToScroll ?? "");
        onFragmentConsumed?.();
      });
      return () => window.cancelAnimationFrame(frame);
    }, [visible, fragmentToScroll, content, scrollToFragment, onFragmentConsumed]);

    // Find: the app's search shortcut, or Ctrl/Cmd+F while this preview is the
    // one in use.
    useEffect(() => {
      if (!visible) return;
      const open = () => setFinding(true);
      const onKeyDown = (event: KeyboardEvent) => {
        if (event.key.toLowerCase() !== "f") return;
        if (!(event.ctrlKey || event.metaKey)) return;
        if (event.shiftKey || event.altKey) return;
        const wrap = wrapRef.current;
        const focused = document.activeElement;
        const inside = !!wrap && !!focused && wrap.contains(focused);
        const idle = !focused || focused === document.body;
        if (!inside && !(idle && (lastTouched === wrap || !lastTouched))) {
          return;
        }
        event.preventDefault();
        open();
      };
      window.addEventListener(MARKDOWN_FIND_EVENT, open);
      window.addEventListener("keydown", onKeyDown);
      return () => {
        window.removeEventListener(MARKDOWN_FIND_EVENT, open);
        window.removeEventListener("keydown", onKeyDown);
      };
    }, [visible]);

    useEffect(
      () => () => {
        if (lastTouched === wrapRef.current) lastTouched = null;
      },
      [],
    );

    const showOutline = wide && !outlineHidden && headings.length > 0;

    return (
      <LinkContext.Provider value={followLink}>
        <div
          ref={wrapRef}
          onPointerDownCapture={() => {
            lastTouched = wrapRef.current;
          }}
          className={cn(
            "relative flex h-full w-full overflow-hidden bg-background",
            !visible && "pointer-events-none",
          )}
        >
          {finding && (
            <MarkdownFindBar
              root={bodyRef}
              revision={content}
              onClose={() => setFinding(false)}
            />
          )}
          {showOutline && (
            <aside
              role="navigation"
              aria-label="Document outline"
              className={cn(
                "order-2 w-[12.5rem] shrink-0 overflow-hidden border-s border-border bg-popover/30 px-2.5 py-3",
                reserveCorner && "pt-11",
              )}
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
            className={cn(
              "order-1 min-w-0 flex-1 overflow-auto px-8 py-6 [scroll-padding-top:3rem]",
              reserveCorner && !showOutline && "pt-11",
            )}
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
                  <>
                    <button
                      type="button"
                      onClick={() => onSetView("split", sourceLineAtTop())}
                      className="rounded border border-border bg-popover/90 px-1.5 py-0.5 text-[11px] text-muted-foreground hover:text-foreground"
                      title="Edit with the preview alongside"
                    >
                      Split
                    </button>
                    <button
                      type="button"
                      onClick={() => onSetView("raw", sourceLineAtTop())}
                      className="rounded border border-border bg-popover/90 px-1.5 py-0.5 text-[11px] text-muted-foreground hover:text-foreground"
                      title="Switch to raw markdown"
                    >
                      Raw
                    </button>
                  </>
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
                This file is {formatBytes(status.size)}, which is over the{" "}
                {formatBytes(status.limit)} preview limit.
              </p>
            )}
            {content !== null && (
              <div ref={bodyRef}>
                {frontmatter && <FrontmatterBlock frontmatter={frontmatter} />}
                <MarkdownBody
                  path={path}
                  boundary={previewBoundary(path, workspaceRoot)}
                  content={body}
                />
              </div>
            )}
          </div>
        </div>
      </LinkContext.Provider>
    );
  },
);

/** Frontmatter is metadata, so it is shown as a small table above the prose. */
function FrontmatterBlock({ frontmatter }: { frontmatter: Frontmatter }) {
  return (
    <section
      aria-label="Frontmatter"
      className="mb-5 select-text rounded-md border border-border/60 bg-muted/30 px-3 py-2 text-[12px]"
    >
      {frontmatter.fields ? (
        <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-0.5">
          {frontmatter.fields.map(([key, value]) => (
            <Fragment key={key}>
              <dt className="font-mono text-muted-foreground">{key}</dt>
              <dd className="min-w-0 break-words">{value}</dd>
            </Fragment>
          ))}
        </dl>
      ) : (
        <pre className="overflow-x-auto font-mono text-[11.5px] text-muted-foreground">
          {frontmatter.raw}
        </pre>
      )}
    </section>
  );
}

type RemarkMath = typeof import("remark-math").default;

/** Loads the math parser the first time a document needs it. */
function useRemarkMath(needed: boolean): RemarkMath | null {
  const [plugin, setPlugin] = useState<RemarkMath | null>(null);
  useEffect(() => {
    if (!needed || plugin) return;
    let alive = true;
    void import("remark-math").then((module) => {
      // A function as state must be wrapped, or React would call it.
      if (alive) setPlugin(() => module.default);
    });
    return () => {
      alive = false;
    };
  }, [needed, plugin]);
  return plugin;
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
  const math: MathSyntax | null = useMemo(() => mathSyntaxOf(content), [content]);
  const remarkMath = useRemarkMath(math !== null);
  const singleDollar = math?.singleDollar ?? false;
  // With `$x$` enabled, dollar amounts would be typeset too; escape those.
  const source = useMemo(
    () => (remarkMath && singleDollar ? escapeLooseDollars(content) : content),
    [content, remarkMath, singleDollar],
  );

  const rehypePlugins = useMemo(
    () => buildPreviewRehypePlugins({ path, boundary, assetUrlFor }),
    [path, boundary, assetUrlFor],
  );

  const remarkPlugins = useMemo(() => {
    const defaults = Object.values(defaultRemarkPlugins);
    return remarkMath
      ? [...defaults, [remarkMath, { singleDollarTextMath: singleDollar }]]
      : defaults;
  }, [remarkMath, singleDollar]);

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
        // Streamdown's memo ignores plugin changes, so remount when the math
        // parser arrives or its dollar syntax changes.
        key={remarkMath ? (singleDollar ? "math-single" : "math") : "plain"}
        className="select-text [&>*:first-child]:mt-0 [&>*:last-child]:mb-0"
        components={components}
        rehypePlugins={rehypePlugins}
        remarkPlugins={remarkPlugins as never}
        mode="static"
        parseIncompleteMarkdown={false}
      >
        {source}
      </Streamdown>
    </>
  );
});
