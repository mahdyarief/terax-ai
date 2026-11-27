import type { AdaptedGhosttyTerminalModel } from "@/modules/terminal/ghostty/AdaptedGhosttyTerminalModel";
import type { VisibleBlocks } from "@/modules/terminal/block/lib/blockTypes";
import type { GhosttyBlocks } from "@/modules/terminal/ghostty/GhosttyBlocks";
import { useEffect, useState } from "react";

type Props = {
  controller: GhosttyBlocks;
  model: AdaptedGhosttyTerminalModel;
  cellHeight: number;
};

const EMPTY: VisibleBlocks = { blocks: [], sticky: null, generation: 0 };

/**
 * Block chrome over a block-mode tab. The PTY's output carries the OSC 133
 * marks (they are in the replay), so the same `GhosttyBlocks` the desktop uses
 * finds the same blocks here; only the chrome is drawn by this page.
 */
export function RemoteBlockOverlay({ controller, model, cellHeight }: Props) {
  const [visible, setVisible] = useState<VisibleBlocks>(EMPTY);

  useEffect(() => {
    const update = () => setVisible(controller.visibleBlocks(cellHeight));
    update();
    return model.subscribeDamage(update);
  }, [controller, model, cellHeight]);

  return (
    <div className="pointer-events-none absolute inset-0 overflow-hidden">
      {visible.blocks.map((block) => (
        <div
          key={block.id}
          className="absolute inset-x-0 border-l-2 border-primary/40"
          style={{
            top: Math.max(0, block.headerTop),
            height: Math.max(0, block.bottom - block.headerTop),
          }}
        >
          <div className="flex items-baseline gap-2 px-2 text-xs text-muted-foreground">
            <span className="truncate font-mono text-foreground/80">
              {block.command || "shell"}
            </span>
            {block.running ? (
              <span className="text-xs">running…</span>
            ) : (
              <span
                className={block.ok ? "text-emerald-500" : "text-destructive"}
              >
                {block.exitCode === null ? "" : `exit ${block.exitCode}`}
              </span>
            )}
          </div>
        </div>
      ))}
    </div>
  );
}
