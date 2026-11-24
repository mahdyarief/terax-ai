import {
  isLeaf,
  type PaneNode,
  type SplitDir,
} from "@/modules/terminal/lib/panes";
import type {
  PtyAttachmentRef,
  PtyHandoffCheckpoint,
} from "@/modules/terminal/lib/ptyHandoff";
import type {
  EditorTab,
  GitCommitFileDiffTab,
  GitDiffTab,
  GitHistoryTab,
  MarkdownTab,
  PreviewTab,
  Tab,
  TerminalTab,
} from "@/modules/tabs/lib/useTabs";

type SerializedNode =
  | {
      kind: "leaf";
      cwd?: string;
      active?: boolean;
      ptyAttachment?: PtyAttachmentRef;
    }
  | { kind: "split"; dir: SplitDir; children: SerializedNode[] };

export type SerializedTab =
  | {
      kind: "terminal";
      tree: SerializedNode;
      blocks?: boolean;
      customTitle?: string;
      /** The tab's own shell; absent follows the setting. */
      shell?: string;
    }
  | { kind: "editor"; path: string }
  | { kind: "preview"; url: string }
  | { kind: "markdown"; path: string }
  // Git views only travel in a live cross-window handoff; they are never
  // persisted, since a restart has no reason to reopen a stale diff.
  | {
      kind: "git-diff";
      title: string;
      path: string;
      repoRoot: string;
      mode: GitDiffTab["mode"];
      originalPath: string | null;
      preview: boolean;
    }
  | { kind: "git-history"; title: string; repoRoot: string }
  | {
      kind: "git-commit-file";
      title: string;
      repoRoot: string;
      sha: string;
      shortSha: string;
      subject: string;
      path: string;
      originalPath: string | null;
    };

function basename(path: string): string {
  const parts = path.split(/[\\/]/).filter(Boolean);
  return parts.length ? parts[parts.length - 1] : path;
}

function titleFromUrl(url: string): string {
  try {
    return new URL(url).host || url;
  } catch {
    return url || "preview";
  }
}

function serializeNode(node: PaneNode, activeLeafId: number): SerializedNode {
  if (isLeaf(node)) {
    // A live leaf's `ptyAttachment` is a leftover from the handoff that
    // created it. It is never written out: on disk it would point a restored
    // leaf at a dead PTY, and in a later handoff it would let a cold leaf claim
    // a PTY it no longer owns. `stampHandoffAttachments` sets it fresh.
    return {
      kind: "leaf",
      ...(node.cwd !== undefined && { cwd: node.cwd }),
      ...(node.id === activeLeafId && { active: true }),
    };
  }
  return {
    kind: "split",
    dir: node.dir,
    children: node.children.map((c) => serializeNode(c, activeLeafId)),
  };
}

export function isSerializableTab(tab: Tab): boolean {
  switch (tab.kind) {
    case "terminal":
      return !tab.private;
    case "editor":
    case "preview":
    case "markdown":
      return true;
    default:
      return false;
  }
}

/**
 * Whether a cross-window handoff can carry this tab without losing anything.
 * Broader than {@link isSerializableTab} (git views travel too) but stricter
 * for editors: a dirty buffer only serializes its path, so it stays behind.
 */
export function isHandoffMovable(tab: Tab): boolean {
  switch (tab.kind) {
    case "git-diff":
    case "git-history":
    case "git-commit-file":
      return true;
    case "editor":
      return !tab.dirty;
    default:
      return isSerializableTab(tab);
  }
}

function serializeGitTab(
  tab: GitDiffTab | GitHistoryTab | GitCommitFileDiffTab,
): SerializedTab {
  switch (tab.kind) {
    case "git-diff":
      return {
        kind: "git-diff",
        title: tab.title,
        path: tab.path,
        repoRoot: tab.repoRoot,
        mode: tab.mode,
        originalPath: tab.originalPath,
        preview: tab.preview,
      };
    case "git-history":
      return { kind: "git-history", title: tab.title, repoRoot: tab.repoRoot };
    case "git-commit-file":
      return {
        kind: "git-commit-file",
        title: tab.title,
        repoRoot: tab.repoRoot,
        sha: tab.sha,
        shortSha: tab.shortSha,
        subject: tab.subject,
        path: tab.path,
        originalPath: tab.originalPath,
      };
  }
}

function serializeTab(tab: Tab, forHandoff: boolean): SerializedTab | null {
  if (forHandoff) {
    if (!isHandoffMovable(tab)) return null;
    if (
      tab.kind === "git-diff" ||
      tab.kind === "git-history" ||
      tab.kind === "git-commit-file"
    ) {
      return serializeGitTab(tab);
    }
  } else if (!isSerializableTab(tab)) {
    return null;
  }
  switch (tab.kind) {
    case "terminal":
      return {
        kind: "terminal",
        tree: serializeNode(tab.paneTree, tab.activeLeafId),
        ...(tab.blocks && { blocks: true }),
        ...(tab.customTitle !== undefined && { customTitle: tab.customTitle }),
        ...(tab.shell && { shell: tab.shell }),
      };
    case "editor":
      return { kind: "editor", path: tab.path };
    case "preview":
      return { kind: "preview", url: tab.url };
    case "markdown":
      return { kind: "markdown", path: tab.path };
    default:
      return null;
  }
}

/** Tabs as persisted to disk. */
export function serializeTabs(tabs: Tab[]): SerializedTab[] {
  return serializeTabList(tabs, false);
}

/**
 * Tabs as carried by a cross-window handoff: only {@link isHandoffMovable}
 * tabs, in order, so leaf order matches `detachTabsForHandoff`.
 */
export function serializeTabsForHandoff(tabs: Tab[]): SerializedTab[] {
  return serializeTabList(tabs, true);
}

function serializeTabList(tabs: Tab[], forHandoff: boolean): SerializedTab[] {
  const out: SerializedTab[] = [];
  for (const tab of tabs) {
    const s = serializeTab(tab, forHandoff);
    if (s) out.push(s);
  }
  return out;
}

type HydratedTree = {
  tree: PaneNode;
  activeLeafId: number;
  firstLeafCwd?: string;
  hasPtyAttachment: boolean;
};

function hydrateNode(
  node: SerializedNode,
  allocId: () => number,
  acc: { activeLeafId: number | null },
  keepPty: boolean,
): PaneNode {
  if (node.kind === "leaf") {
    const id = allocId();
    if (node.active && acc.activeLeafId === null) acc.activeLeafId = id;
    return {
      kind: "leaf",
      id,
      ...(node.cwd !== undefined && { cwd: node.cwd }),
      ...(keepPty &&
        node.ptyAttachment && { ptyAttachment: node.ptyAttachment }),
    };
  }
  const children = node.children.map((c) =>
    hydrateNode(c, allocId, acc, keepPty),
  );
  if (children.length === 0) return { kind: "leaf", id: allocId() };
  if (children.length === 1) return children[0];
  return { kind: "split", id: allocId(), dir: node.dir, children };
}

function hydrateTree(
  tree: SerializedNode,
  allocId: () => number,
  keepPty: boolean,
): HydratedTree {
  const acc: { activeLeafId: number | null } = { activeLeafId: null };
  const paneTree = hydrateNode(tree, allocId, acc, keepPty);
  const leaves = collectLeaves(paneTree);
  const activeLeafId = acc.activeLeafId ?? leaves[0]?.id ?? allocId();
  const firstLeafCwd =
    leaves.find((l) => l.id === activeLeafId)?.cwd ?? leaves[0]?.cwd;
  return {
    tree: paneTree,
    activeLeafId,
    firstLeafCwd,
    hasPtyAttachment: leaves.some((l) => l.ptyAttachment !== undefined),
  };
}

function collectLeaves(node: PaneNode): Array<Extract<PaneNode, { kind: "leaf" }>> {
  if (isLeaf(node)) return [node];
  return node.children.flatMap(collectLeaves);
}

function hydrateTab(
  s: SerializedTab,
  spaceId: string,
  allocId: () => number,
  keepPty: boolean,
): Tab | null {
  switch (s.kind) {
    case "terminal": {
      const { tree, activeLeafId, firstLeafCwd, hasPtyAttachment } =
        hydrateTree(s.tree, allocId, keepPty);
      const title =
        s.customTitle ??
        (firstLeafCwd ? basename(firstLeafCwd) : s.blocks ? "blocks" : "shell");
      return {
        id: allocId(),
        kind: "terminal",
        spaceId,
        cold: !(keepPty && hasPtyAttachment),
        title,
        cwd: firstLeafCwd,
        paneTree: tree,
        activeLeafId,
        ...(s.blocks && { blocks: true }),
        ...(s.customTitle !== undefined && { customTitle: s.customTitle }),
        ...(typeof s.shell === "string" && s.shell && { shell: s.shell }),
      } satisfies TerminalTab;
    }
    case "editor":
      return {
        id: allocId(),
        kind: "editor",
        spaceId,
        cold: true,
        title: basename(s.path),
        path: s.path,
        dirty: false,
        preview: false,
      } satisfies EditorTab;
    case "preview":
      return {
        id: allocId(),
        kind: "preview",
        spaceId,
        cold: true,
        title: titleFromUrl(s.url),
        url: s.url,
      } satisfies PreviewTab;
    case "markdown":
      return {
        id: allocId(),
        kind: "markdown",
        spaceId,
        cold: true,
        title: basename(s.path),
        path: s.path,
      } satisfies MarkdownTab;
    case "git-diff":
      return {
        id: allocId(),
        kind: "git-diff",
        spaceId,
        title: s.title,
        path: s.path,
        repoRoot: s.repoRoot,
        mode: s.mode,
        originalPath: s.originalPath,
        preview: s.preview,
      } satisfies GitDiffTab;
    case "git-history":
      return {
        id: allocId(),
        kind: "git-history",
        spaceId,
        title: s.title,
        repoRoot: s.repoRoot,
      } satisfies GitHistoryTab;
    case "git-commit-file":
      return {
        id: allocId(),
        kind: "git-commit-file",
        spaceId,
        title: s.title,
        repoRoot: s.repoRoot,
        sha: s.sha,
        shortSha: s.shortSha,
        subject: s.subject,
        path: s.path,
        originalPath: s.originalPath,
      } satisfies GitCommitFileDiffTab;
    default:
      return null;
  }
}

export function freshTerminalTab(
  spaceId: string,
  cwd: string | null,
  allocId: () => number,
): TerminalTab {
  const leafId = allocId();
  return {
    id: allocId(),
    kind: "terminal",
    spaceId,
    cold: true,
    title: cwd ? basename(cwd) : "shell",
    cwd: cwd ?? undefined,
    paneTree: { kind: "leaf", id: leafId, ...(cwd && { cwd }) },
    activeLeafId: leafId,
  };
}

function stampNode(
  node: SerializedNode,
  next: () => PtyAttachmentRef | null,
): SerializedNode {
  if (node.kind === "leaf") {
    const attachment = next();
    const { ptyAttachment: _stale, ...rest } = node;
    return attachment ? { ...rest, ptyAttachment: attachment } : rest;
  }
  return {
    ...node,
    children: node.children.map((child) => stampNode(child, next)),
  };
}

/**
 * Attach prepared handoff checkpoints to serialized terminal leaves.
 *
 * `leafPtyIds` is the leaf-order PTY id list returned by
 * `detachTabsForHandoff`; each entry names the PTY the leaf owned before the
 * move, so cold leaves stay cold instead of inheriting a neighbour's
 * checkpoint. Without it, checkpoints are assigned in request order.
 */
export function stampHandoffAttachments(
  serialized: SerializedTab[],
  checkpoints: PtyHandoffCheckpoint[],
  leafPtyIds?: Array<number | null>,
): SerializedTab[] {
  const byPty = new Map(checkpoints.map((c) => [c.ptyId, c]));
  const ordered = leafPtyIds
    ? leafPtyIds.map((ptyId) => (ptyId === null ? null : byPty.get(ptyId)))
    : checkpoints;
  let index = 0;
  const next = (): PtyAttachmentRef | null => {
    const checkpoint = ordered[index++];
    if (!checkpoint) return null;
    return {
      ptyId: checkpoint.ptyId,
      checkpoint: checkpoint.checkpoint,
      cols: checkpoint.cols,
      rows: checkpoint.rows,
    };
  };
  return serialized.map((tab) =>
    tab.kind === "terminal" ? { ...tab, tree: stampNode(tab.tree, next) } : tab,
  );
}

export function hydrateTabs(
  serialized: SerializedTab[],
  spaceId: string,
  allocId: () => number,
  opts: { keepPty?: boolean } = {},
): Tab[] {
  if (!Array.isArray(serialized)) return [];
  const out: Tab[] = [];
  for (const s of serialized) {
    try {
      const tab = hydrateTab(s, spaceId, allocId, opts.keepPty === true);
      if (tab) out.push(tab);
    } catch {
      // Skip corrupted entries rather than failing the whole restore.
    }
  }
  return out;
}
