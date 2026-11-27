import { useMemo, useState } from "react";
import type { Device } from "./api";
import { activePane, useTabsSnapshot, type RemoteTab } from "./tabs";
import { RemoteTerminal } from "./terminal/RemoteTerminal";

/**
 * The paired view: the tab list and the selected terminal. Typing, blocks and
 * tab actions come with their own tasks.
 */
export default function RemoteApp({ device }: { device: Device }) {
  const { snapshot, status } = useTabsSnapshot();
  const [selected, setSelected] = useState<string | null>(null);

  const terminals = useMemo(
    () =>
      (snapshot?.tabs ?? []).filter(
        (tab) => tab.kind === "terminal" && activePane(tab)?.ptyId != null,
      ),
    [snapshot],
  );
  const current: RemoteTab | null =
    terminals.find((tab) => `${tab.window}:${tab.id}` === selected) ??
    terminals[0] ??
    null;
  const ptyId = current ? (activePane(current)?.ptyId ?? null) : null;

  return (
    <main className="flex h-dvh flex-col bg-background text-foreground">
      <header className="flex items-center justify-between gap-2 border-b border-border px-3 py-2">
        <h1 className="text-sm font-semibold">Terax Remote</h1>
        <span className="text-xs text-muted-foreground">{device.name}</span>
      </header>

      <div className="flex min-h-0 flex-1">
        <nav className="hidden w-56 shrink-0 overflow-y-auto border-r border-border sm:block">
          {terminals.length === 0 ? (
            <p className="p-3 text-xs text-muted-foreground">
              {status === "live"
                ? "No terminal tabs are open on the desktop."
                : "Looking for terminals…"}
            </p>
          ) : (
            <ul className="flex flex-col p-1">
              {terminals.map((tab) => (
                <li key={`${tab.window}:${tab.id}`}>
                  <button
                    type="button"
                    onClick={() => setSelected(`${tab.window}:${tab.id}`)}
                    className={`w-full truncate rounded px-2 py-1.5 text-left text-xs ${
                      tab === current ? "bg-accent text-accent-foreground" : ""
                    }`}
                  >
                    {tab.title}
                  </button>
                </li>
              ))}
            </ul>
          )}
        </nav>

        <section className="min-h-0 min-w-0 flex-1">
          {ptyId !== null ? (
            <RemoteTerminal
              ptyId={ptyId}
              blocks={current.mode === "blocks"}
            />
          ) : (
            <p className="p-4 text-sm text-muted-foreground">
              {status === "live"
                ? "No terminal is available yet."
                : "Connecting to Terax…"}
            </p>
          )}
        </section>
      </div>
    </main>
  );
}
