import type { Device } from "./api";

/**
 * The paired view. Tasks build it out: the tab list, a terminal per pane, block
 * mode, typing and tab actions.
 */
export default function RemoteApp({ device }: { device: Device }) {
  return (
    <main className="min-h-dvh bg-background text-foreground">
      <header className="flex items-center justify-between border-b border-border px-4 py-3">
        <h1 className="text-sm font-semibold">Terax Remote</h1>
        <span className="text-xs text-muted-foreground">{device.name}</span>
      </header>
      <p className="p-4 text-sm text-muted-foreground">
        Connected. Your terminals appear here.
      </p>
    </main>
  );
}
