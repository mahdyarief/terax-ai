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
    }
  | { kind: "editor"; path: string }
  | { kind: "preview"; url: string }
  | { kind: "markdown"; path: string };

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
    return {
      kind: "leaf",
      ...(node.cwd !== undefined && { cwd: node.cwd }),
      ...(node.id === activeLeafId && { active: true }),
      ...(node.ptyAttachment && { ptyAttachment: node.ptyAttachment }),
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

function serializeTab(tab: Tab): SerializedTab | null {
  if (!isSerializableTab(tab)) return null;
  switch (tab.kind) {
    case "terminal":
      return {
        kind: "terminal",
        tree: serializeNode(tab.paneTree, tab.activeLeafId),
        ...(tab.blocks && { blocks: true }),
        ...(tab.customTitle !== undefined && { customTitle: tab.customTitle }),
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

export function serializeTabs(tabs: Tab[]): SerializedTab[] {
  const out: SerializedTab[] = [];
  for (const tab of tabs) {
    const s = serializeTab(tab);
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
): PaneNode {
  if (node.kind === "leaf") {
    const id = allocId();
    if (node.active && acc.activeLeafId === null) acc.activeLeafId = id;
    return {
      kind: "leaf",
      id,
      ...(node.cwd !== undefined && { cwd: node.cwd }),
      ...(node.ptyAttachment && { ptyAttachment: node.ptyAttachment }),
    };
  }
  const children = node.children.map((c) => hydrateNode(c, allocId, acc));
  if (children.length === 0) return { kind: "leaf", id: allocId() };
  if (children.length === 1) return children[0];
  return { kind: "split", id: allocId(), dir: node.dir, children };
}

function hydrateTree(
  tree: SerializedNode,
  allocId: () => number,
): HydratedTree {
  const acc: { activeLeafId: number | null } = { activeLeafId: null };
  const paneTree = hydrateNode(tree, allocId, acc);
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
        hydrateTree(s.tree, allocId);
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
    if (!attachment) return node;
    return { ...node, ptyAttachment: attachment };
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
