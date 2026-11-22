import type { MarkdownTab, Tab } from "@/modules/tabs";
import { slotOf, slotStyle } from "@/modules/tabs/lib/splitLayout";
import { MarkdownPreviewPane } from "./MarkdownPreviewPane";
import type { MarkdownViewMode } from "./MarkdownViewToggle";

type Props = {
  tabs: Tab[];
  activeId: number;
  /** Tab shown on the right of a split view, if any. */
  secondaryId?: number | null;
  /** `line` is the source line being read, so the editor can open there. */
  onSetMarkdownView: (id: number, mode: MarkdownViewMode, line?: number) => void;
  /** Open a file that a relative link in the document points at. */
  onOpenFile?: (path: string, pin?: boolean, fragment?: string) => void;
  pendingFragment?: { path: string; fragment: string } | null;
  onConsumePendingFragment?: (path: string, fragment: string) => void;
};

/** The folder the explorer shows: the active terminal's cwd, else any terminal's. */
function workspaceRootOf(tabs: Tab[], activeId: number): string | null {
  const active = tabs.find((t) => t.id === activeId);
  if (active?.kind === "terminal" && active.cwd) return active.cwd;
  for (const t of tabs) if (t.kind === "terminal" && t.cwd) return t.cwd;
  return null;
}

export function MarkdownStack({
  tabs,
  activeId,
  secondaryId = null,
  onSetMarkdownView,
  onOpenFile,
  pendingFragment,
  onConsumePendingFragment,
}: Props) {
  const markdowns = tabs.filter(
    (t): t is MarkdownTab => t.kind === "markdown" && !t.cold,
  );
  if (markdowns.length === 0) return null;
  const workspaceRoot = workspaceRootOf(tabs, activeId);
  const split = secondaryId !== null;
  return (
    <div className="relative h-full w-full">
      {markdowns.map((t) => {
        const slot = slotOf(t.id, activeId, secondaryId);
        const visible = slot !== null;
        return (
          <div
            key={t.id}
            className={
              visible ? "pointer-events-auto" : "invisible pointer-events-none"
            }
            style={slotStyle(slot, split)}
            aria-hidden={!visible}
          >
            <MarkdownPreviewPane
              path={t.path}
              workspaceRoot={workspaceRoot}
              visible={visible}
              onSetView={(mode, line) => onSetMarkdownView(t.id, mode, line)}
              onOpenFile={onOpenFile}
              scrollToFragment={
                pendingFragment?.path.replace(/\\/g, "/") ===
                t.path.replace(/\\/g, "/")
                  ? pendingFragment.fragment
                  : null
              }
              onFragmentConsumed={() => {
                if (
                  pendingFragment?.path.replace(/\\/g, "/") ===
                  t.path.replace(/\\/g, "/")
                ) {
                  onConsumePendingFragment?.(
                    pendingFragment.path,
                    pendingFragment.fragment,
                  );
                }
              }}
            />
          </div>
        );
      })}
    </div>
  );
}
