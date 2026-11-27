import { NO_TEXT_CONTRAST } from "@/modules/terminal/ghostty/gpu/textContrast";
import {
  measureTerminalFont,
  type TerminalFontMetrics,
} from "@/modules/terminal/ghostty/gpu/terminalVisuals";
import type { AdaptedGhosttyTerminalModel } from "@/modules/terminal/ghostty/AdaptedGhosttyTerminalModel";
import {
  WebGlCellRenderer,
  type WebGlRendererProfile,
} from "@/modules/terminal/ghostty/webgl/WebGlCellRenderer";
import { useEffect, useRef, useState } from "react";
import { RemoteTerminalFeed } from "./feed";
import {
  WEB_TERMINAL_FONT,
  createRemoteModel,
  fallbackTheme,
  gpuThemeFromRemote,
  loadGhosttyCore,
} from "./model";

type Status = "loading" | "live" | "reconnecting" | "closed" | "error";

/**
 * One terminal as the browser sees it: the PTY's replay log in, Ghostty's WASM
 * core and the WebGL renderer out. The view is scaled to fit and never resizes
 * the desktop's terminal.
 */
export function RemoteTerminal({ ptyId }: { ptyId: number }) {
  const viewportRef = useRef<HTMLDivElement | null>(null);
  const hostRef = useRef<HTMLDivElement | null>(null);
  const [status, setStatus] = useState<Status>("loading");
  const [message, setMessage] = useState<string | null>(null);
  const [natural, setNatural] = useState({ width: 0, height: 0 });
  const [viewportWidth, setViewportWidth] = useState(0);
  const [zoom, setZoom] = useState(1);

  useEffect(() => {
    const viewport = viewportRef.current;
    if (!viewport) return;
    const observer = new ResizeObserver(() => {
      setViewportWidth(viewport.clientWidth);
    });
    observer.observe(viewport);
    setViewportWidth(viewport.clientWidth);
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;
    let disposed = false;
    let source: EventSource | null = null;
    let renderer: WebGlCellRenderer | null = null;
    let model: AdaptedGhosttyTerminalModel | null = null;
    let metrics: TerminalFontMetrics | null = null;
    let theme = fallbackTheme();
    let frame = 0;

    const paint = () => {
      frame = 0;
      if (!renderer || !model) return;
      renderer.resize(model.cols, model.rows);
      renderer.render({
        model,
        damage: model.consumeDamage(),
        cursorVisible: true,
        textBlinkVisible: true,
        selection: null,
        searchMatchAt: () => 0,
      });
      const width =
        Number.parseFloat(renderer.canvas.style.width) || renderer.canvas.width;
      const height =
        Number.parseFloat(renderer.canvas.style.height) ||
        renderer.canvas.height;
      setNatural((previous) =>
        previous.width === width && previous.height === height
          ? previous
          : { width, height },
      );
    };
    const schedule = () => {
      if (!frame) frame = requestAnimationFrame(paint);
    };

    const profile = (): WebGlRendererProfile | null =>
      metrics
        ? { metrics, theme, scale: metrics.scale, contrast: NO_TEXT_CONTRAST }
        : null;
    const failed = (error: Error) => {
      setStatus("error");
      setMessage(error.message);
    };

    void (async () => {
      try {
        const core = await loadGhosttyCore();
        if (disposed) return;
        model = createRemoteModel(core, 80, 24);
        metrics = await measureTerminalFont(WEB_TERMINAL_FONT);
        if (disposed) return;
        const initial = profile();
        if (!initial) return;
        renderer = new WebGlCellRenderer();
        renderer.configure(initial, schedule, failed);
        renderer.attach(host);
        model.subscribeDamage(schedule);

        const feed = new RemoteTerminalFeed(model);
        const events = new EventSource(`/api/pty/${ptyId}/stream`);
        source = events;

        const applyTheme = () => {
          const next = feed.theme
            ? gpuThemeFromRemote(feed.theme, theme)
            : theme;
          const changed =
            next.background.join() !== theme.background.join() ||
            next.foreground.join() !== theme.foreground.join() ||
            next.cursor.join() !== theme.cursor.join();
          theme = next;
          const nextProfile = profile();
          if (changed && nextProfile) {
            renderer?.configure(nextProfile, schedule, failed);
            schedule();
          }
        };

        events.addEventListener("open", () => setStatus("live"));
        events.addEventListener("meta", (event) => {
          feed.meta(JSON.parse((event as MessageEvent<string>).data));
          applyTheme();
          setStatus("live");
          schedule();
        });
        events.addEventListener("snapshot", (event) => {
          const message = event as MessageEvent<string>;
          feed.snapshot(message.lastEventId, message.data);
          schedule();
        });
        events.addEventListener("data", (event) => {
          const message = event as MessageEvent<string>;
          feed.data(message.lastEventId, message.data);
          schedule();
        });
        events.addEventListener("resize", (event) => {
          feed.resize(JSON.parse((event as MessageEvent<string>).data));
          schedule();
        });
        events.addEventListener("exit", (event) => {
          feed.exit(JSON.parse((event as MessageEvent<string>).data));
          setStatus("closed");
          events.close();
          schedule();
        });
        events.addEventListener("error", () => {
          setStatus((previous) =>
            previous === "closed" ? previous : "reconnecting",
          );
        });
        return;
      } catch (error) {
        if (disposed) return;
        setStatus("error");
        setMessage(error instanceof Error ? error.message : String(error));
      }
    })();

    return () => {
      disposed = true;
      source?.close();
      if (frame) cancelAnimationFrame(frame);
      model?.dispose();
      renderer?.dispose();
    };
  }, [ptyId]);

  const fit = viewportWidth && natural.width
    ? Math.min(1, viewportWidth / natural.width) * zoom
    : zoom;

  return (
    <div className="relative flex h-full min-h-0 w-full flex-col">
      <div
        ref={viewportRef}
        className="min-h-0 flex-1 overflow-auto bg-black"
      >
        <div
          style={{
            width: natural.width ? natural.width * fit : undefined,
            height: natural.height ? natural.height * fit : undefined,
          }}
        >
          <div
            ref={hostRef}
            style={{
              width: natural.width || undefined,
              height: natural.height || undefined,
              transform: `scale(${fit})`,
              transformOrigin: "top left",
            }}
          />
        </div>
      </div>

      {status !== "live" ? (
        <div className="pointer-events-none absolute inset-x-0 top-0 flex justify-center p-2">
          <span className="rounded-full bg-black/70 px-2 py-1 text-xs text-white">
            {status === "loading"
              ? "Opening terminal…"
              : status === "reconnecting"
                ? "Reconnecting…"
                : status === "closed"
                  ? "This terminal has exited."
                  : (message ?? "The terminal could not be shown.")}
          </span>
        </div>
      ) : null}

      <div className="flex items-center justify-end gap-1 border-t border-border bg-card px-2 py-1 text-xs">
        <button
          type="button"
          onClick={() => setZoom((value) => Math.max(0.25, value - 0.1))}
          className="rounded px-2 py-1 hover:bg-accent"
          aria-label="Zoom out"
        >
          −
        </button>
        <button
          type="button"
          onClick={() => setZoom(1)}
          className="rounded px-2 py-1 tabular-nums hover:bg-accent"
        >
          {Math.round(zoom * 100)}%
        </button>
        <button
          type="button"
          onClick={() => setZoom((value) => Math.min(4, value + 0.1))}
          className="rounded px-2 py-1 hover:bg-accent"
          aria-label="Zoom in"
        >
          +
        </button>
      </div>
    </div>
  );
}
