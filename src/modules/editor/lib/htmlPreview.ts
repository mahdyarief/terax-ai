// Debounced, single-use publishing for the HTML preview frame.

export type HtmlPreviewPublisherOptions = {
  publish: (html: string) => Promise<string>;
  drop: (token: string) => Promise<void>;
  onReady: (token: string) => void;
  onError: (message: string) => void;
  delayMs?: number;
};

export type HtmlPreviewPublisher = {
  request: (html: string) => void;
  dispose: () => void;
};

// Edits arrive on every keystroke; republishing each one would thrash the
// preview frame.
const DEFAULT_DELAY_MS = 250;

/**
 * Coalesces edits into one publish, and drops any token that arrives after a
 * newer edit superseded it. The frame's token is single-use, so every publish
 * yields a fresh URL.
 */
export function createHtmlPreviewPublisher({
  publish,
  drop,
  onReady,
  onError,
  delayMs = DEFAULT_DELAY_MS,
}: HtmlPreviewPublisherOptions): HtmlPreviewPublisher {
  let timer: ReturnType<typeof setTimeout> | null = null;
  let generation = 0;
  let live: string | null = null;
  let disposed = false;

  const run = async (html: string, current: number) => {
    try {
      const token = await publish(html);
      if (disposed || generation !== current) {
        void drop(token);
        return;
      }
      const previous = live;
      live = token;
      onReady(token);
      if (previous) void drop(previous);
    } catch (cause) {
      if (!disposed && generation === current) {
        onError(cause instanceof Error ? cause.message : String(cause));
      }
    }
  };

  return {
    request(html) {
      if (disposed) return;
      if (timer !== null) clearTimeout(timer);
      const current = ++generation;
      timer = setTimeout(() => {
        timer = null;
        void run(html, current);
      }, delayMs);
    },
    dispose() {
      disposed = true;
      generation += 1;
      if (timer !== null) clearTimeout(timer);
      timer = null;
      const token = live;
      live = null;
      if (token) void drop(token);
    },
  };
}

export type PreviewScroll = { x: number; y: number };

const SCROLL_MESSAGE = "terax-preview-scroll";

/**
 * Every edit loads a fresh frame, which would jump back to the top. This adds
 * a small script that reports the frame's scroll position to the parent and
 * restores `scroll` once the new document has laid out. It only runs when the
 * preview is served with scripts on.
 */
export function withScrollBridge(html: string, scroll: PreviewScroll): string {
  const x = Math.max(0, Math.round(scroll.x)) || 0;
  const y = Math.max(0, Math.round(scroll.y)) || 0;
  const script =
    `<script data-terax-preview>(()=>{let u=0,t=0;` +
    `const r=()=>{if(!u&&(${x}||${y}))scrollTo(${x},${y})};` +
    `for(const e of["wheel","pointerdown","keydown","touchstart"])` +
    `addEventListener(e,()=>{u=1},{passive:true,capture:true});` +
    `addEventListener("DOMContentLoaded",r);addEventListener("load",r);` +
    `addEventListener("scroll",()=>{if(t)return;t=setTimeout(()=>{t=0;` +
    `parent.postMessage({type:"${SCROLL_MESSAGE}",x:scrollX,y:scrollY},"*")},100)},` +
    `{passive:true})})()</script>`;
  const close = html.toLowerCase().lastIndexOf("</body>");
  return close < 0
    ? html + script
    : html.slice(0, close) + script + html.slice(close);
}

/** The scroll position a preview frame reported, or null for anything else. */
export function readScrollMessage(data: unknown): PreviewScroll | null {
  if (typeof data !== "object" || data === null) return null;
  const { type, x, y } = data as Record<string, unknown>;
  if (type !== SCROLL_MESSAGE) return null;
  if (typeof x !== "number" || typeof y !== "number") return null;
  if (!Number.isFinite(x) || !Number.isFinite(y)) return null;
  return { x: Math.max(0, x), y: Math.max(0, y) };
}

/** Does this markup carry anything that would execute? (UI hint only.) */
export function hasActiveContent(html: string): boolean {
  return /<script\b/i.test(html) || /\son[a-z]+\s*=/i.test(html);
}
