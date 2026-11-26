import {
  ResizableHandle,
  ResizablePanel,
  ResizablePanelGroup,
} from "@/components/ui/resizable";
import type { TerminalSearchController } from "@/modules/terminal/search/TerminalSearchController";
import { Fragment, useCallback, useEffect, useMemo, useRef } from "react";
import { type LayoutChangedMeta, useGroupRef } from "react-resizable-panels";
import { useTerminalDropStore } from "./lib/dropStore";
import { firstLeafSlotId, type PaneNode, validSplitSizes } from "./lib/panes";
import {
  beginTerminalResizeInteraction,
  endTerminalResizeInteraction,
} from "./lib/terminalResizeInteraction";
import { TerminalPane, type TerminalPaneHandle } from "./TerminalPane";

type LeafBundle = {
  setRef: (h: TerminalPaneHandle | null) => void;
  onSearchReady: (leafId: number, addon: TerminalSearchController) => void;
  onCwd: (leafId: number, cwd: string) => void;
  onExit: (leafId: number, code: number) => void;
};

type Props = {
  node: PaneNode;
  tabVisible: boolean;
  activeLeafId: number;
  blocks: boolean;
  /** The tab's own shell; undefined follows the setting. */
  shell?: string;
  onFocusLeaf: (leafId: number) => void;
  /** The user resized a split (`undefined`: back to even sizes). */
  onSplitSizes: (splitId: number, sizes: readonly number[] | undefined) => void;
  getBundle: (leafId: number) => LeafBundle;
};

type SplitNode = Extract<PaneNode, { kind: "split" }>;

export function PaneTreeView(props: Props) {
  const { node } = props;
  if (node.kind === "leaf") {
    const { tabVisible, activeLeafId, blocks, shell, onFocusLeaf, getBundle } =
      props;
    const focused = node.id === activeLeafId;
    const b = getBundle(node.id);
    return (
      <div
        onMouseDownCapture={() => {
          if (!focused) onFocusLeaf(node.id);
        }}
        // Catches focus from Tab, programmatic focus, or any path that
        // skips mousedown — keeps activeLeafId in sync with DOM focus.
        onFocus={() => {
          if (!focused) onFocusLeaf(node.id);
        }}
        data-pane-leaf={node.id}
        className="relative h-full w-full"
      >
        <TerminalPane
          leafId={node.id}
          visible={tabVisible}
          focused={focused}
          initialCwd={node.cwd}
          ptyId={node.ptyAttachment?.ptyId}
          blocks={blocks}
          shell={shell}
          ref={b.setRef}
          onSearchReady={b.onSearchReady}
          onCwd={b.onCwd}
          onExit={b.onExit}
        />
        <DropOverlay leafId={node.id} />
      </div>
    );
  }

  return <PaneSplitView {...props} node={node} />;
}

const panelIdOf = (child: PaneNode) => `pane-slot-${firstLeafSlotId(child)}`;

function PaneSplitView(props: Props & { node: SplitNode }) {
  const { node, onSplitSizes } = props;
  const groupRef = useGroupRef();
  const panelIds = node.children.map(panelIdOf);
  const panelKey = panelIds.join(" ");

  // Read once per set of panes: the group owns the layout after it mounts, and
  // what the user drags is written back to the tree (and the saved space).
  // biome-ignore lint/correctness/useExhaustiveDependencies: on purpose, see above
  const defaultLayout = useMemo(() => {
    if (!validSplitSizes(node.sizes, panelIds.length)) return undefined;
    const sizes = node.sizes;
    return Object.fromEntries(panelIds.map((id, i) => [id, sizes[i]]));
  }, [panelKey]);

  const onLayoutChanged = (
    layout: Record<string, number>,
    meta: LayoutChangedMeta,
  ) => {
    if (!meta.isUserInteraction) return;
    const sizes = panelIds.map((id) => layout[id]);
    if (sizes.every((n) => typeof n === "number")) onSplitSizes(node.id, sizes);
  };

  const resetToEven = () => {
    const even = 100 / panelIds.length;
    groupRef.current?.setLayout(
      Object.fromEntries(panelIds.map((id) => [id, even])),
    );
    onSplitSizes(node.id, undefined);
  };

  return (
    <ResizablePanelGroup
      groupRef={groupRef}
      defaultLayout={defaultLayout}
      onLayoutChanged={onLayoutChanged}
      orientation={node.dir === "row" ? "horizontal" : "vertical"}
    >
      {node.children.map((child, i) => {
        const slotId = firstLeafSlotId(child);
        return (
          <Fragment key={slotId}>
            {i > 0 && <TerminalResizableHandle onReset={resetToEven} />}
            <ResizablePanel id={panelIdOf(child)} minSize="10%">
              <PaneTreeView {...props} node={child} />
            </ResizablePanel>
          </Fragment>
        );
      })}
    </ResizablePanelGroup>
  );
}

const RESIZE_KEYS = new Set([
  "ArrowDown",
  "ArrowLeft",
  "ArrowRight",
  "ArrowUp",
  "End",
  "Home",
]);

function TerminalResizableHandle({ onReset }: { onReset: () => void }) {
  const interactionToken = useRef<object>({});
  const removePointerListeners = useRef<(() => void) | null>(null);

  const finish = useCallback(() => {
    removePointerListeners.current?.();
    removePointerListeners.current = null;
    endTerminalResizeInteraction(interactionToken.current);
  }, []);

  const startPointerInteraction = useCallback(() => {
    finish();
    beginTerminalResizeInteraction(interactionToken.current);
    const handlePointerEnd = () => finish();
    window.addEventListener("pointerup", handlePointerEnd, true);
    window.addEventListener("pointercancel", handlePointerEnd, true);
    removePointerListeners.current = () => {
      window.removeEventListener("pointerup", handlePointerEnd, true);
      window.removeEventListener("pointercancel", handlePointerEnd, true);
    };
  }, [finish]);

  useEffect(() => finish, [finish]);

  return (
    <ResizableHandle
      className="bg-border/50 transition-colors duration-[var(--dur-fast)] after:w-3 hover:bg-border"
      onPointerDownCapture={(event) => {
        if (event.button === 0) startPointerInteraction();
      }}
      onKeyDownCapture={(event) => {
        if (RESIZE_KEYS.has(event.key)) {
          beginTerminalResizeInteraction(interactionToken.current);
        }
      }}
      onKeyUpCapture={(event) => {
        if (RESIZE_KEYS.has(event.key)) finish();
      }}
      onBlur={finish}
      // Double-click puts the panes of this split back to even sizes.
      onDoubleClick={onReset}
      title="Drag to resize, double-click to make even"
    />
  );
}

function DropOverlay({ leafId }: { leafId: number }) {
  const active = useTerminalDropStore((s) => s.targetLeafId === leafId);
  if (!active) return null;
  return (
    <div className="pointer-events-none absolute inset-2 grid place-items-center rounded-lg border border-primary/45 bg-background/70 text-xs font-medium text-foreground shadow-lg backdrop-blur-sm">
      Drop file path here
    </div>
  );
}
