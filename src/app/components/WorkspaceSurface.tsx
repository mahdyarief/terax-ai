import type { ComponentProps } from "react";
import { cn } from "@/lib/utils";
import { AiDiffStack, EditorStack, GitDiffStack } from "@/modules/editor";
import { GitHistoryStack } from "@/modules/git-history";
import { MarkdownStack } from "@/modules/markdown";
import { PreviewStack } from "@/modules/preview";
import type { Tab } from "@/modules/tabs";
import { TerminalStack } from "@/modules/terminal";

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
  /** When set, the active tab is mirrored into a side-by-side secondary pane. */
  secondaryActiveId?: number | null;
  secondaryActiveTab?: Tab | undefined;
  onCloseSecondary?: () => void;
};

const LAYER = "absolute inset-0";

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
}: Props) {
  const kind = activeTab?.kind;
  const isTerminalTab = kind === "terminal";
  const isEditorTab = kind === "editor";
  const isPreviewTab = kind === "preview";
  const isMarkdownTab = kind === "markdown";
  const isAiDiffTab = kind === "ai-diff";
  const isGitDiffTab = kind === "git-diff" || kind === "git-commit-file";
  const isGitHistoryTab = kind === "git-history";

  return (
    <div className="relative h-full min-h-0">
      <div
        className={cn(
          LAYER,
          "px-3 pt-2 pb-2",
          !isTerminalTab && "invisible pointer-events-none",
        )}
        aria-hidden={!isTerminalTab}
      >
        <TerminalStack
          tabs={tabs}
          activeId={activeId}
          registerHandle={registerTerminalHandle}
          onSearchReady={onSearchReady}
          onCwd={onCwd}
          onExit={onExit}
          onFocusLeaf={onFocusLeaf}
        />
      </div>
      <div
        className={cn(LAYER, !isEditorTab && "invisible pointer-events-none")}
        aria-hidden={!isEditorTab}
      >
        <EditorStack
          tabs={tabs}
          activeId={activeId}
          registerHandle={registerEditorHandle}
          onDirtyChange={onEditorDirtyChange}
          onCloseTab={onEditorCloseTab}
          onSetMarkdownView={onSetMarkdownView}
        />
      </div>
      <div
        className={cn(LAYER, !isPreviewTab && "invisible pointer-events-none")}
        aria-hidden={!isPreviewTab}
      >
        <PreviewStack
          tabs={tabs}
          activeId={activeId}
          registerHandle={registerPreviewHandle}
          onUrlChange={onPreviewUrlChange}
        />
      </div>
      <div
        className={cn(LAYER, !isMarkdownTab && "invisible pointer-events-none")}
        aria-hidden={!isMarkdownTab}
      >
        <MarkdownStack
          tabs={tabs}
          activeId={activeId}
          onSetMarkdownView={onSetMarkdownView}
        />
      </div>
      <div
        className={cn(LAYER, !isAiDiffTab && "invisible pointer-events-none")}
        aria-hidden={!isAiDiffTab}
      >
        <AiDiffStack
          tabs={tabs}
          activeId={activeId}
          onAccept={onAiDiffAccept}
          onReject={onAiDiffReject}
        />
      </div>
      <div
        className={cn(LAYER, !isGitDiffTab && "invisible pointer-events-none")}
        aria-hidden={!isGitDiffTab}
      >
        <GitDiffStack tabs={tabs} activeId={activeId} />
      </div>
      <div
        className={cn(
          LAYER,
          !isGitHistoryTab && "invisible pointer-events-none",
        )}
        aria-hidden={!isGitHistoryTab}
      >
        <GitHistoryStack
          tabs={tabs}
          activeId={activeId}
          onOpenCommitFile={onOpenCommitFile}
          onSearchHandle={onGitHistorySearchHandle}
        />
      </div>
    </div>
  );
}

export function WorkspaceSurface(props: Props) {
  const {
    tabs,
    secondaryActiveId,
    secondaryActiveTab,
    onCloseSecondary,
  } = props;

  if (secondaryActiveId == null || !secondaryActiveTab) {
    return <SurfaceLayers {...props} />;
  }

  // Primary shows every tab except the secondary one; the secondary pane shows
  // only that tab. Splitting so no tab id is mounted twice avoids duplicate
  // LSP didOpen / iframe / registerHandle calls.
  const primaryTabs = tabs.filter((t) => t.id !== secondaryActiveId);
  const secondaryTabs = tabs.filter((t) => t.id === secondaryActiveId);

  return (
    <div className="flex h-full min-h-0 w-full flex-row">
      <div className="relative min-h-0 flex-1">
        <SurfaceLayers {...props} tabs={primaryTabs} />
      </div>
      <div className="w-px shrink-0 bg-border" />
      <div className="relative min-h-0 flex-1">
        <button
          type="button"
          onClick={onCloseSecondary}
          className="absolute right-2 top-2 z-10 rounded bg-background/80 px-2 py-0.5 text-xs text-muted-foreground hover:bg-accent hover:text-accent-foreground"
          aria-label="Close side pane"
        >
          ✕
        </button>
        <SurfaceLayers
          {...props}
          tabs={secondaryTabs}
          activeId={secondaryActiveId}
          activeTab={secondaryActiveTab}
        />
      </div>
    </div>
  );
}
