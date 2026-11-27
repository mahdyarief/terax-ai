import { useMemo, useState } from "react";
import type { Device } from "./api";
import {
  activePane,
  panes,
  useTabsSnapshot,
  type RemotePane,
  type RemoteTab,
} from "./tabs";
import { RemoteTerminal } from "./terminal/RemoteTerminal";

const STATE_LABEL: Record<string, string> = {
  working: "running",
  attention: "waiting",
  finished: "done",
  idle: "idle",
};

const STATE_COLOR: Record<string, string> = {
  working: "bg-emerald-500",
  attention: "bg-amber-500",
  finished: "bg-sky-500",
  idle: "bg-muted-foreground",
};

type Group = { key: string; label: string; tabs: RemoteTab[] };

/** Tabs of every window, grouped by the workspace they run in. */
function groupByWorkspace(tabs: readonly RemoteTab[]): Group[] {
  const groups = new Map<string, Group>();
  for (const tab of tabs) {
    const key = tab.workspace?.key ?? tab.spaceId ?? "local";
    const label = tab.workspace?.label ?? "Local";
    const group = groups.get(key) ?? { key, label, tabs: [] };
    group.tabs.push(tab);
    groups.set(key, group);
  }
  return [...groups.values()].sort((left, right) =>
    left.label.localeCompare(right.label),
  );
}

function AgentChip({ tab }: { tab: RemoteTab }) {
  if (!tab.agent?.state) return null;
  return (
    <span className="flex shrink-0 items-center gap-1 text-[10px] text-muted-foreground">
      <svg viewBox="0 0 16 16" aria-hidden="true" className="size-3">
        <path
          d="M8 1l1.7 4.3L14 7l-4.3 1.7L8 13 6.3 8.7 2 7l4.3-1.7z"
          fill="currentColor"
          opacity="0.7"
        />
      </svg>
      <span
        className={`size-1.5 rounded-full ${STATE_COLOR[tab.agent.state] ?? STATE_COLOR.idle}`}
      />
      {tab.agent.kind ?? "agent"} {STATE_LABEL[tab.agent.state] ?? ""}
    </span>
  );
}

function TabButton({
  tab,
  active,
  onSelect,
}: {
  tab: RemoteTab;
  active: boolean;
  onSelect: () => void;
}) {
  const terminal = tab.kind === "terminal";
  return (
    <button
      type="button"
      onClick={onSelect}
      disabled={!terminal}
      className={`flex w-full items-center gap-2 rounded px-2 py-1.5 text-left text-xs ${
        active ? "bg-accent text-accent-foreground" : "hover:bg-accent/50"
      } ${terminal ? "" : "opacity-60"}`}
    >
      <span className="min-w-0 flex-1 truncate">{tab.title}</span>
      {terminal ? <AgentChip tab={tab} /> : <span>tab</span>}
    </button>
  );
}

function TabList({
  groups,
  currentKey,
  onSelect,
}: {
  groups: Group[];
  currentKey: string | null;
  onSelect: (tab: RemoteTab) => void;
}) {
  return (
    <div className="flex flex-col gap-3 p-2">
      {groups.map((group) => (
        <section key={group.key}>
          <h2 className="px-2 pb-1 text-[10px] font-semibold tracking-wide text-muted-foreground uppercase">
            {group.label}
          </h2>
          <ul className="flex flex-col gap-0.5">
            {group.tabs.map((tab) => (
              <li key={`${tab.window}:${tab.id}`}>
                <TabButton
                  tab={tab}
                  active={`${tab.window}:${tab.id}` === currentKey}
                  onSelect={() => onSelect(tab)}
                />
              </li>
            ))}
          </ul>
        </section>
      ))}
    </div>
  );
}

function PanePicker({
  list,
  selected,
  onSelect,
}: {
  list: RemotePane[];
  selected: number | null;
  onSelect: (pane: RemotePane) => void;
}) {
  if (list.length < 2) return null;
  return (
    <div className="absolute inset-x-0 top-0 z-10 flex justify-center gap-1 p-1">
      {list.map((pane, index) => (
        <button
          key={pane.leafId}
          type="button"
          onClick={() => onSelect(pane)}
          className={`rounded-full px-2 py-0.5 text-[10px] ${
            pane.leafId === selected
              ? "bg-primary text-primary-foreground"
              : "bg-black/60 text-white"
          }`}
        >
          pane {index + 1}
        </button>
      ))}
    </div>
  );
}

/**
 * The paired view: every window's tabs, grouped by workspace, and the selected
 * terminal. On a phone the list is a drawer.
 */
export default function RemoteApp({ device }: { device: Device }) {
  const { snapshot, status } = useTabsSnapshot();
  const [selectedKey, setSelectedKey] = useState<string | null>(null);
  const [leafByTab, setLeafByTab] = useState<Record<string, number>>({});
  const [drawer, setDrawer] = useState(false);
  const [typing, setTyping] = useState(false);

  const groups = useMemo(
    () =>
      groupByWorkspace(
        (snapshot?.tabs ?? []).filter(
          (tab) => tab.kind === "terminal" && panes(tab).length > 0,
        ),
      ),
    [snapshot],
  );
  const terminals = useMemo(
    () => groups.flatMap((group) => group.tabs),
    [groups],
  );
  const current: RemoteTab | null =
    terminals.find((tab) => `${tab.window}:${tab.id}` === selectedKey) ??
    terminals[0] ??
    null;
  const key = current ? `${current.window}:${current.id}` : null;
  const pane =
    (current &&
      panes(current).find(
        (candidate) => candidate.leafId === leafByTab[key ?? ""],
      )) ||
    (current ? activePane(current) : null);
  const ptyId = pane?.ptyId ?? null;

  const select = (tab: RemoteTab) => {
    setSelectedKey(`${tab.window}:${tab.id}`);
    setDrawer(false);
  };

  const list = <TabList groups={groups} currentKey={key} onSelect={select} />;

  return (
    <main className="flex h-dvh flex-col bg-background text-foreground">
      <header className="flex items-center gap-2 border-b border-border px-3 py-2">
        <button
          type="button"
          onClick={() => setDrawer((open) => !open)}
          className="rounded px-2 py-1 text-xs hover:bg-accent md:hidden"
          aria-expanded={drawer}
        >
          Tabs
        </button>
        <h1 className="min-w-0 flex-1 truncate text-sm font-semibold">
          {current?.title ?? "Terax Remote"}
        </h1>
        {device.grant.input ? (
          <button
            type="button"
            onClick={() => setTyping((on) => !on)}
            aria-pressed={typing}
            className={`shrink-0 rounded px-2 py-1 text-xs ${
              typing ? "bg-primary text-primary-foreground" : "hover:bg-accent"
            }`}
          >
            {typing ? "Typing" : "Type"}
          </button>
        ) : null}
        <span className="shrink-0 text-xs text-muted-foreground">
          {device.name}
        </span>
      </header>

      <div className="relative flex min-h-0 flex-1">
        <nav className="hidden w-56 shrink-0 overflow-y-auto border-r border-border md:block">
          {groups.length === 0 ? (
            <p className="p-3 text-xs text-muted-foreground">
              {status === "live"
                ? "No terminal tabs are open on the desktop."
                : "Looking for terminals…"}
            </p>
          ) : (
            list
          )}
        </nav>

        {drawer ? (
          <div className="absolute inset-0 z-20 flex md:hidden">
            <div className="w-72 overflow-y-auto border-r border-border bg-card">
              {groups.length === 0 ? (
                <p className="p-3 text-xs text-muted-foreground">
                  No terminal tabs yet.
                </p>
              ) : (
                list
              )}
            </div>
            <button
              type="button"
              aria-label="Close tabs"
              onClick={() => setDrawer(false)}
              className="flex-1 bg-black/40"
            />
          </div>
        ) : null}

        <section className="relative min-h-0 min-w-0 flex-1">
          <PanePicker
            list={current ? panes(current) : []}
            selected={pane?.leafId ?? null}
            onSelect={(next) => {
              if (key) setLeafByTab((map) => ({ ...map, [key]: next.leafId }));
            }}
          />
          {ptyId !== null && current ? (
            <RemoteTerminal
              ptyId={ptyId}
              blocks={current.mode === "blocks"}
              typing={typing && device.grant.input}
            />
          ) : (
            <p className="p-4 text-sm text-muted-foreground">
              {status === "live"
                ? "This tab has no live terminal yet."
                : "Connecting to Terax…"}
            </p>
          )}
        </section>
      </div>
    </main>
  );
}
