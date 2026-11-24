import {
  ResizableHandle,
  ResizablePanel,
  ResizablePanelGroup,
} from "@/components/ui/resizable";
import { Toaster } from "@/components/ui/sonner";
import { toast } from "sonner";
import { TooltipProvider } from "@/components/ui/tooltip";
import { consumeLaunchFiles, getLaunchDir } from "@/lib/launchDir";
import { IS_MAC } from "@/lib/platform";
import { quoteShellArg } from "@/lib/shellQuote";
import { usePresence } from "@/lib/usePresence";
import { useZoom } from "@/lib/useZoom";
import { isMarkdownPath } from "@/lib/utils";
import { MARKDOWN_FIND_EVENT, type MarkdownViewMode } from "@/modules/markdown";
import {
  type AgentLaunchRequest,
  AgentNotificationsBridge,
  findAgentLauncher,
  nextAttentionTarget,
  validateAgentLaunchCommand,
} from "@/modules/agents";
import {
  AgentRunBridge,
  AiMiniWindow,
  LocalAgentNotificationsBridge,
  SelectionAskAi,
  useAiBootstrap,
  useAiLiveBridge,
  useChatStore,
  useSelectionAskAi,
} from "@/modules/ai";
import { AiComposerProvider } from "@/modules/ai/lib/composer";
import { native } from "@/modules/ai/lib/native";
import {
  CommandPalette,
  createCommandItems,
  type OpenFileTarget,
  type PaletteInitialMode,
} from "@/modules/command-palette";
import { useControlBridge } from "@/modules/control";
import {
  type EditorPaneHandle,
  NewEditorDialog,
  ProblemsPanel,
  useApplyEditorFontSize,
  useEditorFileSync,
} from "@/modules/editor";
import { FileExplorer, type FileExplorerHandle } from "@/modules/explorer";
import type { GitHistorySearchHandle } from "@/modules/git-history";
import {
  Header,
  type SearchInlineHandle,
  type SearchTarget,
} from "@/modules/header";
import { setLspNavigator } from "@/modules/lsp";
import { setPreviewNavigator } from "@/modules/preview";
import { isAllowedPreviewUrl } from "@/modules/preview/lib/previewUrl";
import type { PreviewPaneHandle } from "@/modules/preview";
import { openSettingsWindow } from "@/modules/settings/openSettingsWindow";
import { usePreferencesStore } from "@/modules/settings/preferences";
import { setShowHidden } from "@/modules/settings/store";
import {
  type ShortcutHandlers,
  type ShortcutId,
  shouldDisableAskSelection,
  shouldDisablePaneSwapShortcut,
  tabDigit,
  useGlobalShortcuts,
} from "@/modules/shortcuts";
import {
  SIDEBAR_MAX_WIDTH,
  SIDEBAR_MIN_WIDTH,
  SidebarRail,
  useSidebarPanel,
} from "@/modules/sidebar";
import {
  SourceControlPanel,
  useRepositoryTargeting,
  useSourceControlContext,
} from "@/modules/source-control";
import {
  SpaceSwitcher,
  useSpacePersistence,
  useSpaces,
  useSpacesBoot,
} from "@/modules/spaces";
import { selectHostedSpaces } from "@/modules/spaces/lib/useSpaces";
import {
  spaceTransferToastId,
  useSpaceTransfers,
} from "@/modules/spaces/lib/spaceTransfers";
import {
  reclaimHost,
  SPACE_RECLAIM_REQUEST_EVENT,
  SPACE_TABS_ATTACHED_EVENT,
  SPACE_WINDOW_READY_EVENT,
  type HostChangedPayload,
  type TabsAttachedPayload,
  type TabsMovedPayload,
  type WindowReadyPayload,
} from "@/modules/spaces/lib/crossWindowSync";
import { createAttachWaiter } from "@/modules/spaces/lib/attachWaiter";
import {
  isSpaceWindow,
  pickHandoffFallback,
  planSpaceHandoff,
  spaceWindowLabel,
} from "@/modules/spaces/lib/handoff";
import { moveTabsAcrossWindows } from "@/modules/spaces/lib/moveTabsAcrossWindows";
import {
  hydrateTabs,
  isHandoffMovable,
  serializeTabsForHandoff,
  stampHandoffAttachments,
} from "@/modules/spaces/lib/serialize";
import {
  forgetWindowReady,
  isWindowReady,
  noteWindowReady,
  waitForWindowReady,
} from "@/modules/spaces/lib/windowReady";
import {
  buildAttachHandoff,
  nextHandoffId,
  type PtyHandoffCheckpoint,
  type PtyReplayPayload,
} from "@/modules/terminal/lib/ptyHandoff";
import { StatusBar } from "@/modules/statusbar";
import {
  TabSwitcherHud,
  type CloseTabsPlan,
  type Tab,
  useTabSwitcher,
  useTabs,
  useWindowTitle,
  useWorkspaceCwd,
  setTabHostingGuard,
} from "@/modules/tabs";
import {
  planSplitRight,
  reconcileSecondary,
  SPLIT_DEFAULT,
} from "@/modules/tabs/lib/splitView";
import { DEFAULT_SPACE_ID } from "@/modules/tabs/lib/useTabs";
import {
  clearFocusedTerminal,
  disposeSession,
  findLeafCwd,
  hasLeaf,
  isTerminalSurfaceTarget,
  leafIds,
  navigateFocusedBlocks,
  dropHandoffReplays,
  queueHandoffReplay,
  type PaneBounds,
  ptyIdForLeaf,
  scrollFocusedTerminalToBottom,
  type TerminalPaneHandle,
  useAgentActivityStore,
  useTerminalFileDrop,
  whenSessionReady,
  writeToSession,
} from "@/modules/terminal";
import type { TerminalSearchController } from "@/modules/terminal/search/TerminalSearchController";
import { bindTerminalWindowFocus } from "@/modules/terminal/ghostty/renderScheduling";
import { subscribeWindowPresentation } from "@/modules/terminal/ghostty/windowPresentation";
import {
  LivePageResizeBridge,
  ThemeProvider,
  useThemeFileEditing,
  WindowActivityBridge,
  WindowVibrancyBridge,
} from "@/modules/theme";
import { UpdaterDialog } from "@/modules/updater";
import {
  AskpassDialog,
  folderName,
  RemoteConnections,
  RemoteGate,
  useRemoteStore,
} from "@/modules/remote";
import {
  useWorkspaceEnvStore,
  type WorkspaceEnv,
  workspaceScopeKey,
} from "@/modules/workspace";
import { invoke } from "@tauri-apps/api/core";
import { emit, emitTo, listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  lazy,
  Suspense,
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { CloseDialogs } from "./components/CloseDialogs";
import {
  TOGGLE_BLOCK_INPUT_EVENT,
  WorkspaceInputBar,
} from "./components/WorkspaceInputBar";
import { WorkspaceSurface } from "./components/WorkspaceSurface";
import { useAppCloseGuard } from "./hooks/useAppCloseGuard";
import {
  hasOpenPathTab,
  renamedPath,
  spacesEmptiedByTabs,
} from "./hooks/tabCloseGuards";
import { useTabCloseGuards } from "./hooks/useTabCloseGuards";
import { useWorkspaceSwitcher } from "./hooks/useWorkspaceSwitcher";

// Real-application benchmark driver (scripts/app-lab). `__TERAX_BENCH__` is the literal `false`
// in a normal build, so this is `null` and neither the import nor its chunk is emitted.
const BenchBridge = __TERAX_BENCH__
  ? lazy(() => import("@/bench/BenchBridge"))
  : null;

const tauriAttachListen = (
  event: string,
  fn: (event: { payload: TabsAttachedPayload }) => void,
) => listen(event, fn);

async function windowExists(label: string): Promise<boolean> {
  try {
    const { WebviewWindow } = await import("@tauri-apps/api/webviewWindow");
    return (await WebviewWindow.getByLabel(label)) !== null;
  } catch {
    // Unknown: keep the readiness we have rather than wait on a guess.
    return true;
  }
}

function restoreHandoffSource(
  snapshot: Tab[],
  spaceId: string,
  replays: PtyReplayPayload[],
  checkpoints: PtyHandoffCheckpoint[],
  leafPtyIds: Array<number | null>,
  allocId: () => number,
): Tab[] {
  for (const replay of replays) queueHandoffReplay(replay);
  const serialized = stampHandoffAttachments(
    serializeTabsForHandoff(snapshot),
    checkpoints,
    leafPtyIds,
  );
  return hydrateTabs(serialized, spaceId, allocId, { keepPty: true });
}

export default function App() {
  const {
    tabs,
    activeId,
    setActiveId,
    allocId,
    booted,
    replaceTabs,
    moveTabToSpace,
    reorderTab,
    reorderTabByGap,
    newTabInSpace,
    removeTabsForSpace,
    detachTabsForHandoff,
    attachTabsForSpace,
    markBooted,
    setActiveSpaceForNewTabs,
    newTab,
    newBlockTab,
    newAgentTab,
    newAgentGroupTab,
    newPrivateTab,
    openFileTab,
    pinTab,
    newPreviewTab,
    newMarkdownTab,
    setMarkdownView,
    setOverrideLanguage,
    openAiDiffTab,
    closeAiDiffTab,
    openGitDiffTab,
    openCommitHistoryTab,
    openCommitFileDiffTab,
    closeTab,
    closeTabs,
    updateTab,
    selectByIndex,
    setLeafCwd,
    focusPane,
    focusNextPaneInTab,
    swapActivePaneInDirection,
    splitActivePane,
    closeActivePane,
    closePaneByLeaf,
    resetWorkspace,
  } = useTabs(getLaunchDir() ? { cwd: getLaunchDir() } : undefined);

  // Mirror `tabs` into a ref so callbacks scheduled with `setTimeout`
  // (e.g. cdInNewTab) read the latest pane state instead of a stale closure.
  const tabsRef = useRef(tabs);
  const activeIdRef = useRef(activeId);

  const activeTerminalTab = useMemo(() => {
    const t = tabs.find((x) => x.id === activeId);
    return t && t.kind === "terminal" ? t : null;
  }, [tabs, activeId]);
  const activeLeafId = activeTerminalTab?.activeLeafId ?? null;

  const searchAddons = useRef<Map<number, TerminalSearchController>>(new Map());
  const [activeSearchAddon, setActiveSearchAddon] =
    useState<TerminalSearchController | null>(null);
  const searchInlineRef = useRef<SearchInlineHandle | null>(null);
  const terminalRefs = useRef<Map<number, TerminalPaneHandle>>(new Map());
  const editorRefs = useRef<Map<number, EditorPaneHandle>>(new Map());
  const previewRefs = useRef<Map<number, PreviewPaneHandle>>(new Map());
  const [activeEditorHandle, setActiveEditorHandle] =
    useState<EditorPaneHandle | null>(null);
  const [gitHistoryHandle, setGitHistoryHandle] =
    useState<GitHistorySearchHandle | null>(null);
  const { zoomIn, zoomOut, zoomReset } = useZoom();
  useApplyEditorFontSize();
  const terminalPathDropTarget = useTerminalFileDrop();
  const explorerRef = useRef<FileExplorerHandle>(null);

  // Drives session disposal off the pane tree, not React lifecycles —
  // split/unsplit re-mount components but the leaf is still live.
  const liveLeavesRef = useRef<Set<number>>(new Set());

  const clearWorkspaceState = useCallback(() => {
    for (const id of liveLeavesRef.current) disposeSession(id);
    searchAddons.current.clear();
    terminalRefs.current.clear();
    editorRefs.current.clear();
    previewRefs.current.clear();
    setActiveSearchAddon(null);
    setActiveEditorHandle(null);
  }, []);

  const workspaceEnv = useWorkspaceEnvStore((s) => s.env);
  const setWorkspaceEnv = useWorkspaceEnvStore((s) => s.setEnv);
  const {
    home,
    launchCwd,
    launchCwdResolved,
    switchWorkspace,
    adoptWorkspaceEnv,
  } = useWorkspaceSwitcher({
    tabsRef,
    workspaceEnv,
    setWorkspaceEnv,
    resetWorkspace,
    clearWorkspaceState,
  });

  const activeSpaceId = useSpaces((s) => s.activeId);
  const activeSpaceName = useSpaces(
    (s) => s.spaces.find((sp) => sp.id === s.activeId)?.name ?? null,
  );
  const activeSpaceRoot = useSpaces(
    (s) => s.spaces.find((sp) => sp.id === s.activeId)?.root ?? null,
  );
  const spacesHydrated = useSpaces((s) => s.hydrated);
  const activeSpaceIdRef = useRef(activeSpaceId);
  useLayoutEffect(() => {
    tabsRef.current = tabs;
    activeIdRef.current = activeId;
    activeSpaceIdRef.current = activeSpaceId;
  }, [tabs, activeId, activeSpaceId]);
  const sourceControlSpaceId = activeSpaceId ?? DEFAULT_SPACE_ID;

  const handleWorkspaceChange = useCallback(
    async (env: WorkspaceEnv) => {
      const switched = await switchWorkspace(env);
      if (switched && activeSpaceId) {
        useSpaces.getState().setEnv(activeSpaceId, env);
      }
    },
    [switchWorkspace, activeSpaceId],
  );

  const windowLabel = getCurrentWindow().label;

  useSpacesBoot({
    ready: launchCwdResolved,
    launchCwd,
    home,
    windowLabel,
    allocId,
    replaceTabs,
    markBooted,
    setActiveSpaceForNewTabs,
    adoptWorkspaceEnv,
  });

  const { flushNow } = useSpacePersistence({
    tabs,
    activeId,
    activeSpaceId: activeSpaceId ?? DEFAULT_SPACE_ID,
    enabled: spacesHydrated,
  });

  useEffect(() => {
    setTabHostingGuard((spaceId) => {
      const space = useSpaces
        .getState()
        .spaces.find((s) => s.id === spaceId);
      return !space || (space.hostWindowLabel ?? "main") === windowLabel;
    });
  }, [windowLabel]);

  // Incoming handoffs this window is attaching or just attached, so a later
  // correction from the source can undo exactly that handoff's tabs.
  const incomingHandoffsRef = useRef(
    new Map<string, { ptyIds: number[]; tabIds: number[] }>(),
  );
  const abandonedHandoffsRef = useRef(new Set<string>());

  const prevSpaceRef = useRef<string | null>(null);
  useEffect(() => {
    if (!spacesHydrated || !activeSpaceId) return;
    setActiveSpaceForNewTabs(activeSpaceId);
    const prev = prevSpaceRef.current;
    prevSpaceRef.current = activeSpaceId;
    if (prev === activeSpaceId) return;
    // A window that boots with no hosted space has no active space until a
    // handoff arrives (`prev === null`), so first activation must adopt too:
    // PTY spawn reads the workspace env, and a stale default spawns in the
    // wrong directory (or local instead of WSL).
    const meta = useSpaces
      .getState()
      .spaces.find((s) => s.id === activeSpaceId);
    if (meta) void adoptWorkspaceEnv(meta.env);
    const inSpace = tabsRef.current.filter((t) => t.spaceId === activeSpaceId);
    if (inSpace.length === 0) return;
    // Keep the active tab if it already belongs to the newly active space (a
    // cross-space jump set it explicitly); else fall to the space's last tab.
    if (inSpace.some((t) => t.id === activeId)) return;
    setActiveId(inSpace[inSpace.length - 1].id);
  }, [
    activeSpaceId,
    activeId,
    spacesHydrated,
    setActiveSpaceForNewTabs,
    setActiveId,
    adoptWorkspaceEnv,
  ]);

  const [switcherOpen, setSwitcherOpen] = useState(false);

  const spaceTabs = useMemo(
    () => tabs.filter((t) => t.spaceId === (activeSpaceId ?? DEFAULT_SPACE_ID)),
    [tabs, activeSpaceId],
  );

  const {
    sidebarRef,
    sidebarWidthRef,
    sidebarView,
    sidebarCollapsed,
    initialSidebarCollapsed,
    persistSidebarView,
    persistSidebarCollapsed,
    toggleSidebar,
    cycleSidebarView,
    openSidebarView,
    persistSidebarWidth,
    toggleExplorerFocus,
  } = useSidebarPanel(explorerRef);

  const [newEditorOpen, setNewEditorOpen] = useState(false);
  const [commandPaletteOpen, setCommandPaletteOpen] = useState(false);
  const [paletteInitialMode, setPaletteInitialMode] =
    useState<PaletteInitialMode>("commands");
  const openCommandPalette = useCallback(
    (mode: PaletteInitialMode = "commands") => {
      setPaletteInitialMode(mode);
      setCommandPaletteOpen(true);
    },
    [],
  );
  const miniOpen = useChatStore((s) => s.mini.open);
  const miniPresence = usePresence(miniOpen, 200);
  const openMini = useChatStore((s) => s.openMini);
  const toggleMini = useChatStore((s) => s.toggleMini);
  const focusInput = useChatStore((s) => s.focusInput);
  const openPanel = useChatStore((s) => s.openPanel);
  const panelOpen = useChatStore((s) => s.panelOpen);
  const setLive = useChatStore((s) => s.setLive);
  const respondToApproval = useChatStore((s) => s.respondToApproval);

  const { hasComposer, keysLoaded } = useAiBootstrap();

  const activeTab = tabs.find((t) => t.id === activeId);
  // Split view: the active tab on the left, `secondaryTabId` on the right.
  const [rawSecondaryTabId, setSecondaryTabId] = useState<number | null>(null);
  const [splitRatio, setSplitRatio] = useState(SPLIT_DEFAULT);
  // Read through the rules on every render, so a stale id (closed, moved to
  // another space, or now the active tab) never shows for a frame.
  const secondaryTabId = reconcileSecondary(tabs, activeId, rawSecondaryTabId);
  const secondaryActiveTab =
    secondaryTabId == null
      ? undefined
      : tabs.find((t) => t.id === secondaryTabId);
  useEffect(() => {
    if (rawSecondaryTabId !== null && secondaryTabId === null)
      setSecondaryTabId(null);
  }, [rawSecondaryTabId, secondaryTabId]);
  const splitTabRight = useCallback(
    (tabId: number) => {
      // Read through refs so the callback stays stable across tab switches;
      // it reaches memoized children (tab bar, explorer).
      const current = activeIdRef.current;
      const plan = planSplitRight(tabsRef.current, current, tabId);
      if (!plan) return;
      if (plan.activeId !== current) setActiveId(plan.activeId);
      setSecondaryTabId(plan.secondaryId);
    },
    [setActiveId],
  );
  const closeSplit = useCallback(() => setSecondaryTabId(null), []);
  // The right-hand tab is already on screen: picking it in the tab bar moves
  // the keyboard into its pane instead of rearranging the split.
  const selectTab = useCallback(
    (id: number) => {
      if (id !== secondaryTabId) {
        setActiveId(id);
        return;
      }
      const tab = tabsRef.current.find((t) => t.id === id);
      if (tab?.kind === "terminal")
        terminalRefs.current.get(tab.activeLeafId)?.focus();
      else editorRefs.current.get(id)?.focus();
    },
    [secondaryTabId, setActiveId],
  );
  const isTerminalTab = activeTab?.kind === "terminal";
  const isBlockTab = activeTerminalTab?.blocks === true;
  const isEditorTab = activeTab?.kind === "editor";
  const isGitHistoryTab = activeTab?.kind === "git-history";

  useEditorFileSync({ tabs, tabsRef, editorRefs });
  useThemeFileEditing({ tabsRef, openFileTab });

  const { explorerRoot, explorerViewRoot, inheritedCwdForNewTab } =
    useWorkspaceCwd(activeTab, tabs, launchCwd ?? home, activeSpaceRoot);

  useWindowTitle(
    activeTab,
    explorerRoot,
    windowLabel !== "main" ? activeSpaceName : null,
  );

  useEffect(() => {
    setActiveSearchAddon(
      activeLeafId !== null
        ? (searchAddons.current.get(activeLeafId) ?? null)
        : null,
    );
    setActiveEditorHandle(editorRefs.current.get(activeId) ?? null);
  }, [activeId, activeLeafId]);

  const handleSearchReady = useCallback(
    (leafId: number, addon: TerminalSearchController) => {
      searchAddons.current.set(leafId, addon);
      if (leafId === activeLeafId) setActiveSearchAddon(addon);
    },
    [activeLeafId],
  );

  const disposeTab = useCallback(
    (id: number) => {
      // Terminal-leaf-keyed maps (terminalRefs/searchAddons) are pruned by
      // the effect below as the pane tree changes; only the tab-id-keyed
      // handles need explicit cleanup here.
      editorRefs.current.delete(id);
      previewRefs.current.delete(id);
      closeTab(id);
    },
    [closeTab],
  );

  const disposeTabs = useCallback(
    (anchorId: number, plan: CloseTabsPlan) => {
      const closedIds = closeTabs(anchorId, plan);
      for (const id of closedIds) {
        editorRefs.current.delete(id);
        previewRefs.current.delete(id);
      }
    },
    [closeTabs],
  );

  const disposeDeletedTabs = useCallback(
    (ids: number[]) => {
      if (ids.length === 0) return;
      for (const spaceId of spacesEmptiedByTabs(tabsRef.current, ids)) {
        const root = useSpaces
          .getState()
          .spaces.find((s) => s.id === spaceId)?.root;
        newTabInSpace(spaceId, root ?? undefined);
      }
      for (const id of ids) disposeTab(id);
    },
    [disposeTab, newTabInSpace],
  );

  const {
    pendingCloseTab,
    pendingTerminalCloseTab,
    pendingDeleteTabs,
    pendingCloseMany,
    closeManyConfirming,
    handleClose,
    handleCloseTabsToRight,
    handleCloseOtherTabs,
    confirmClose,
    cancelClose,
    confirmTerminalClose,
    cancelTerminalClose,
    confirmDeleteClose,
    cancelDeleteClose,
    confirmCloseMany,
    cancelCloseMany,
    handlePathsDeleted,
  } = useTabCloseGuards({
    tabs,
    activeId,
    disposeTab,
    disposeDeletedTabs,
    disposeTabs,
  });

  const { pendingAppClose, confirmAppClose, cancelAppClose } =
    useAppCloseGuard(tabsRef);

  useEffect(() => {
    const live = new Set<number>();
    for (const t of tabs) {
      if (t.kind === "terminal") {
        for (const id of leafIds(t.paneTree)) live.add(id);
      }
    }
    for (const id of liveLeavesRef.current) {
      if (!live.has(id)) disposeSession(id);
    }
    liveLeavesRef.current = live;
    for (const k of [...terminalRefs.current.keys()])
      if (!live.has(k)) terminalRefs.current.delete(k);
    for (const k of [...searchAddons.current.keys()])
      if (!live.has(k)) searchAddons.current.delete(k);
  }, [tabs]);

  useEffect(() => {
    const tab = tabsRef.current.find((t) => t.id === activeId);
    if (tab?.kind !== "terminal") return;
    const ptyIds = leafIds(tab.paneTree).flatMap((leafId) => {
      const ptyId = ptyIdForLeaf(leafId);
      return ptyId === null ? [] : [ptyId];
    });
    useAgentActivityStore.getState().acknowledgeAttention(ptyIds);
  }, [activeId]);

  // Most-recently-used tab ids, most recent first, pruned to live tabs. Drives
  // the Ctrl+Tab quick switcher so it cycles by recency, not strip order.
  const mruRef = useRef<number[]>([activeId]);
  useEffect(() => {
    mruRef.current = [
      activeId,
      ...mruRef.current.filter((id) => id !== activeId),
    ];
  }, [activeId]);
  useEffect(() => {
    const live = new Set(tabs.map((t) => t.id));
    mruRef.current = mruRef.current.filter((id) => live.has(id));
  }, [tabs]);

  const getSwitcherOrder = useCallback(() => {
    const space = activeSpaceId ?? DEFAULT_SPACE_ID;
    const inSpace = tabsRef.current
      .filter((t) => t.spaceId === space)
      .map((t) => t.id);
    const present = new Set(inSpace);
    const ordered = mruRef.current.filter((id) => present.has(id));
    for (const id of inSpace) if (!ordered.includes(id)) ordered.push(id);
    return [activeId, ...ordered.filter((id) => id !== activeId)];
  }, [activeId, activeSpaceId]);

  const { state: switcherState, step: stepSwitcher } = useTabSwitcher({
    getOrder: getSwitcherOrder,
    onCommit: (id) => {
      if (tabsRef.current.some((t) => t.id === id)) setActiveId(id);
    },
  });

  const cycleSpace = useCallback((delta: 1 | -1) => {
    const { spaces: all, activeId: sid, setActive } = useSpaces.getState();
    // Only cycle through spaces this window shows; a space hosted elsewhere
    // would render an empty tab list here. A space window has just one.
    const spaces = selectHostedSpaces(all, windowLabel);
    if (spaces.length < 2) return;
    const idx = spaces.findIndex((s) => s.id === sid);
    const next = (idx + delta + spaces.length) % spaces.length;
    setActive(spaces[next].id);
  }, [windowLabel]);

  const captureActiveSelection = useCallback((): string | null => {
    const t = tabs.find((x) => x.id === activeId);
    if (!t) return null;
    if (t.kind === "terminal") {
      const lid = t.activeLeafId;
      return terminalRefs.current.get(lid)?.getSelection() ?? null;
    }
    if (t.kind === "editor") {
      return editorRefs.current.get(activeId)?.getSelection() ?? null;
    }
    return null;
  }, [tabs, activeId]);

  const togglePanelAndFocus = useCallback(() => {
    if (!hasComposer) {
      void openSettingsWindow("models");
      return;
    }
    if (panelOpen) {
      useChatStore.getState().closePanel();
    } else {
      openPanel();
      focusInput(null);
    }
  }, [hasComposer, panelOpen, openPanel, focusInput]);

  const attachSelection = useChatStore((s) => s.attachSelection);

  const handleAttachFileToAgent = useCallback(
    (path: string) => {
      if (!hasComposer) {
        void openSettingsWindow("models");
        return;
      }
      // Dispatch a window event the composer listens for. Same pattern as
      // selections — keeps file-explorer decoupled from the AI module.
      window.dispatchEvent(
        new CustomEvent<string>("terax:ai-attach-file", { detail: path }),
      );
      openPanel();
      focusInput(null);
    },
    [hasComposer, openPanel, focusInput],
  );

  const askFromSelection = useCallback(() => {
    if (!hasComposer) {
      void openSettingsWindow("models");
      return;
    }
    const selection = captureActiveSelection();
    if (!selection || !selection.trim()) {
      focusInput(null);
      return;
    }
    const source: "terminal" | "editor" =
      activeTab?.kind === "editor" ? "editor" : "terminal";
    attachSelection(selection, source);
  }, [
    hasComposer,
    captureActiveSelection,
    focusInput,
    attachSelection,
    activeTab,
  ]);

  const { askPopup, setAskPopup, onAskFromSelection } = useSelectionAskAi({
    captureActiveSelection,
    askFromSelection,
  });
  const askPresence = usePresence(Boolean(askPopup), 120);

  const openNewTab = useCallback(() => {
    newTab(inheritedCwdForNewTab());
  }, [newTab, inheritedCwdForNewTab]);

  const openNewTabWithShell = useCallback(
    (shell: string) => {
      newTab(inheritedCwdForNewTab(), shell);
    },
    [newTab, inheritedCwdForNewTab],
  );

  const openNewPrivateTab = useCallback(() => {
    newPrivateTab(inheritedCwdForNewTab());
  }, [newPrivateTab, inheritedCwdForNewTab]);

  const openNewBlockTab = useCallback(() => {
    newBlockTab(inheritedCwdForNewTab());
  }, [newBlockTab, inheritedCwdForNewTab]);

  const launchAgentGroup = useCallback(
    (request: AgentLaunchRequest) => {
      const command = validateAgentLaunchCommand(request.command);
      if (!command.ok) return;
      const launcher = findAgentLauncher(request.agent);
      const title =
        request.instances === 1
          ? launcher.label
          : `${launcher.label} × ${request.instances}`;
      const { leafIds: agentLeafIds } = newAgentGroupTab(
        inheritedCwdForNewTab(),
        title,
        request.instances,
      );
      const hooksReady = launcher.supportsHooks
        ? invoke("agent_enable_hooks", {
            agent: request.agent,
          }).catch((error) => {
            console.warn(
              `[terax] could not enable ${request.agent} notifications:`,
              error,
            );
          })
        : Promise.resolve();

      for (const leafId of agentLeafIds) {
        void (async () => {
          await Promise.all([whenSessionReady(leafId), hooksReady]);
          if (!writeToSession(leafId, `${command.command}\r`)) {
            console.error(
              `[terax] agent terminal ${leafId} closed before launch`,
            );
          }
        })();
      }
    },
    [inheritedCwdForNewTab, newAgentGroupTab],
  );

  const sendCd = useCallback(
    (path: string) => {
      if (activeLeafId === null) return;
      const term = terminalRefs.current.get(activeLeafId);
      if (!term) return;
      void term.writeAtPrompt(`cd ${quoteShellArg(path)}\r`).then((typed) => {
        if (typed) {
          term.focus();
          return;
        }
        toast("Terminal is busy", {
          description: "Open a new tab to go there, or exit the running program.",
        });
      });
    },
    [activeLeafId],
  );

  const cdInNewTab = useCallback(
    (path: string) => {
      const tabId = newTab(path);
      setTimeout(() => {
        const tab = tabsRef.current.find((x) => x.id === tabId);
        if (!tab || tab.kind !== "terminal") return;
        const t = terminalRefs.current.get(tab.activeLeafId);
        if (!t) return;
        t.write(`cd ${quoteShellArg(path)}\r`);
        t.focus();
      }, 80);
    },
    [newTab],
  );

  const [pendingMarkdownFragment, setPendingMarkdownFragment] = useState<{
    path: string;
    fragment: string;
  } | null>(null);
  const handleOpenFile = useCallback(
    (path: string, pin?: boolean, fragment?: string) => {
      // Markdown opens in its rendered view by default; a per-tab toggle flips
      // it to the raw editor. Other files default to preview (pin=false);
      // explicit actions like context-menu "Open" pass pin=true to persist.
      if (isMarkdownPath(path)) {
        const normalizedPath = path.replace(/\\/g, "/");
        setPendingMarkdownFragment(
          fragment ? { path: normalizedPath, fragment } : null,
        );
        newMarkdownTab(path);
      } else openFileTab(path, pin ?? false);
    },
    [openFileTab, newMarkdownTab],
  );
  const consumePendingMarkdownFragment = useCallback(
    (path: string, fragment: string) => {
      setPendingMarkdownFragment((pending) =>
        pending?.path === path && pending.fragment === fragment ? null : pending,
      );
    },
    [],
  );

  const openLaunchFiles = useCallback(
    (paths: string[]) => {
      for (const path of paths) handleOpenFile(path, true);
    },
    [handleOpenFile],
  );

  // Warm start: the backend emits once the window already exists. Attach on
  // mount so an "Open With" that lands mid-restore isn't dropped — the backend
  // also seeds the drain-once state, so the boot drain below is the safety net.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let disposed = false;
    (async () => {
      const off = await listen<string[]>("terax:open-file", (e) => {
        openLaunchFiles(e.payload);
      });
      if (disposed) off();
      else unlisten = off;
    })();
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [openLaunchFiles]);

  // Cold start: files arrive as CLI args (Linux/Windows) or the macOS open-files
  // event, and get_launch_files drains them once. Wait for `booted` — the spaces
  // restore ends in replaceTabs(), which overwrites the whole tab list and would
  // discard a launch tab opened before it, making the file flash open and vanish.
  // Booting first also lands the tab in the restored active space, and lets
  // openFileTab dedupe against a session that already had the file open.
  useEffect(() => {
    if (!booted) return;
    void (async () => {
      openLaunchFiles(await consumeLaunchFiles());
    })();
  }, [booted, openLaunchFiles]);

  const handleExplorerPathRenamed = useCallback(
    (from: string, to: string) => {
      for (const tab of tabsRef.current) {
        if (tab.kind !== "editor" && tab.kind !== "markdown") continue;
        const path = renamedPath(tab.path, from, to);
        if (path === null) continue;
        const i = path.lastIndexOf("/");
        updateTab(tab.id, {
          path,
          title: i === -1 ? path : path.slice(i + 1),
        });
      }
    },
    [updateTab],
  );

  const canReplaceExplorerPath = useCallback(
    (path: string) => !hasOpenPathTab(tabsRef.current, path),
    [],
  );

  const activeTerminalLeafCwd =
    activeTab?.kind === "terminal"
      ? (findLeafCwd(activeTab.paneTree, activeTab.activeLeafId) ??
        activeTab.cwd ??
        null)
      : null;

  const activeFilePath = (() => {
    if (activeTab?.kind === "editor") return activeTab.path;
    if (activeTab?.kind === "git-diff") {
      if (/^([A-Za-z]:|\/|\\)/.test(activeTab.path)) return activeTab.path;
      const root = activeTab.repoRoot.replace(/[\\/]+$/, "");
      const rel = activeTab.path.replace(/^[\\/]+/, "");
      return `${root}/${rel}`;
    }
    if (activeTab?.kind === "git-commit-file") {
      const root = activeTab.repoRoot.replace(/[\\/]+$/, "");
      const rel = activeTab.path.replace(/^[\\/]+/, "");
      return `${root}/${rel}`;
    }
    return null;
  })();
  const explorerActiveFilePath =
    activeTab?.kind === "editor" || activeTab?.kind === "markdown"
      ? activeTab.path
      : null;
  const isRepositoryContextCurrent = useCallback(
    (spaceId: string, workspaceKey: string) => {
      const currentSpaceId = useSpaces.getState().activeId ?? DEFAULT_SPACE_ID;
      const currentWorkspaceKey = workspaceScopeKey(
        useWorkspaceEnvStore.getState().env,
      );
      return spaceId === currentSpaceId && workspaceKey === currentWorkspaceKey;
    },
    [],
  );
  const openSourceControl = useCallback(() => {
    openSidebarView("source-control");
  }, [openSidebarView]);
  const toggleHiddenFiles = useCallback(() => {
    openSidebarView("explorer");
    void setShowHidden(!usePreferencesStore.getState().showHidden);
  }, [openSidebarView]);
  const {
    repositoryTarget: sourceControlRepositoryTarget,
    openInSourceControl: handleOpenRepositoryInSourceControl,
    openGitHistory: handleOpenGitHistoryForPath,
    followActiveContext: handleFollowRepositoryContext,
  } = useRepositoryTargeting({
    spaceId: sourceControlSpaceId,
    workspaceKey: workspaceScopeKey(workspaceEnv),
    isContextCurrent: isRepositoryContextCurrent,
    openSourceControl,
    openCommitHistoryTab,
  });
  const explorerGitDecorations = usePreferencesStore(
    (s) => s.explorerGitDecorations,
  );
  const { sourceControl, toggleSourceControl, openGitGraphFromContext } =
    useSourceControlContext({
      activeTab,
      tabs,
      activeTerminalLeafCwd,
      explorerRoot,
      launchCwd,
      launchCwdResolved,
      home,
      sidebarView,
      sidebarCollapsed,
      explorerGitDecorations,
      repositoryTarget: sourceControlRepositoryTarget,
      cycleSidebarView,
      openCommitHistoryTab,
    });

  const openPreviewTab = useCallback(
    (url: string) => {
      const id = newPreviewTab(url);
      // Focus the address bar if the URL is empty so the user can type.
      if (!url) {
        setTimeout(() => previewRefs.current.get(id)?.focusAddressBar(), 0);
      }
      return id;
    },
    [newPreviewTab],
  );

  const splitActivePaneInActiveTab = useCallback(
    (dir: "row" | "col") => {
      const t = tabsRef.current.find((x) => x.id === activeId);
      if (!t || t.kind !== "terminal") return;
      splitActivePane(activeId, dir);
    },
    [activeId, splitActivePane],
  );

  const livePaneBounds = useCallback((tabId: number): PaneBounds[] => {
    const tab = document.querySelector<HTMLElement>(
      `[data-terminal-tab="${tabId}"]`,
    );
    if (!tab) return [];
    return [...tab.querySelectorAll<HTMLElement>("[data-pane-leaf]")].flatMap(
      (element) => {
        const id = Number(element.dataset.paneLeaf);
        if (!Number.isFinite(id)) return [];
        const { left, right, top, bottom } = element.getBoundingClientRect();
        return [{ id, left, right, top, bottom }];
      },
    );
  }, []);

  const swapActivePane = useCallback(
    (direction: "left" | "right" | "up" | "down") => {
      swapActivePaneInDirection(activeId, direction, livePaneBounds(activeId));
    },
    [activeId, livePaneBounds, swapActivePaneInDirection],
  );

  const handleCloseTabOrPane = useCallback(() => {
    const t = tabsRef.current.find((x) => x.id === activeId);
    if (t?.kind === "terminal" && leafIds(t.paneTree).length > 1) {
      closeActivePane(activeId);
      return;
    }
    void handleClose(activeId);
  }, [activeId, closeActivePane, handleClose]);

  const [zenMode, setZenMode] = useState(false);

  // Focus an agent's tab, switching to its space first so the header and tab
  // strip don't end up showing a different space than the focused pane.
  const activateAgentTarget = useCallback(
    (tabId: number, leafId: number) => {
      const space = tabsRef.current.find((t) => t.id === tabId)?.spaceId;
      if (space && space !== useSpaces.getState().activeId) {
        useSpaces.getState().setActive(space);
      }
      setActiveId(tabId);
      focusPane(tabId, leafId);
    },
    [setActiveId, focusPane],
  );

  const shortcutHandlers = useMemo<ShortcutHandlers>(
    () => ({
      "commandPalette.open": () => openCommandPalette("commands"),
      "commandPalette.files": () => openCommandPalette("files"),
      "commandPalette.content": () => openCommandPalette("content"),
      "tab.new": openNewTab,
      "tab.newBlock": openNewBlockTab,
      "tab.newPrivate": openNewPrivateTab,
      "tab.newPreview": () => openPreviewTab(""),
      "tab.newEditor": () => setNewEditorOpen(true),
      "tab.close": handleCloseTabOrPane,
      "tab.next": () => stepSwitcher(1),
      "tab.prev": () => stepSwitcher(-1),
      "tab.selectByIndex": (e) =>
        selectByIndex(
          (tabDigit(e, e.shiftKey) ?? 1) - 1,
          activeSpaceId ?? DEFAULT_SPACE_ID,
        ),
      "space.next": () => cycleSpace(1),
      "space.prev": () => cycleSpace(-1),
      "space.overview": () => setSwitcherOpen(true),
      "pane.splitRight": () => splitActivePaneInActiveTab("row"),
      "pane.splitDown": () => splitActivePaneInActiveTab("col"),
      "pane.focusNext": () => focusNextPaneInTab(activeId, 1),
      "pane.focusPrev": () => focusNextPaneInTab(activeId, -1),
      "pane.swapLeft": () => swapActivePane("left"),
      "pane.swapRight": () => swapActivePane("right"),
      "pane.swapUp": () => swapActivePane("up"),
      "pane.swapDown": () => swapActivePane("down"),
      "pane.source": toggleSourceControl,
      "terminal.clear": () => {
        clearFocusedTerminal();
      },
      "terminal.scrollToBottom": () => {
        scrollFocusedTerminalToBottom();
      },
      "terminal.toggleInput": () =>
        window.dispatchEvent(new CustomEvent(TOGGLE_BLOCK_INPUT_EVENT)),
      "blocks.prev": () => navigateFocusedBlocks(-1),
      "blocks.next": () => navigateFocusedBlocks(1),
      "search.focus": () => {
        const editor = editorRefs.current.get(activeId);
        if (editor) editor.openSearch();
        else if (
          tabsRef.current.find((t) => t.id === activeId)?.kind === "markdown"
        )
          window.dispatchEvent(new CustomEvent(MARKDOWN_FIND_EVENT));
        else searchInlineRef.current?.focus();
      },
      "ai.toggle": togglePanelAndFocus,
      "ai.toggleMini": () => {
        if (!hasComposer) {
          void openSettingsWindow("models");
          return;
        }
        toggleMini();
      },
      "ai.askSelection": onAskFromSelection,
      "agent.focusAttention": () => {
        const t = nextAttentionTarget();
        if (t) activateAgentTarget(t.tabId, t.leafId);
      },
      "settings.open": () => void openSettingsWindow(),
      "sidebar.toggle": toggleSidebar,
      "explorer.focus": toggleExplorerFocus,
      "explorer.toggleHidden": toggleHiddenFiles,
      "view.zoomIn": zoomIn,
      "view.zoomOut": zoomOut,
      "view.zoomReset": zoomReset,
      "view.zenMode": () => setZenMode((v) => !v),
      "editor.undo": () => editorRefs.current.get(activeId)?.undo(),
      "editor.redo": () => editorRefs.current.get(activeId)?.redo(),
      "editor.save": () => {
        void editorRefs.current.get(activeId)?.save();
      },
      "editor.aiComplete": () =>
        editorRefs.current.get(activeId)?.triggerAiComplete(),
      "editor.codeComplete": () =>
        editorRefs.current.get(activeId)?.triggerCodeComplete(),
    }),
    [
      activeId,
      openCommandPalette,
      stepSwitcher,
      cycleSpace,
      handleCloseTabOrPane,
      openNewTab,
      openNewBlockTab,
      openNewPrivateTab,
      openPreviewTab,
      activeSpaceId,
      selectByIndex,
      splitActivePaneInActiveTab,
      focusNextPaneInTab,
      swapActivePane,
      toggleSourceControl,
      hasComposer,
      togglePanelAndFocus,
      toggleMini,
      onAskFromSelection,
      toggleSidebar,
      toggleExplorerFocus,
      toggleHiddenFiles,
      zoomIn,
      zoomOut,
      zoomReset,
      activateAgentTarget,
    ],
  );

  const shortcutsDisabled = useCallback(
    (id: ShortcutId, e: KeyboardEvent) => {
      const terminalPaneCount =
        activeTab?.kind === "terminal"
          ? leafIds(activeTab.paneTree).length
          : null;
      if (shouldDisablePaneSwapShortcut(id, terminalPaneCount)) return true;
      if (
        id === "editor.undo" ||
        id === "editor.redo" ||
        id === "editor.save" ||
        id === "editor.aiComplete" ||
        id === "editor.codeComplete"
      ) {
        return activeTab?.kind !== "editor";
      }
      if (id === "ai.askSelection") {
        const target =
          (e.target as HTMLElement | null) ?? document.activeElement;
        return shouldDisableAskSelection(
          e,
          () => isTerminalSurfaceTarget(target),
          () => !!captureActiveSelection()?.trim(),
        );
      }
      if (id === "terminal.clear" || id === "terminal.scrollToBottom") {
        // Only intercept the chord while a terminal is focused; elsewhere let
        // it fall through (we never preventDefault when disabled).
        const target =
          (e.target as HTMLElement | null) ?? document.activeElement;
        return !isTerminalSurfaceTarget(target);
      }
      if (
        id === "terminal.toggleInput" ||
        id === "blocks.prev" ||
        id === "blocks.next"
      ) {
        return !(activeTab?.kind === "terminal" && activeTab.blocks === true);
      }
      if (id === "sidebar.toggle") {
        // Ctrl+B is also Claude Code's "run in background" key. While a terminal
        // is focused, let Ctrl+B reach the shell/Claude instead of toggling the
        // sidebar. Ctrl+Shift+B (second binding) still toggles it from anywhere.
        const target =
          (e.target as HTMLElement | null) ?? document.activeElement;
        const inTerminal = isTerminalSurfaceTarget(target);
        // Only defer the plain (no-shift) Ctrl/⌘+B binding; the Shift variant
        // is the always-on toggle and is never claimed by the terminal.
        return inTerminal && !e.shiftKey;
      }
      return false;
    },
    [activeTab, captureActiveSelection],
  );

  useGlobalShortcuts(shortcutHandlers, { isDisabled: shortcutsDisabled });

  const registerTerminalHandle = useCallback(
    (leafId: number, h: TerminalPaneHandle | null) => {
      if (h) terminalRefs.current.set(leafId, h);
      else terminalRefs.current.delete(leafId);
    },
    [],
  );

  const registerEditorHandle = useCallback(
    (id: number, h: EditorPaneHandle | null) => {
      if (h) {
        editorRefs.current.set(id, h);
        const pending = pendingEditorNavigation.current.get(id);
        if (pending != null) {
          pendingEditorNavigation.current.delete(id);
          if (pending.line === undefined) h.focus();
          else h.gotoLine(pending.line, { focus: pending.focus });
        }
      } else {
        editorRefs.current.delete(id);
      }
      if (id === activeId) setActiveEditorHandle(h);
    },
    [activeId],
  );

  const registerPreviewHandle = useCallback(
    (id: number, h: PreviewPaneHandle | null) => {
      if (h) previewRefs.current.set(id, h);
      else previewRefs.current.delete(id);
    },
    [],
  );

  const handlePreviewUrl = useCallback(
    (id: number, url: string) => updateTab(id, { url }),
    [updateTab],
  );

  const authorizedCwds = useRef(new Set<string>());
  const handleTerminalCwd = useCallback(
    (leafId: number, cwd: string) => {
      setLeafCwd(leafId, cwd);
      if (cwd && !authorizedCwds.current.has(cwd)) {
        authorizedCwds.current.add(cwd);
        native.workspaceAuthorize(cwd).catch(() => {
          authorizedCwds.current.delete(cwd);
        });
      }
    },
    [setLeafCwd],
  );

  const handleFocusLeaf = useCallback(
    (tabId: number, leafId: number) => focusPane(tabId, leafId),
    [focusPane],
  );

  const onActivateAgent = activateAgentTarget;

  const onActivateLocalAgent = useCallback(() => {
    openPanel();
    focusInput(null);
  }, [openPanel, focusInput]);

  const handleLeafExit = useCallback(
    (leafId: number, _code: number) => {
      const all = tabsRef.current;
      const tab = all.find(
        (t) => t.kind === "terminal" && hasLeaf(t.paneTree, leafId),
      );
      if (!tab || tab.kind !== "terminal") return;
      // Last pane of the last tab: quit instead of respawning a shell.
      if (leafIds(tab.paneTree).length === 1 && all.length === 1) {
        void getCurrentWindow().close();
      } else {
        closePaneByLeaf(leafId);
      }
    },
    [closePaneByLeaf],
  );

  const handleEditorDirty = useCallback(
    (id: number, dirty: boolean) => updateTab(id, { dirty }),
    [updateTab],
  );

  const handleRenameTab = useCallback(
    (id: number, title: string) => updateTab(id, { customTitle: title.trim() }),
    [updateTab],
  );

  const searchTarget = useMemo<SearchTarget>(() => {
    if (isTerminalTab && activeLeafId !== null && activeSearchAddon)
      return {
        kind: "terminal",
        addon: activeSearchAddon,
        focus: () => terminalRefs.current.get(activeLeafId)?.focus(),
      };
    if (isEditorTab && activeEditorHandle)
      return {
        kind: "editor",
        handle: activeEditorHandle,
        focus: () => activeEditorHandle.focus(),
      };
    if (isGitHistoryTab && gitHistoryHandle)
      return {
        kind: "git-history",
        handle: gitHistoryHandle,
        focus: () => {},
      };
    return null;
  }, [
    isTerminalTab,
    isEditorTab,
    isGitHistoryTab,
    activeLeafId,
    activeSearchAddon,
    activeEditorHandle,
    gitHistoryHandle,
  ]);

  const activeCwd = activeTerminalLeafCwd;

  const handleNewSpace = useCallback(() => {
    // A space window is dedicated to one space; new spaces start in main.
    if (isSpaceWindow(windowLabel)) return null;
    const { spaces, create, setActive } = useSpaces.getState();
    const meta = create({
      name: `Space ${spaces.length + 1}`,
      root: activeCwd ?? home ?? null,
      env: workspaceEnv,
    });
    setActiveSpaceForNewTabs(meta.id);
    newTab(activeCwd ?? undefined);
    setActive(meta.id);
    return meta.id;
  }, [
    windowLabel,
    activeCwd,
    home,
    workspaceEnv,
    newTab,
    setActiveSpaceForNewTabs,
  ]);

  const handleOpenFolder = useCallback(
    async (targetPath?: string) => {
      const chosen =
        targetPath ??
        (await invoke<string | null>("workspace_pick_folder", {
          defaultPath: activeSpaceRoot ?? activeCwd ?? launchCwd ?? home ?? null,
        }));
      if (!chosen) return;

      await native.workspaceAuthorize(chosen);
      const state = useSpaces.getState();
      const currentSpace = state.spaces.find((s) => s.id === state.activeId);
      const folderName =
        chosen.split(/[\\/]/).filter(Boolean).pop() || chosen;

      if (currentSpace) {
        state.setRoot(currentSpace.id, chosen);
        if (/^(Space \d+|Default)$/.test(currentSpace.name)) {
          state.rename(currentSpace.id, folderName);
        }
      }
      if (
        activeTab?.kind !== "terminal" ||
        activeTab.cwd !== chosen
      ) {
        newTab(chosen);
      }
    },
    [activeSpaceRoot, activeCwd, launchCwd, home, activeTab, newTab],
  );

  const handleOpenFolderAsSpace = useCallback(
    async (targetPath?: string) => {
      if (isSpaceWindow(windowLabel)) return null;
      const chosen =
        targetPath ??
        (await invoke<string | null>("workspace_pick_folder", {
          defaultPath: activeSpaceRoot ?? activeCwd ?? launchCwd ?? home ?? null,
        }));
      if (!chosen) return null;

      await native.workspaceAuthorize(chosen);
      const folderName =
        chosen.split(/[\\/]/).filter(Boolean).pop() || chosen;
      const { create, setActive } = useSpaces.getState();
      const meta = create({
        name: folderName,
        root: chosen,
        env: workspaceEnv,
      });
      setActiveSpaceForNewTabs(meta.id);
      newTab(chosen);
      setActive(meta.id);
      return meta.id;
    },
    [
      windowLabel,
      activeSpaceRoot,
      activeCwd,
      launchCwd,
      home,
      workspaceEnv,
      setActiveSpaceForNewTabs,
      newTab,
    ],
  );

  // A folder picked on a server opens as a new space on that host.
  useEffect(() => {
    const { setFolderHandler } = useRemoteStore.getState();
    setFolderHandler(async (host, path) => {
      if (isSpaceWindow(windowLabel)) {
        throw new Error("Open a server folder from the main window.");
      }
      const env: WorkspaceEnv = { kind: "ssh", host };
      const root = await invoke<string>("workspace_authorize", {
        path,
        workspace: env,
      });
      const { create, setActive } = useSpaces.getState();
      const meta = create({ name: `${folderName(root)} (${host})`, root, env });
      // The env first: the new tab must never start on this machine.
      useWorkspaceEnvStore.getState().setEnv(env);
      setActiveSpaceForNewTabs(meta.id);
      newTab(root);
      setActive(meta.id);
    });
    return () => setFolderHandler(null);
  }, [windowLabel, setActiveSpaceForNewTabs, newTab]);

  const handlePickFolderForSpace = useCallback(
    async (spaceId: string) => {
      const space = useSpaces.getState().spaces.find((s) => s.id === spaceId);
      const chosen = await invoke<string | null>("workspace_pick_folder", {
        defaultPath: space?.root ?? activeSpaceRoot ?? launchCwd ?? home ?? null,
      });
      if (!chosen) return;

      await native.workspaceAuthorize(chosen);
      const state = useSpaces.getState();
      state.setRoot(spaceId, chosen);
      const folderName =
        chosen.split(/[\\/]/).filter(Boolean).pop() || chosen;
      if (space && /^(Space \d+|Default)$/.test(space.name)) {
        state.rename(spaceId, folderName);
      }
    },
    [activeSpaceRoot, launchCwd, home],
  );

  const handleDeleteSpace = useCallback(
    (id: string) => {
      const nextSpaceId = useSpaces.getState().remove(id);
      if (!nextSpaceId) return;
      const root = useSpaces
        .getState()
        .spaces.find((s) => s.id === nextSpaceId)?.root;
      removeTabsForSpace(id, nextSpaceId, root ?? undefined);
    },
    [removeTabsForSpace],
  );

  const handleOpenSpaceInWindow = useCallback(
    (spaceId: string) => {
      const state = useSpaces.getState();
      const space = state.spaces.find((s) => s.id === spaceId);
      if (!space) return;
      const plan = planSpaceHandoff(
        {
          spaceId,
          hostWindowLabel: space.hostWindowLabel,
        },
        windowLabel,
      );
      if (!plan) return;

      if (plan.kind === "focus") {
        void invoke("focus_space_window", { spaceId }).catch((e) => {
          console.error("[terax] focus space window failed:", e);
        });
        return;
      }

      // Tabs that cannot travel losslessly (private terminals, AI diffs,
      // unsaved editors) stay in this window under another space it hosts.
      const fallbackSpaceId = pickHandoffFallback(
        state.spaces,
        spaceId,
        windowLabel,
      );
      const unmovable = tabsRef.current.filter(
        (t) => t.spaceId === spaceId && !isHandoffMovable(t),
      );
      if (unmovable.length > 0 && fallbackSpaceId === null) {
        toast(`Save or close ${unmovable.length} tab(s) first`, {
          description:
            "Private terminals, AI diffs and unsaved editors cannot move, and no other space in this window can keep them.",
        });
        return;
      }

      const transfers = useSpaceTransfers.getState();
      if (transfers.pending[spaceId]) return;
      const toastId = spaceTransferToastId(spaceId);
      transfers.begin(spaceId, "opening");
      toast.loading(`Opening ${space.name} in a new window…`, {
        id: toastId,
        description: "Moving its tabs and terminals.",
      });

      void (async () => {
        let detachedTabs: Tab[] = [];
        let leafPtyIdsSnapshot: Array<number | null> = [];
        let rollbackReplays: PtyReplayPayload[] = [];
        let rollbackCheckpoints: PtyHandoffCheckpoint[] = [];
        const wasActive = useSpaces.getState().activeId === spaceId;
        try {
          await flushNow();
          // A previous window for this space announced readiness and was
          // closed; a new one must announce again before we emit to it.
          if (isWindowReady(plan.to) && !(await windowExists(plan.to))) {
            forgetWindowReady(plan.to);
          }
          await invoke("open_space_window", { spaceId });
          await waitForWindowReady(plan.to);

          // Publish the new host before the target window's debounced
          // persistence can flush; otherwise it sees no hosted space and
          // overwrites the space state on disk with an empty list.
          await state.assignSpaceToWindow(spaceId, plan.to);

          // Plan, serialize and detach from the same tabs in the same tick so
          // the leaf -> PTY order always matches what is sent.
          const { ptyIds, leafPtyIds, detached, kept } = detachTabsForHandoff(
            spaceId,
            { isMovable: isHandoffMovable, fallbackSpaceId },
          );
          detachedTabs = detached;
          leafPtyIdsSnapshot = leafPtyIds;

          const result = await moveTabsAcrossWindows({
            handoffId: nextHandoffId(),
            tabs: serializeTabsForHandoff(detached),
            ptyIds,
            leafPtyIds,
            targetWindow: plan.to,
            spaceId,
            fromWindow: windowLabel,
            invoke,
            emitTo,
            waitForAttach: createAttachWaiter({ listen: tauriAttachListen })
              .waitForAttach,
            onRollback: (replays, checkpoints) => {
              rollbackReplays = replays;
              rollbackCheckpoints = checkpoints;
            },
          });
          if (!result.ok) throw new Error(result.error);
          await state.assignSpaceToWindow(spaceId, plan.to);
          toast.success(`${space.name} opened in its own window`, {
            id: toastId,
            description: undefined,
          });

          // This window no longer shows the moved space; land on the space
          // that kept the leftovers (or any other one this window hosts).
          if (wasActive && fallbackSpaceId) {
            useSpaces.getState().setActive(fallbackSpaceId);
            if (!tabsRef.current.some((t) => t.spaceId === fallbackSpaceId)) {
              const root = useSpaces
                .getState()
                .spaces.find((s) => s.id === fallbackSpaceId)?.root;
              newTabInSpace(fallbackSpaceId, root ?? undefined);
            }
          }
          if (kept.length > 0 && fallbackSpaceId) {
            const name =
              useSpaces
                .getState()
                .spaces.find((s) => s.id === fallbackSpaceId)?.name ??
              "another space";
            toast(`${kept.length} tab(s) stayed in ${name}`, {
              description:
                "Private terminals, AI diffs and unsaved editors cannot move between windows.",
            });
          }
        } catch (e) {
          console.error("[terax] open space window failed:", e);
          try {
            await state.assignSpaceToWindow(spaceId, plan.from);
            if (detachedTabs.length > 0) {
              const restored = restoreHandoffSource(
                detachedTabs,
                spaceId,
                rollbackReplays,
                rollbackCheckpoints,
                leafPtyIdsSnapshot,
                allocId,
              );
              attachTabsForSpace(spaceId, restored);
            } else {
              // Nothing moved (the new window never became ready). Close the
              // empty window instead of leaving a dead frame behind.
              forgetWindowReady(plan.to);
              const { WebviewWindow } = await import(
                "@tauri-apps/api/webviewWindow"
              );
              await (await WebviewWindow.getByLabel(plan.to))?.destroy();
            }
          } catch (rollbackError) {
            console.error(
              "[terax] open space window rollback failed:",
              rollbackError,
            );
          }
          toast.error("Could not open the space in a new window", {
            id: toastId,
            description: e instanceof Error ? e.message : String(e),
          });
        } finally {
          useSpaceTransfers.getState().end(spaceId);
        }
      })();
    },
    [
      windowLabel,
      flushNow,
      detachTabsForHandoff,
      attachTabsForSpace,
      newTabInSpace,
      allocId,
    ],
  );

  useEffect(() => {
    if (!spacesHydrated) return;
    let disposed = false;
    const unlisteners: Array<() => void> = [];

    const refreshRegistry = async () => {
      try {
        const { loadMergedRegistry } = await import(
          "@/modules/spaces/lib/crossWindowSync"
        );
        const spaces = await loadMergedRegistry(useSpaces.getState().spaces);
        if (disposed) return;
        useSpaces.setState((s) => ({ spaces, hydrated: s.hydrated }));
      } catch (e) {
        console.error("[terax] spaces registry sync failed:", e);
      }
    };

    void (async () => {
      const {
        HOST_CHANGED_REASONS,
        SPACE_DELETED_EVENT,
        SPACE_META_CHANGED_EVENT,
        SPACE_HOST_CHANGED_EVENT,
        SPACE_TABS_MOVED_EVENT,
        applyHostChanged,
        applyDeletedSpace,
      } = await import("@/modules/spaces/lib/crossWindowSync");
      const subs = await Promise.all([
        listen(SPACE_META_CHANGED_EVENT, () => void refreshRegistry()),
        listen<HostChangedPayload>(
          SPACE_HOST_CHANGED_EVENT,
          (event) => {
            const payload = event.payload;
            if (!payload?.spaceId || !payload.hostWindowLabel) {
              void refreshRegistry();
              return;
            }
            if (payload.reason === HOST_CHANGED_REASONS.windowClosed) {
              // The window is gone: a reopened window for this space reuses the
              // label and must announce itself again before a handoff targets it.
              forgetWindowReady(spaceWindowLabel(payload.spaceId));
            }
            void (async () => {
              try {
                // A live handoff is persisted by its source window before the
                // broadcast, so only a closed host needs the on-disk registry
                // and tab state. Reading it on every broadcast would also make
                // several windows race to rewrite the same file.
                const restoreFromDisk =
                  payload.reason === HOST_CHANGED_REASONS.windowClosed;
                const spacesStore = restoreFromDisk
                  ? await import("@/modules/spaces/lib/store")
                  : null;
                const loaded = spacesStore ? await spacesStore.loadAll() : null;
                const currentSpaces =
                  loaded?.spaces ?? useSpaces.getState().spaces;
                const prev = currentSpaces.find(
                  (s) => s.id === payload.spaceId,
                );
                const merged = applyHostChanged(currentSpaces, payload);
                if (spacesStore && payload.hostWindowLabel === windowLabel) {
                  await spacesStore.saveSpacesList(merged);
                }
                if (disposed) return;
                if (merged !== currentSpaces) {
                  useSpaces.setState((s) => ({
                    spaces: merged,
                    hydrated: s.hydrated,
                  }));
                }

                const movedToThisWindow = payload.hostWindowLabel === windowLabel;
                const cameFromElsewhere =
                  (prev?.hostWindowLabel ?? "main") !== windowLabel;
                if (movedToThisWindow && cameFromElsewhere && !disposed) {
                  useSpaces.getState().setActive(payload.spaceId);
                  // A closed window cannot run a tabs-moved handoff, so this
                  // is the only case where the receiving window restores from
                  // disk. Live handoffs restore through SPACE_TABS_MOVED_EVENT
                  // alone; doing it here too would append duplicate tabs and
                  // attach PTYs that were detached for the handoff.
                  if (!restoreFromDisk) return;
                  const state = loaded?.states.get(payload.spaceId);
                  if (!state?.tabs?.length) return;
                  const { hydrateTabs } = await import(
                    "@/modules/spaces/lib/serialize"
                  );
                  const hydrated = hydrateTabs(
                    state.tabs,
                    payload.spaceId,
                    allocId,
                  );
                  if (!hydrated.length || disposed) return;
                  attachTabsForSpace(payload.spaceId, hydrated);
                  const idx = Math.min(
                    Math.max(0, state.activeTabIndex ?? 0),
                    hydrated.length - 1,
                  );
                  setActiveId(hydrated[idx]?.id ?? hydrated[0].id);
                }
              } catch (e) {
                console.error("[terax] spaces host sync failed:", e);
                void refreshRegistry();
              }
            })();
          },
        ),
        listen<{ spaceId: string; toWindow: string; handoffId?: string }>(
          SPACE_RECLAIM_REQUEST_EVENT,
          (event) => {
            const { spaceId, toWindow, handoffId } = event.payload ?? {};
            if (!spaceId || !toWindow || windowLabel === toWindow) return;
            // Only the current host answers; other windows ignore the request.
            if (
              windowLabel !== spaceWindowLabel(spaceId) &&
              windowLabel !== "main"
            ) {
              return;
            }
            void (async () => {
              try {
                await flushNow();
                const fallbackSpaceId = pickHandoffFallback(
                  useSpaces.getState().spaces,
                  spaceId,
                  windowLabel,
                );
                const unmovable = tabsRef.current.filter(
                  (t) => t.spaceId === spaceId && !isHandoffMovable(t),
                );
                if (unmovable.length > 0 && fallbackSpaceId === null) {
                  // This window closes after a reclaim, so leftovers would be
                  // lost. Keep everything here and ask the user instead.
                  toast(`Save or close ${unmovable.length} tab(s) first`, {
                    description:
                      "Private terminals, AI diffs and unsaved editors cannot move back to the main window.",
                  });
                  await getCurrentWindow().setFocus();
                  return;
                }
                const { ptyIds, leafPtyIds, detached } = detachTabsForHandoff(
                  spaceId,
                  { isMovable: isHandoffMovable, fallbackSpaceId },
                );
                const snapshot = detached;
                let rollbackReplays: PtyReplayPayload[] = [];
                let rollbackCheckpoints: PtyHandoffCheckpoint[] = [];
                const result = await moveTabsAcrossWindows({
                  handoffId: handoffId ?? nextHandoffId(),
                  tabs: serializeTabsForHandoff(detached),
                  ptyIds,
                  leafPtyIds,
                  targetWindow: toWindow,
                  spaceId,
                  fromWindow: windowLabel,
                  invoke,
                  emitTo,
                  waitForAttach: createAttachWaiter({
                    listen: tauriAttachListen,
                  }).waitForAttach,
                  onRollback: (replays, checkpoints) => {
                    rollbackReplays = replays;
                    rollbackCheckpoints = checkpoints;
                  },
                });
                if (!result.ok) {
                  const restored = restoreHandoffSource(
                    snapshot,
                    spaceId,
                    rollbackReplays,
                    rollbackCheckpoints,
                    leafPtyIds,
                    allocId,
                  );
                  attachTabsForSpace(spaceId, restored);
                  throw new Error(result.error);
                }
                await useSpaces
                  .getState()
                  .assignSpaceToWindow(spaceId, toWindow);
                if (windowLabel !== "main") await getCurrentWindow().destroy();
              } catch (e) {
                console.error("[terax] reclaim handoff failed:", e);
                void refreshRegistry();
              }
            })();
          },
        ),
        listen<TabsMovedPayload>(SPACE_TABS_MOVED_EVENT, (event) => {
          const payload = event.payload;
          if (!payload || payload.toWindow !== windowLabel) return;
          const { handoffId } = payload;
          void (async () => {
            const ack = async (
              replays: TabsAttachedPayload["replays"],
              error?: string,
            ) => {
              const body: TabsAttachedPayload = error
                ? { handoffId, replays, error }
                : { handoffId, replays };
              try {
                await emitTo(payload.fromWindow, SPACE_TABS_ATTACHED_EVENT, body);
              } catch (e) {
                console.error("[terax] handoff ack failed:", e);
              }
            };
            // Release only what *this* handoff built: the PTYs went back to
            // the source, so the views are dropped without closing them, and
            // tabs this window already had in the space are left alone.
            const releaseHandoff = () => {
              const record = incomingHandoffsRef.current.get(handoffId);
              incomingHandoffsRef.current.delete(handoffId);
              if (!record) return;
              dropHandoffReplays(record.ptyIds);
              if (record.tabIds.length === 0) return;
              const ids = new Set(record.tabIds);
              detachTabsForHandoff(payload.spaceId, {
                isMovable: (t) => ids.has(t.id),
                fallbackSpaceId: null,
                tabIds: record.tabIds,
              });
            };
            // A return this window asked for: close its spinner and toast.
            const settleReturn = (error?: string) => {
              const transfers = useSpaceTransfers.getState();
              if (transfers.pending[payload.spaceId] !== "returning") return;
              transfers.end(payload.spaceId);
              const name =
                useSpaces
                  .getState()
                  .spaces.find((s) => s.id === payload.spaceId)?.name ??
                "The space";
              const id = spaceTransferToastId(payload.spaceId);
              if (error) {
                toast.error(`Could not bring ${name} back`, {
                  id,
                  description: error,
                });
              } else {
                toast.success(`${name} is back in this window`, {
                  id,
                  description: undefined,
                });
              }
            };
            try {
              // A disposed effect only stops registering *new* listeners. An
              // event already delivered to this handler must still be answered,
              // otherwise the source times out and rolls back with no replays,
              // leaving a live PTY with no tab to host it.
              const correction = payload.tabs.length === 0;
              if (correction) {
                // The source rolled back. The original event may still be
                // mid-attach, so mark it abandoned before releasing.
                abandonedHandoffsRef.current.add(handoffId);
                releaseHandoff();
                await ack([], "source rolled back");
                settleReturn("The move was rolled back.");
                return;
              }
              const ptyIds = payload.checkpoints.map((c) => c.ptyId);
              incomingHandoffsRef.current.set(handoffId, { ptyIds, tabIds: [] });

              // Adopt the space locally before any debounced persistence runs:
              // the boot path saw this space as hosted elsewhere, so without
              // this the next flush skips its tabs and clears its saved state.
              useSpaces.setState((s) => ({
                spaces: applyHostChanged(s.spaces, {
                  spaceId: payload.spaceId,
                  hostWindowLabel: windowLabel,
                  updatedAt: Date.now(),
                }),
              }));

              // This window booted with no hosted space, so the boot path
              // returned before adopting its env. Attaching PTYs and spawning
              // tabs must see the space's workspace, not the default one.
              let meta = useSpaces
                .getState()
                .spaces.find((s) => s.id === payload.spaceId);
              if (!meta) {
                // A space created in another window after this one booted may
                // not be in the local registry yet. Fall back to disk before
                // deciding how to attach its PTYs.
                const { spaces: diskSpaces } = await import(
                  "@/modules/spaces/lib/store"
                ).then((m) => m.loadAll());
                meta = diskSpaces.find((s) => s.id === payload.spaceId);
              }
              if (payload.checkpoints.length > 0 && !meta) {
                throw new Error(
                  `Cannot attach PTYs: workspace for space ${payload.spaceId} is unknown`,
                );
              }
              if (meta) await adoptWorkspaceEnv(meta.env);

              let replays: TabsAttachedPayload["replays"] = [];
              if (ptyIds.length > 0) {
                replays =
                  (await invoke<PtyReplayPayload[]>(
                    "pty_attach",
                    buildAttachHandoff(handoffId, ptyIds),
                  )) ?? [];
                // A leaf whose replay is missing would spawn a fresh shell and
                // strand its real PTY here. Fail the move so the source rolls
                // back and keeps the session.
                const got = new Set(replays.map((r) => r.ptyId));
                const missing = ptyIds.filter((id) => !got.has(id));
                if (missing.length > 0) {
                  throw new Error(
                    `terminal(s) ${missing.join(", ")} returned no replay`,
                  );
                }
                for (const replay of replays) queueHandoffReplay(replay);
              }
              if (abandonedHandoffsRef.current.delete(handoffId)) {
                releaseHandoff();
                return;
              }

              const stamped = stampHandoffAttachments(
                payload.tabs,
                payload.checkpoints,
                payload.leafPtyIds,
              );
              const hydrated = hydrateTabs(stamped, payload.spaceId, allocId, {
                keepPty: true,
              });
              // Once a handoff event is being handled it must finish: `disposed`
              // only guards listener registration. A re-render mid-handoff (e.g.
              // adoptWorkspaceEnv changing state) must not leave the target with
              // an attached PTY and no tab hosting it.
              if (hydrated.length === 0) {
                throw new Error("no tabs could be restored from the handoff");
              }
              incomingHandoffsRef.current.set(handoffId, {
                ptyIds,
                tabIds: hydrated.map((t) => t.id),
              });
              attachTabsForSpace(payload.spaceId, hydrated);
              setActiveId(hydrated[0].id);
              useSpaces.getState().setActive(payload.spaceId);
              await ack(replays);
              settleReturn();
              // The source may still fail its commit and send a correction, so
              // keep the record briefly instead of forgetting it at once.
              setTimeout(() => {
                incomingHandoffsRef.current.delete(handoffId);
                abandonedHandoffsRef.current.delete(handoffId);
              }, 60_000);
            } catch (e) {
              console.error("[terax] cross-window tab attach failed:", e);
              releaseHandoff();
              const reason = e instanceof Error ? e.message : String(e);
              await ack([], reason);
              settleReturn(reason);
            }
          })();
        }),
        listen<WindowReadyPayload>(SPACE_WINDOW_READY_EVENT, (event) => {
          if (event.payload?.window) noteWindowReady(event.payload.window);
        }),
        listen<{ spaceId: string }>(SPACE_DELETED_EVENT, (event) => {
          const id = event.payload?.spaceId;
          if (!id) {
            void refreshRegistry();
            return;
          }
          useSpaces.setState((s) => ({
            spaces: applyDeletedSpace(s.spaces, id),
            hydrated: s.hydrated,
          }));
        }),
      ]);
      if (disposed) {
        for (const un of subs) un();
        return;
      }
      unlisteners.push(...subs);
      noteWindowReady(windowLabel);
      // Announce to every other window. The global Tauri event reaches all
      // windows except the emitter, so each peer can note our readiness.
      void emit(SPACE_WINDOW_READY_EVENT, {
        window: windowLabel,
      } satisfies WindowReadyPayload).catch((e) => {
        console.error("[terax] window-ready announce failed:", e);
      });
    })();

    return () => {
      disposed = true;
      for (const un of unlisteners) un();
    };
  }, [
    spacesHydrated,
    windowLabel,
    allocId,
    adoptWorkspaceEnv,
    attachTabsForSpace,
    detachTabsForHandoff,
    flushNow,
    setActiveId,
  ]);

  const handleMoveTab = useCallback(
    (tabId: number, targetSpaceId: string) => {
      const target = useSpaces
        .getState()
        .spaces.find((s) => s.id === targetSpaceId);
      const targetWindow = target?.hostWindowLabel ?? "main";
      if (target && targetWindow !== windowLabel) {
        const tab = tabsRef.current.find((t) => t.id === tabId);
        if (!tab) return;
        void (async () => {
          if (!isHandoffMovable(tab)) {
            toast("This tab cannot move between windows", {
              description:
                "Private terminals, AI diffs and unsaved editors stay in their window.",
            });
            return;
          }
          const serialized = serializeTabsForHandoff([tab]);
          try {
            const { ptyIds, leafPtyIds } = detachTabsForHandoff(tab.spaceId, {
              isMovable: isHandoffMovable,
              fallbackSpaceId: null,
              tabIds: [tabId],
            });
            let rollbackReplays: PtyReplayPayload[] = [];
            let rollbackCheckpoints: PtyHandoffCheckpoint[] = [];
            const result = await moveTabsAcrossWindows({
              handoffId: nextHandoffId(),
              tabs: serialized,
              ptyIds,
              leafPtyIds,
              targetWindow,
              spaceId: targetSpaceId,
              fromWindow: windowLabel,
              invoke,
              emitTo,
              waitForAttach: createAttachWaiter({ listen: tauriAttachListen })
                .waitForAttach,
              onRollback: (replays, checkpoints) => {
                rollbackReplays = replays;
                rollbackCheckpoints = checkpoints;
              },
            });
            if (!result.ok) {
              const restored = restoreHandoffSource(
                [tab],
                tab.spaceId,
                rollbackReplays,
                rollbackCheckpoints,
                leafPtyIds,
                allocId,
              );
              attachTabsForSpace(tab.spaceId, restored);
              throw new Error(result.error);
            }
          } catch (e) {
            console.error("[terax] cross-window tab move failed:", e);
            toast("Could not move the tab", {
              description: "Its window may have closed.",
            });
            return;
          }
        })();
        return;
      }
      if (moveTabToSpace(tabId, targetSpaceId)) {
        useSpaces.getState().setActive(targetSpaceId);
      }
    },
    [
      moveTabToSpace,
      windowLabel,
      detachTabsForHandoff,
      attachTabsForSpace,
      allocId,
    ],
  );

  const handleBringSpaceToWindow = useCallback(
    (spaceId: string) => {
      const space = useSpaces
        .getState()
        .spaces.find((s) => s.id === spaceId);
      const host = reclaimHost(space, windowLabel);
      if (!host || !space) return;
      const transfers = useSpaceTransfers.getState();
      if (transfers.pending[spaceId]) return;
      const toastId = spaceTransferToastId(spaceId);
      // The other window does the move; this one hears back when the tabs
      // arrive (SPACE_TABS_MOVED_EVENT), or gives up.
      transfers.begin(spaceId, "returning", () => {
        toast.error(`${space.name} did not come back`, {
          id: toastId,
          description:
            "Its window did not answer. It may be asking you to save or close tabs first.",
        });
      });
      toast.loading(`Bringing ${space.name} back…`, {
        id: toastId,
        description: "Moving its tabs and terminals to this window.",
      });
      void (async () => {
        try {
          await waitForWindowReady(host);
          await emitTo(host, SPACE_RECLAIM_REQUEST_EVENT, {
            spaceId,
            toWindow: windowLabel,
            handoffId: nextHandoffId(),
          });
        } catch (e) {
          console.error("[terax] reclaim request failed:", e);
          useSpaceTransfers.getState().end(spaceId);
          toast.error("Could not bring the space back", {
            id: toastId,
            description: "Its window may have closed.",
          });
        }
      })();
    },
    [windowLabel],
  );

  const handleReorderTab = useCallback(
    (tabId: number, targetTabId: number, edge: "top" | "bottom") => {
      if (reorderTab(tabId, targetTabId, edge)) {
        const target = tabsRef.current.find((x) => x.id === targetTabId);
        if (target) useSpaces.getState().setActive(target.spaceId);
      }
    },
    [reorderTab],
  );

  const handleNewTabInSpace = useCallback(
    (spaceId: string) => {
      const root = useSpaces
        .getState()
        .spaces.find((s) => s.id === spaceId)?.root;
      newTabInSpace(spaceId, root ?? undefined);
    },
    [newTabInSpace],
  );

  const jumpToTab = useCallback(
    (tabId: number) => {
      const t = tabsRef.current.find((x) => x.id === tabId);
      if (!t) return;
      setActiveId(tabId);
      useSpaces.getState().setActive(t.spaceId);
      setSwitcherOpen(false);
    },
    [setActiveId],
  );

  const spaceSwitcher = (
    <SpaceSwitcher
      open={switcherOpen}
      onOpenChange={setSwitcherOpen}
      tabs={tabs}
      onNewSpace={() => void handleNewSpace()}
      onOpenFolderAsSpace={() => void handleOpenFolderAsSpace()}
      onPickFolderForSpace={(id) => void handlePickFolderForSpace(id)}
      onDeleteSpace={handleDeleteSpace}
      onOpenSpaceInWindow={handleOpenSpaceInWindow}
      onBringSpaceToWindow={handleBringSpaceToWindow}
      onNewTabInSpace={handleNewTabInSpace}
      onJumpTab={jumpToTab}
      onCloseTab={handleClose}
      onMoveTabToSpace={handleMoveTab}
      onReorderTab={handleReorderTab}
      onReorderSpaces={(ids) => useSpaces.getState().reorder(ids)}
    />
  );

  const commandPaletteItems = useMemo(
    () =>
      commandPaletteOpen
        ? createCommandItems({
            tabs,
            activeId,
            searchTarget,
            explorerRoot,
            home,
            openNewTab,
            openNewBlock: openNewBlockTab,
            openNewPrivate: openNewPrivateTab,
            openNewEditor: () => setNewEditorOpen(true),
            openNewPreview: () => openPreviewTab(""),
            openGitGraph: openGitGraphFromContext,
            toggleSourceControl,
            closeActiveTabOrPane: handleCloseTabOrPane,
            splitPaneRight: () => splitActivePaneInActiveTab("row"),
            splitPaneDown: () => splitActivePaneInActiveTab("col"),
            focusSearch: () => searchInlineRef.current?.focus(),
            focusExplorerSearch: () => explorerRef.current?.focusSearch(),
            toggleSidebar,
            toggleHiddenFiles,
            toggleAi: togglePanelAndFocus,
            askAiSelection: askFromSelection,
            openSettings: () => void openSettingsWindow(),
            openKeyboardShortcuts: () => void openSettingsWindow("shortcuts"),
            spaces: useSpaces.getState().spaces,
            activeSpaceId,
            windowLabel,
            openSpacesOverview: () => setSwitcherOpen(true),
            newSpace: () => void handleNewSpace(),
            openFolder: () => void handleOpenFolder(),
            openFolderAsSpace: () => void handleOpenFolderAsSpace(),
            switchSpace: (id) => useSpaces.getState().setActive(id),
          })
        : [],
    [
      commandPaletteOpen,
      tabs,
      activeId,
      searchTarget,
      explorerRoot,
      home,
      openNewTab,
      openNewBlockTab,
      openNewPrivateTab,
      openPreviewTab,
      openGitGraphFromContext,
      toggleSourceControl,
      handleCloseTabOrPane,
      splitActivePaneInActiveTab,
      toggleSidebar,
      toggleHiddenFiles,
      togglePanelAndFocus,
      askFromSelection,
      activeSpaceId,
      handleNewSpace,
      handleOpenFolder,
      handleOpenFolderAsSpace,
      windowLabel,
    ],
  );

  const pendingEditorNavigation = useRef<
    Map<number, { line?: number; focus: boolean }>
  >(new Map());
  const openContentHit = useCallback(
    (path: string, line: number) => {
      const id = openFileTab(path, true);
      if (id == null) return;
      const h = editorRefs.current.get(id);
      if (h) h.gotoLine(line);
      else pendingEditorNavigation.current.set(id, { line, focus: true });
    },
    [openFileTab],
  );

  // A preview tab that switches to its source opens the editor at the line
  // that was being read.
  const handleSetMarkdownView = useCallback(
    (id: number, mode: MarkdownViewMode, line?: number) => {
      if (line !== undefined && mode !== "rendered") {
        const handle = editorRefs.current.get(id);
        if (handle) handle.gotoLine(line);
        else pendingEditorNavigation.current.set(id, { line, focus: true });
      }
      setMarkdownView(id, mode);
    },
    [setMarkdownView],
  );

  const openLinkedFile = useCallback(
    (path: string, pin?: boolean, fragment?: string) =>
      handleOpenFile(path, pin ?? true, fragment),
    [handleOpenFile],
  );

  // Open beside the active tab without stealing it: the file shows on the
  // right of a split view and the active tab stays on the left.
  const openFileToSide = useCallback(
    (path: string, line?: number) => {
      const existed = tabsRef.current.some(
        (t) =>
          t.kind === "editor" &&
          t.path === path &&
          t.spaceId === activeSpaceIdRef.current,
      );
      const id = openFileTab(path, true, { activate: false });
      // A file that is already the active tab moves right; its neighbour
      // takes the left.
      if (id === activeIdRef.current) splitTabRight(id);
      else setSecondaryTabId(id);
      // A markdown file opened normally shows rendered; keep that beside too,
      // unless it already had a tab with a layout of its own.
      if (!existed && isMarkdownPath(path)) setMarkdownView(id, "rendered");
      if (line !== undefined) {
        const h = editorRefs.current.get(id);
        if (h) h.gotoLine(line, { focus: false });
        else pendingEditorNavigation.current.set(id, { line, focus: false });
      }
    },
    [openFileTab, setMarkdownView, splitTabRight],
  );

  const openPaletteFile = useCallback(
    (path: string, target: OpenFileTarget) => {
      if (target.side) {
        openFileToSide(path, target.line);
        return;
      }
      if (target.line !== undefined) openContentHit(path, target.line);
      else handleOpenFile(path, true);
    },
    [openContentHit, handleOpenFile, openFileToSide],
  );

  const openControlFile = useCallback(
    ({
      path,
      line,
      focus,
      spaceId,
    }: {
      path: string;
      line?: number;
      focus: boolean;
      spaceId: string;
    }) => {
      if (focus && useSpaces.getState().activeId !== spaceId) {
        useSpaces.getState().setActive(spaceId);
      }
      const id = openFileTab(path, true, {
        spaceId,
        activate: focus,
      });
      const editor = editorRefs.current.get(id);
      if (line !== undefined) {
        if (editor) editor.gotoLine(line, { focus });
        else pendingEditorNavigation.current.set(id, { line, focus });
      } else if (focus) {
        if (editor) editor.focus();
        else pendingEditorNavigation.current.set(id, { focus: true });
      }
      return id;
    },
    [openFileTab],
  );

  useControlBridge({
    ready: spacesHydrated && launchCwdResolved,
    tabsRef,
    activeTabIdRef: activeIdRef,
    activeSpaceIdRef,
    onOpen: openControlFile,
  });

  useEffect(() => {
    setLspNavigator({ openFile: openContentHit });
    return () => setLspNavigator(null);
  }, [openContentHit]);

  useEffect(() => {
    setPreviewNavigator({
      openUrl: (url) => {
        if (!isAllowedPreviewUrl(url)) return false;
        openPreviewTab(url);
        return true;
      },
    });
    return () => setPreviewNavigator(null);
  }, [openPreviewTab]);

  const insertHistoryCommand = useMemo(
    () =>
      isTerminalTab && activeLeafId !== null
        ? (cmd: string) => {
            writeToSession(activeLeafId, cmd);
            terminalRefs.current.get(activeLeafId)?.focus();
          }
        : null,
    [isTerminalTab, activeLeafId],
  );

  useAiLiveBridge({
    setLive,
    activeId,
    tabs,
    explorerRoot,
    launchCwd,
    home,
    openPreviewTab,
    newAgentTab,
    terminalRefs,
  });

  const shell = (
    <ThemeProvider>
      <TooltipProvider>
        {BenchBridge && (
          <Suspense fallback={null}>
            <BenchBridge
              tabs={tabs}
              activeId={activeId}
              booted={booted}
              setActiveId={setActiveId}
              newTab={newTab}
              newBlockTab={newBlockTab}
              newAgentGroupTab={newAgentGroupTab}
              splitActivePane={splitActivePane}
              closeTab={closeTab}
              readBuffer={(leafId, maxLines) =>
                terminalRefs.current.get(leafId)?.getBuffer(maxLines) ?? null
              }
            />
          </Suspense>
        )}
        <div className="relative flex h-screen flex-col overflow-hidden bg-frame text-foreground">
          {!zenMode && (
            <Header
              tabs={spaceTabs}
              activeId={activeId}
              onSelect={selectTab}
              splitTabId={secondaryTabId}
              onSplitRight={splitTabRight}
              onCloseSplit={closeSplit}
              onNew={openNewTab}
              onNewBlock={openNewBlockTab}
              onNewPrivate={openNewPrivateTab}
              onNewWithShell={openNewTabWithShell}
              onNewPreview={() => openPreviewTab("")}
              onNewEditor={() => setNewEditorOpen(true)}
              onNewGitGraph={openGitGraphFromContext}
              onLaunchAgents={launchAgentGroup}
              onClose={handleClose}
              onCloseTabsToRight={handleCloseTabsToRight}
              onCloseOtherTabs={handleCloseOtherTabs}
              onPin={pinTab}
              onRename={handleRenameTab}
              onReorder={reorderTabByGap}
              onToggleSidebar={toggleSidebar}
              onOpenCommandPalette={() => openCommandPalette("commands")}
              onActivateAgent={onActivateAgent}
              onActivateLocalAgent={onActivateLocalAgent}
              onOpenSettings={() => void openSettingsWindow()}
              spaceSwitcher={spaceSwitcher}
              searchTarget={searchTarget}
              searchRef={searchInlineRef}
              onOverrideLanguage={setOverrideLanguage}
            />
          )}

          <main className="zoom-content flex min-h-0 flex-1 flex-col">
            <ResizablePanelGroup
              orientation="horizontal"
              className="min-h-0 flex-1"
              onLayoutChanged={(_, { isUserInteraction }) => {
                const width = sidebarRef.current?.getSize().inPixels ?? 0;
                persistSidebarWidth(width, isUserInteraction);
              }}
            >
              <ResizablePanel
                id="sidebar"
                panelRef={sidebarRef}
                defaultSize={
                  initialSidebarCollapsed
                    ? "0px"
                    : `${sidebarWidthRef.current}px`
                }
                minSize={`${SIDEBAR_MIN_WIDTH}px`}
                maxSize={`${SIDEBAR_MAX_WIDTH}px`}
                collapsible
                collapsedSize={0}
                onResize={(size) => {
                  persistSidebarCollapsed(size.inPixels <= 0);
                }}
              >
                <div className="h-full min-h-0 pl-2 pr-0.5">
                  <div className="terax-pane flex h-full min-h-0 flex-col">
                    <div
                      key={sidebarView}
                      className="min-h-0 flex-1 terax-panel-in"
                    >
                      {sidebarView === "explorer" ? (
                        <RemoteGate env={workspaceEnv} root={explorerRoot}>
                        <FileExplorer
                          ref={explorerRef}
                          rootPath={explorerViewRoot}
                          onOpenFolder={() => void handleOpenFolder()}
                          gitStatus={
                            explorerGitDecorations ? sourceControl.status : null
                          }
                          activeFilePath={explorerActiveFilePath}
                          onOpenFile={handleOpenFile}
                          onOpenFileToSide={openFileToSide}
                          onPathRenamed={handleExplorerPathRenamed}
                          onPathsDeleted={handlePathsDeleted}
                          canReplacePath={canReplaceExplorerPath}
                          onRevealInTerminal={cdInNewTab}
                          onOpenInSourceControl={
                            handleOpenRepositoryInSourceControl
                          }
                          onOpenGitHistory={handleOpenGitHistoryForPath}
                          onAttachToAgent={handleAttachFileToAgent}
                          pathDropTarget={terminalPathDropTarget}
                          visible={!sidebarCollapsed}
                        />
                        </RemoteGate>
                      ) : sidebarView === "source-control" ? (
                        <SourceControlPanel
                          open
                          sourceControl={sourceControl}
                          onOpenDiff={openGitDiffTab}
                          onOpenGitGraph={openGitGraphFromContext}
                          onOpenFile={handleOpenFile}
                          onNavigateToPath={cdInNewTab}
                          repositoryTarget={sourceControlRepositoryTarget}
                          onFollowRepositoryContext={
                            handleFollowRepositoryContext
                          }
                        />
                      ) : (
                        <ProblemsPanel onOpenAt={openContentHit} />
                      )}
                    </div>
                    <SidebarRail
                      activeView={sidebarView}
                      onSelectView={persistSidebarView}
                      changedCount={sourceControl.changedCount}
                    />
                  </div>
                </div>
              </ResizablePanel>
              <ResizableHandle className="w-1 rounded-full bg-transparent transition-colors duration-[var(--dur-fast)] after:w-4 hover:bg-border" />
              <ResizablePanel id="workspace" defaultSize="78%" minSize="30%">
                <div className="h-full min-h-0 pl-0.5 pr-2">
                  <div className="terax-pane flex h-full min-h-0 flex-col">
                    <div className="relative min-h-0 flex-1">
                      <WorkspaceSurface
                        tabs={tabs}
                        activeId={activeId}
                        activeTab={activeTab}
                        registerTerminalHandle={registerTerminalHandle}
                        onSearchReady={handleSearchReady}
                        onCwd={handleTerminalCwd}
                        onExit={handleLeafExit}
                        onFocusLeaf={handleFocusLeaf}
                        registerEditorHandle={registerEditorHandle}
                        onEditorDirtyChange={handleEditorDirty}
                        onEditorCloseTab={disposeTab}
                        registerPreviewHandle={registerPreviewHandle}
                        onPreviewUrlChange={handlePreviewUrl}
                        onAiDiffAccept={(id) => respondToApproval(id, true)}
                        onAiDiffReject={(id) => respondToApproval(id, false)}
                        onOpenCommitFile={openCommitFileDiffTab}
                        onGitHistorySearchHandle={setGitHistoryHandle}
                        onSetMarkdownView={handleSetMarkdownView}
                        onOpenFile={openLinkedFile}
                        pendingFragment={pendingMarkdownFragment}
                        onConsumePendingFragment={
                          consumePendingMarkdownFragment
                        }
                        workspaceRoot={explorerRoot}
                        secondaryActiveId={secondaryTabId}
                        secondaryActiveTab={secondaryActiveTab}
                        splitRatio={splitRatio}
                        onSplitRatioChange={setSplitRatio}
                      />
                    </div>

                    <WorkspaceInputBar
                      isBlockTab={isBlockTab}
                      isTerminalTab={isTerminalTab}
                      activeLeafId={activeLeafId}
                      cwd={activeCwd}
                      home={home}
                      hasComposer={hasComposer}
                      panelOpen={panelOpen}
                      keysLoaded={keysLoaded}
                      onConnect={() => void openSettingsWindow("models")}
                    />
                  </div>
                </div>
              </ResizablePanel>
            </ResizablePanelGroup>
          </main>

          {!zenMode && (
            <StatusBar
              cwd={activeCwd}
              filePath={activeFilePath}
              home={home}
              onCd={sendCd}
              onWorkspaceChange={handleWorkspaceChange}
              onOpenMini={openMini}
              onOpenAi={togglePanelAndFocus}
              hasComposer={hasComposer}
              privateActive={
                activeTab?.kind === "terminal" && activeTab.private === true
              }
              onOpenProblems={() => openSidebarView("problems")}
            />
          )}

          <WindowVibrancyBridge />
          {IS_MAC ? <LivePageResizeBridge /> : null}
          <WindowActivityBridge
            bindFocus={bindTerminalWindowFocus}
            subscribePresentation={subscribeWindowPresentation}
          />

          <AgentNotificationsBridge
            tabs={tabs}
            activeId={activeId}
            onActivate={onActivateAgent}
          />
          <Toaster position="bottom-right" />

          {hasComposer ? (
            <>
              <AgentRunBridge
                openAiDiffTab={openAiDiffTab}
                closeAiDiffTab={closeAiDiffTab}
              />
              <LocalAgentNotificationsBridge />
            </>
          ) : null}

          {hasComposer && miniPresence.mounted ? (
            <AiMiniWindow state={miniPresence.state} />
          ) : null}
          {askPresence.mounted ? (
            <SelectionAskAi
              state={askPresence.state}
              x={askPopup?.x ?? 0}
              y={askPopup?.y ?? 0}
              onAsk={onAskFromSelection}
              onDismiss={() => setAskPopup(null)}
            />
          ) : null}

          {switcherState && (
            <TabSwitcherHud tabs={spaceTabs} state={switcherState} />
          )}

          <CommandPalette
            open={commandPaletteOpen}
            onOpenChange={setCommandPaletteOpen}
            initialMode={paletteInitialMode}
            commandItems={commandPaletteItems}
            workspaceRoot={explorerRoot}
            onOpenContentHit={openContentHit}
            onOpenFile={openPaletteFile}
            insertCommand={insertHistoryCommand}
          />

          <NewEditorDialog
            open={newEditorOpen}
            onOpenChange={setNewEditorOpen}
            rootPath={explorerRoot ?? home}
            onCreated={(path) => openFileTab(path)}
          />

          <UpdaterDialog />

          <AskpassDialog />
          <RemoteConnections />

          <CloseDialogs
            tabs={tabs}
            pendingCloseTab={pendingCloseTab}
            onCancelClose={cancelClose}
            onConfirmClose={confirmClose}
            pendingTerminalCloseTab={pendingTerminalCloseTab}
            onCancelTerminalClose={cancelTerminalClose}
            onConfirmTerminalClose={confirmTerminalClose}
            pendingDeleteTabs={pendingDeleteTabs}
            onCancelDeleteClose={cancelDeleteClose}
            onConfirmDeleteClose={confirmDeleteClose}
            pendingCloseMany={pendingCloseMany}
            closeManyConfirming={closeManyConfirming}
            onCancelCloseMany={cancelCloseMany}
            onConfirmCloseMany={confirmCloseMany}
            pendingAppClose={pendingAppClose}
            onCancelAppClose={cancelAppClose}
            onConfirmAppClose={confirmAppClose}
          />
        </div>
      </TooltipProvider>
    </ThemeProvider>
  );

  return <AiComposerProvider>{shell}</AiComposerProvider>;
}
