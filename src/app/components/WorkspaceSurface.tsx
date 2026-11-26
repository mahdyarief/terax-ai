import {
  type ComponentProps,
  type CSSProperties,
  useCallback,
  useEffect,
  useRef,
} from "react";
import { cn } from "@/lib/utils";
import { AiDiffStack, EditorStack, GitDiffStack } from "@/modules/editor";
import { GitHistoryStack } from "@/modules/git-history";
import { MarkdownStack } from "@/modules/markdown";
import { PreviewStack } from "@/modules/preview";
import type { Tab } from "@/modules/tabs";
import { clampSplitRatio, SPLIT_DEFAULT } from "@/modules/tabs/lib/splitView";
import { TerminalStack } from "@/modules/terminal";
import {
  beginTerminalResizeInteraction,
  endTerminalResizeInteraction,
} from "@/modules/terminal/lib/terminalResizeInteraction";

type TerminalStackProps = ComponentProps<typeof TerminalStack>;
type EditorStackProps = ComponentProps<typeof EditorStack>;
type PreviewStackProps = ComponentProps<typeof PreviewStack>;
type AiDiffStackProps = ComponentProps<typeof AiDiffStack>;
type GitHistoryStackProps = ComponentProps<typeof GitHistoryStack>;

type Props = {
  tabs: Tab[];
  activeId: number;
  activeTab: Tab | undefined;
  registerTerminalHandle: TerminalStackProps["registerHandle"];
  onSearchReady: TerminalStackProps["onSearchReady"];
  onCwd: TerminalStackProps["onCwd"];
  onExit: TerminalStackProps["onExit"];
  onFocusLeaf: TerminalStackProps["onFocusLeaf"];
  onSplitSizes: TerminalStackProps["onSplitSizes"];
  registerEditorHandle: EditorStackProps["registerHandle"];
  onEditorDirtyChange: EditorStackProps["onDirtyChange"];
  onEditorCloseTab: EditorStackProps["onCloseTab"];
  registerPreviewHandle: PreviewStackProps["registerHandle"];
  onPreviewUrlChange: PreviewStackProps["onUrlChange"];
  onAiDiffAccept: AiDiffStackProps["onAccept"];
  onAiDiffReject: AiDiffStackProps["onReject"];
  onOpenCommitFile: GitHistoryStackProps["onOpenCommitFile"];
  onGitHistorySearchHandle: GitHistoryStackProps["onSearchHandle"];
  onSetMarkdownView: EditorStackProps["onSetMarkdownView"];
  /** Open a file that a link in a markdown preview points at. */
  onOpenFile?: (path: string, pin?: boolean, fragment?: string) => void;
  pendingFragment?: { path: string; fragment: string } | null;
  onConsumePendingFragment?: (path: string, fragment: string) => void;
  /** Workspace root used to scope local assets in HTML previews. */
  workspaceRoot?: string | null;
  /** Tab shown on the right of a split view; the active tab is on the left. */
  secondaryActiveId?: number | null;
  secondaryActiveTab?: Tab | undefined;
  /** Share of the width the left pane takes (0..1). */
  splitRatio?: number;
  onSplitRatioChange?: (ratio: number) => void;
};

/**
 * Layers never take clicks themselves: in a split two of them can cover the
 * whole surface, so only the tab wrappers inside opt back in.
 */
const LAYER = "pointer-events-none absolute inset-0";

/** All tab-kind layers, stacked absolutely and toggled by `activeTab.kind`. */
function SurfaceLayers({
  tabs,
  activeId,
  activeTab,
  registerTerminalHandle,
  onSearchReady,
  onCwd,
  onExit,
  onFocusLeaf,
  onSplitSizes,
  registerEditorHandle,
  onEditorDirtyChange,
  onEditorCloseTab,
  registerPreviewHandle,
  onPreviewUrlChange,
  onAiDiffAccept,
  onAiDiffReject,
  onOpenCommitFile,
  onGitHistorySearchHandle,
  onSetMarkdownView,
  onOpenFile,
  pendingFragment,
  onConsumePendingFragment,
  workspaceRoot,
  secondaryActiveId,
  secondaryActiveTab,
}: Props) {
  const secondaryId = secondaryActiveTab ? (secondaryActiveId ?? null) : null;
  // A layer shows when either side of a split holds a tab of its kind.
  const shows = (...kinds: Tab["kind"][]) =>
    (activeTab !== undefined && kinds.includes(activeTab.kind)) ||
    (secondaryActiveTab !== undefined &&
      kinds.includes(secondaryActiveTab.kind));
  const isTerminalTab = shows("terminal");
  const isEditorTab = shows("editor");
  const isPreviewTab = shows("preview");
  const isMarkdownTab = shows("markdown");
  const isAiDiffTab = shows("ai-diff");
  const isGitDiffTab = shows("git-diff", "git-commit-file");
  const isGitHistoryTab = shows("git-history");

  return (
    <div className="relative h-full min-h-0">
      <div
        className={cn(LAYER, "px-3 pt-2 pb-2", !isTerminalTab && "invisible")}
        aria-hidden={!isTerminalTab}
      >
        <TerminalStack
          tabs={tabs}
          activeId={activeId}
          secondaryId={secondaryId}
          registerHandle={registerTerminalHandle}
          onSearchReady={onSearchReady}
          onCwd={onCwd}
          onExit={onExit}
          onFocusLeaf={onFocusLeaf}
          onSplitSizes={onSplitSizes}
        />
      </div>
      <div
        className={cn(LAYER, !isEditorTab && "invisible")}
        aria-hidden={!isEditorTab}
      >
        <EditorStack
          tabs={tabs}
          activeId={activeId}
          secondaryId={secondaryId}
          registerHandle={registerEditorHandle}
          onDirtyChange={onEditorDirtyChange}
          onCloseTab={onEditorCloseTab}
          onSetMarkdownView={onSetMarkdownView}
          onOpenFile={onOpenFile}
          workspaceRoot={workspaceRoot}
        />
      </div>
      <div
        className={cn(LAYER, !isPreviewTab && "invisible")}
        aria-hidden={!isPreviewTab}
      >
        <PreviewStack
          tabs={tabs}
          activeId={activeId}
          secondaryId={secondaryId}
          registerHandle={registerPreviewHandle}
          onUrlChange={onPreviewUrlChange}
        />
      </div>
      <div
        className={cn(LAYER, !isMarkdownTab && "invisible")}
        aria-hidden={!isMarkdownTab}
      >
        <MarkdownStack
          tabs={tabs}
          activeId={activeId}
          secondaryId={secondaryId}
          onSetMarkdownView={onSetMarkdownView}
          onOpenFile={onOpenFile}
          pendingFragment={pendingFragment}
          onConsumePendingFragment={onConsumePendingFragment}
        />
      </div>
      <div
        className={cn(LAYER, !isAiDiffTab && "invisible")}
        aria-hidden={!isAiDiffTab}
      >
        <AiDiffStack
          tabs={tabs}
          activeId={activeId}
          secondaryId={secondaryId}
          onAccept={onAiDiffAccept}
          onReject={onAiDiffReject}
        />
      </div>
      <div
        className={cn(LAYER, !isGitDiffTab && "invisible")}
        aria-hidden={!isGitDiffTab}
      >
        <GitDiffStack
          tabs={tabs}
          activeId={activeId}
          secondaryId={secondaryId}
        />
      </div>
      <div
        className={cn(LAYER, !isGitHistoryTab && "invisible")}
        aria-hidden={!isGitHistoryTab}
      >
        <GitHistoryStack
          tabs={tabs}
          activeId={activeId}
          secondaryId={secondaryId}
          onOpenCommitFile={onOpenCommitFile}
          onSearchHandle={onGitHistorySearchHandle}
        />
      </div>
    </div>
  );
}

/** Keyboard step of the divider, as a share of the width. */
const KEY_STEP = 0.05;

function SplitDivider({
  ratio,
  surface,
  onCommit,
}: {
  ratio: number;
  surface: React.RefObject<HTMLDivElement | null>;
  onCommit: (ratio: number) => void;
}) {
  const token = useRef<object>({});
  const cleanup = useRef<(() => void) | null>(null);
  const finish = useCallback(() => {
    cleanup.current?.();
    cleanup.current = null;
    endTerminalResizeInteraction(token.current);
  }, []);
  useEffect(() => finish, [finish]);

  const ratioAt = (clientX: number) => {
    const rect = surface.current?.getBoundingClientRect();
    if (!rect || rect.width <= 0) return ratio;
    return clampSplitRatio((clientX - rect.left) / rect.width);
  };

  const onPointerDown = (e: React.PointerEvent<HTMLDivElement>) => {
    if (e.button !== 0) return;
    e.preventDefault();
    finish();
    beginTerminalResizeInteraction(token.current);
    let latest = ratio;
    const body = document.body.style;
    const prevCursor = body.cursor;
    const prevSelect = body.userSelect;
    body.cursor = "col-resize";
    body.userSelect = "none";
    // Restyle through the variable while dragging; React only hears the end.
    const onMove = (ev: PointerEvent) => {
      latest = ratioAt(ev.clientX);
      surface.current?.style.setProperty("--terax-split", `${latest * 100}%`);
    };
    const onUp = () => {
      finish();
      onCommit(latest);
    };
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp, true);
    window.addEventListener("pointercancel", onUp, true);
    cleanup.current = () => {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp, true);
      window.removeEventListener("pointercancel", onUp, true);
      body.cursor = prevCursor;
      body.userSelect = prevSelect;
    };
  };

  return (
    // biome-ignore lint/a11y/useSemanticElements: a focusable, draggable separator; <hr> cannot take focus or pointer drags.
    <div
      role="separator"
      aria-orientation="vertical"
      aria-label="Resize split view"
      aria-valuemin={20}
      aria-valuemax={80}
      aria-valuenow={Math.round(ratio * 100)}
      tabIndex={0}
      title="Drag to resize · double-click to reset"
      onPointerDown={onPointerDown}
      onDoubleClick={() => onCommit(SPLIT_DEFAULT)}
      onKeyDown={(e) => {
        if (e.key === "ArrowLeft") onCommit(clampSplitRatio(ratio - KEY_STEP));
        else if (e.key === "ArrowRight")
          onCommit(clampSplitRatio(ratio + KEY_STEP));
        else return;
        e.preventDefault();
      }}
      className="group absolute inset-y-0 z-20 flex w-3 -translate-x-1/2 cursor-col-resize justify-center outline-none"
      style={{ left: "var(--terax-split)" }}
    >
      <span className="h-full w-px bg-border transition-colors duration-[var(--dur-fast)] group-hover:bg-primary/50 group-focus-visible:bg-primary group-active:bg-primary" />
    </div>
  );
}

export function WorkspaceSurface(props: Props) {
  const {
    secondaryActiveId,
    secondaryActiveTab,
    splitRatio = SPLIT_DEFAULT,
    onSplitRatioChange,
  } = props;
  const surface = useRef<HTMLDivElement>(null);
  const split = secondaryActiveId != null && secondaryActiveTab !== undefined;
  const ratio = clampSplitRatio(splitRatio);

  // One tree for both sides: each stack lays its tabs out in the left or right
  // slot, so splitting never remounts a tab (a remount closes a terminal's
  // shell and drops an editor's unsaved text).
  return (
    <div
      ref={surface}
      className="relative h-full min-h-0 w-full"
      style={{ "--terax-split": `${ratio * 100}%` } as CSSProperties}
    >
      <SurfaceLayers {...props} />
      {split && (
        <SplitDivider
          ratio={ratio}
          surface={surface}
          onCommit={(next) => {
            surface.current?.style.setProperty(
              "--terax-split",
              `${next * 100}%`,
            );
            onSplitRatioChange?.(next);
          }}
        />
      )}
    </div>
  );
}
