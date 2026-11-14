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