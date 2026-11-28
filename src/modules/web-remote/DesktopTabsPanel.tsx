import { Button } from "@/components/ui/button";
import { activePane, useTabsSnapshot, type RemoteTab, type TabSnapshot } from "@/web/tabs";
import { RemoteTerminal } from "@/web/terminal/RemoteTerminal";
import { emitTo } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useEffect, useMemo, useState } from "react";

export type DesktopTabEntry = {
  windowLabel: string;
  tab: RemoteTab;
};

export function otherWindowTabs(
  snapshot: TabSnapshot | null,
  myLabel: string,
): DesktopTabEntry[] {
  if (!snapshot) return [];
  return snapshot.tabs
    .filter((tab) => tab.window !== myLabel)
    .map((tab) => ({ windowLabel: tab.window, tab }));
}

export function DesktopTabsPanel({
  activeSpaceId,
  localTerminalTabs,
  onSendBack,
}: {
  activeSpaceId: string;
  localTerminalTabs: ReadonlyArray<{ id: number; title: string }>;
  onSendBack: (tabId: number) => void;
}) {
  const { snapshot } = useTabsSnapshot();
  const [selected, setSelected] = useState<DesktopTabEntry | null>(null);
  const [canInput, setCanInput] = useState(false);

  useEffect(() => {
    void fetch("/api/me")
      .then((r) => (r.ok ? r.json() : null))
      .then((me: { grant?: { input?: boolean } } | null) => {
        setCanInput(Boolean(me?.grant?.input));
      })
      .catch(() => {});
  }, []);

  const myLabel = useMemo(() => {
    try {
      return getCurrentWindow().label;
    } catch {
      return "remote-0";
    }
  }, []);

  const entries = useMemo(
    () => otherWindowTabs(snapshot, myLabel),
    [snapshot, myLabel],
  );

  const selectedPane = selected ? activePane(selected.tab) : undefined;
  const selectedPtyId = selectedPane?.ptyId ?? null;

  return (
    <div className="flex h-full min-h-0 flex-col gap-2 p-2 text-xs">
      <div className="flex items-center justify-between px-1 text-[11px] font-medium text-muted-foreground">
        <span>Desktop windows</span>
        <span>{entries.length}</span>
      </div>

      {selected && selectedPtyId != null ? (
        <div className="flex min-h-0 flex-1 flex-col gap-1.5 rounded border border-border/60 bg-card/40 p-1.5">
          <div className="flex items-center justify-between gap-2">
            <span className="truncate font-medium">
              {selected.tab.title} ({selected.windowLabel})
            </span>
            <div className="flex items-center gap-1">
              <Button
                size="sm"
                variant="outline"
                className="h-6 px-2 text-[11px]"
                onClick={() => {
                  void emitTo(selected.windowLabel, "remote:take-over-tab", {
                    tabId: selected.tab.id,
                    targetWindow: myLabel,
                    spaceId: activeSpaceId,
                  });
                  setSelected(null);
                }}
              >
                Take over
              </Button>
              <Button
                size="sm"
                variant="ghost"
                className="h-6 px-2 text-[11px]"
                onClick={() => setSelected(null)}
              >
                Close
              </Button>
            </div>
          </div>
          <div className="min-h-0 flex-1 overflow-hidden rounded border border-border/40">
            <RemoteTerminal ptyId={selectedPtyId} typing={canInput} />
          </div>
        </div>
      ) : (
        <div className="flex min-h-0 flex-1 flex-col gap-1 overflow-y-auto">
          {entries.length === 0 ? (
            <p className="px-1 py-2 text-[11px] text-muted-foreground">
              No tabs open on other windows.
            </p>
          ) : (
            entries.map((entry) => {
              const pane = activePane(entry.tab);
              const hasPty = pane?.ptyId != null;
              return (
                <div
                  key={`${entry.windowLabel}:${entry.tab.id}`}
                  className="flex items-center justify-between gap-2 rounded px-2 py-1.5 hover:bg-muted/50"
                >
                  <button
                    type="button"
                    className="min-w-0 flex-1 text-left"
                    onClick={() => {
                      if (hasPty) setSelected(entry);
                    }}
                  >
                    <div className="truncate font-medium">{entry.tab.title}</div>
                    <div className="truncate text-[10px] text-muted-foreground">
                      {entry.windowLabel}
                      {entry.tab.workspace?.label
                        ? ` · ${entry.tab.workspace.label}`
                        : ""}
                    </div>
                  </button>
                  {hasPty ? (
                    <Button
                      size="sm"
                      variant="outline"
                      className="h-6 shrink-0 px-2 text-[11px]"
                      onClick={() => {
                        void emitTo(entry.windowLabel, "remote:take-over-tab", {
                          tabId: entry.tab.id,
                          targetWindow: myLabel,
                          spaceId: activeSpaceId,
                        });
                      }}
                    >
                      Take over
                    </Button>
                  ) : null}
                </div>
              );
            })
          )}

          {localTerminalTabs.length > 0 ? (
            <div className="mt-3 border-t border-border/50 pt-2">
              <div className="mb-1 px-1 text-[11px] font-medium text-muted-foreground">
                Browser terminals
              </div>
              {localTerminalTabs.map((t) => (
                <div
                  key={t.id}
                  className="flex items-center justify-between gap-2 rounded px-2 py-1"
                >
                  <span className="truncate">{t.title}</span>
                  <Button
                    size="sm"
                    variant="ghost"
                    className="h-6 shrink-0 px-2 text-[11px]"
                    onClick={() => onSendBack(t.id)}
                  >
                    Send back
                  </Button>
                </div>
              ))}
            </div>
          ) : null}
        </div>
      )}
    </div>
  );
}
