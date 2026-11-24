import { CopyButton } from "@/components/ui/copy-button";
import { usePreferencesStore } from "@/modules/settings/preferences";
import type { TerminalSearchController } from "@/modules/terminal/search/TerminalSearchController";
import { useTheme } from "@/modules/theme";
import { ArrowDown01Icon } from "@hugeicons/core-free-icons";
import { HugeiconsIcon } from "@hugeicons/react";
import {
  forwardRef,
  lazy,
  memo,
  Suspense,
  useEffect,
  useImperativeHandle,
  useRef,
  useState,
} from "react";
import type { TerminalBackendKind } from "./backend/contracts";
import { terminalBackendFor } from "./backend/selection";
import { ghosttyBlocks } from "./ghostty/ghosttyBlockSessions";
import {
  ghosttyBlockGeometry,
  useGhosttyTerminalSession,
} from "./ghostty/useGhosttyTerminalSession";

export type TerminalPaneHandle = {
  write: (data: string) => void;
  /** Like `write`, but only when the shell is at its prompt; false if not. */
  writeAtPrompt: (data: string) => Promise<boolean>;
  focus: () => void;
  getBuffer: (maxLines?: number) => string | null;
  getSelection: () => string | null;
};

export type TerminalPaneProps = {
  /** Stable identifier for this leaf (passed back through callbacks). */
  leafId: number;
  /** Tab containing this pane is on screen. */
  visible: boolean;
  /** This leaf is the active pane within its tab and receives auto-focus. */
  focused?: boolean;
  initialCwd?: string;
  /** PTY id carried by a handed-off leaf; enables replay adoption. */
  ptyId?: number;
  /** Enable command-block decorations (OSC 133) for this terminal. */
  blocks?: boolean;
  /** Shell to start; undefined follows Settings > Integrated terminal shell. */
  shell?: string;
  onSearchReady?: (leafId: number, addon: TerminalSearchController) => void;
  onExit?: (leafId: number, code: number) => void;
  onCwd?: (leafId: number, cwd: string) => void;
};

const TerminalAccessibleOutput = lazy(
  () => import("./ghostty/TerminalAccessibleOutput"),
);
const GhosttyBlockOverlay = lazy(() => import("./ghostty/GhosttyBlockOverlay"));

const GhosttyTerminalPane = memo(
  forwardRef<
    TerminalPaneHandle,
    TerminalPaneProps & {
      backend: Extract<TerminalBackendKind, `ghostty-${string}`>;
    }
  >(function GhosttyTerminalPane(
    {
      leafId,
      visible,
      focused = true,
      initialCwd,
      ptyId,
      blocks = false,
      shell,
      onSearchReady,
      onExit,
      onCwd,
      backend,
    },
    ref,
  ) {
    const screenReader = usePreferencesStore(
      (state) => state.terminalScreenReader,
    );
    const containerRef = useRef<HTMLDivElement>(null);
    const down = useRef<{ x: number; y: number } | null>(null);
    const { resolvedMode, activeTheme } = useTheme();
    const [scrolledUp, setScrolledUp] = useState(false);
    const session = useGhosttyTerminalSession({
      leafId,
      backend,
      container: containerRef,
      visible,
      focused,
      initialCwd,
      ptyId,
      blocks,
      shell,
      onSearchReady: (search) => onSearchReady?.(leafId, search),
      onExit: (code) => onExit?.(leafId, code),
      onCwd: (cwd) => onCwd?.(leafId, cwd),
    });

    useEffect(() => {
      const model = session.model;
      if (!model) {
        setScrolledUp(false);
        return;
      }
      const update = () =>
        setScrolledUp(
          model.scrollPosition().offset > 0 && !model.modes().alternateScreen,
        );
      update();
      return model.subscribeDamage(update);
    }, [session.model]);

    useEffect(() => {
      void resolvedMode;
      void activeTheme;
      const id = requestAnimationFrame(() => session.applyTheme());
      return () => cancelAnimationFrame(id);
    }, [resolvedMode, activeTheme, session]);

    useImperativeHandle(
      ref,
      () => ({
        write: session.write,
        writeAtPrompt: session.writeAtPrompt,
        focus: session.focus,
        getBuffer: session.getBuffer,
        getSelection: session.getSelection,
      }),
      [session],
    );

    return (
      <div
        className="zoom-exempt relative h-full w-full overflow-hidden"
        style={{
          visibility: visible ? "visible" : "hidden",
          pointerEvents: visible ? "auto" : "none",
        }}
        onPointerDownCapture={
          blocks
            ? (event) => {
                down.current =
                  event.button === 0 &&
                  event.detail === 1 &&
                  !event.shiftKey &&
                  !event.altKey &&
                  !event.metaKey &&
                  !event.ctrlKey &&
                  containerRef.current?.contains(event.target as Node)
                    ? { x: event.clientX, y: event.clientY }
                    : null;
              }
            : undefined
        }
        onPointerUp={
          blocks
            ? (event) => {
                const origin = down.current;
                down.current = null;
                const state = ghosttyBlocks(leafId);
                const geometry = ghosttyBlockGeometry(leafId);
                if (
                  origin &&
                  geometry &&
                  geometry.height > 0 &&
                  Math.hypot(
                    event.clientX - origin.x,
                    event.clientY - origin.y,
                  ) <= 4 &&
                  !state?.model?.trackedSelection?.()
                ) {
                  const row = Math.floor(
                    (event.clientY - geometry.top) / geometry.height,
                  );
                  if (state?.model && row >= 0 && row < state.model.rows)
                    state.controller?.selectAtLine(
                      state.model.bufferLineAtViewportRow(row),
                    );
                }
                if (state?.getMode() === "prompt") state.focus?.();
              }
            : undefined
        }
        data-terminal-backend={backend}
      >
        <div ref={containerRef} className="absolute inset-0" />
        {screenReader && session.model && (
          <Suspense fallback={null}>
            <TerminalAccessibleOutput
              model={session.model}
              visible={visible}
              focused={focused}
              onExit={session.focus}
            />
          </Suspense>
        )}
        {blocks && (
          <Suspense fallback={null}>
            <GhosttyBlockOverlay leafId={leafId} />
          </Suspense>
        )}
        {visible && session.model && scrolledUp && !session.error && (
          <button
            type="button"
            aria-label="Scroll to bottom"
            title="Scroll to bottom"
            className="absolute bottom-3 right-[18px] z-10 flex size-8 items-center justify-center rounded-full border bg-background/90 text-muted-foreground shadow-sm hover:bg-accent hover:text-foreground focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
            onClick={() => session.model?.scrollToBottom()}
          >
            <HugeiconsIcon icon={ArrowDown01Icon} size={16} strokeWidth={2} />
          </button>
        )}
        {session.error && (
          <div
            role="alert"
            className="absolute inset-0 z-10 flex flex-col items-center justify-center gap-3 bg-background p-6 text-center text-sm"
          >
            <strong>{terminalErrorTitle(session.error.kind)}</strong>
            <p className="select-text max-w-md break-words text-muted-foreground">
              {session.error.message}
            </p>
            {session.error.kind === "renderer" && (
              <p className="text-muted-foreground">
                Your terminal session is preserved. Retry to restore its
                display.
              </p>
            )}
            <div className="flex items-center gap-2">
              <button
                type="button"
                className="rounded-md border px-3 py-1.5 hover:bg-accent focus-visible:outline-2"
                onClick={session.retry}
              >
                {session.error.kind === "renderer" ? "Retry display" : "Retry"}
              </button>
              <CopyButton
                text={`${terminalErrorTitle(session.error.kind)}\n${session.error.message}`}
              />
            </div>
          </div>
        )}
      </div>
    );
  }),
);

export const TerminalPane = memo(
  forwardRef<TerminalPaneHandle, TerminalPaneProps>(
    function TerminalPane(props, ref) {
      const [backend] = useState(() =>
        terminalBackendFor(usePreferencesStore.getState().terminalRenderer),
      );
      return <GhosttyTerminalPane ref={ref} {...props} backend={backend} />;
    },
  ),
);

function terminalErrorTitle(kind: "startup" | "renderer" | "disconnected") {
  if (kind === "renderer") return "Unable to display terminal";
  if (kind === "disconnected") return "Terminal disconnected";
  return "Unable to start terminal";
}
