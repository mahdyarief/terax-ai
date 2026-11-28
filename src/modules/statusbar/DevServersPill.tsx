import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { statusMenuClass, statusMenuScrollClass } from "@/components/ui/menu";
import { writeClipboardText } from "@/lib/clipboard";
import { openExternalUrl } from "@/lib/external-link";
import { isRemoteRuntime } from "@/lib/remoteRuntime";
import { cn } from "@/lib/utils";
import { remoteProxyUrl } from "@/modules/preview/lib/previewUrl";
import { invoke } from "@tauri-apps/api/core";
import {
  type LiveServer,
  liveServerName,
  useLiveServerStore,
} from "@/modules/live-server/liveServer";
import { portPresetLabel } from "@/modules/preview/lib/portPresets";
import {
  type DevServer,
  listDevServers,
  useDevServerStore,
} from "@/modules/terminal/lib/devServers";
import {
  Copy01Icon,
  Globe02Icon,
  LinkSquare02Icon,
  StopCircleIcon,
  Wifi01Icon,
} from "@hugeicons/core-free-icons";
import { HugeiconsIcon } from "@hugeicons/react";
import { Fragment, useEffect, useMemo } from "react";
import { toast } from "sonner";

type Props = {
  /** Opens the address in a Terax preview tab. */
  onOpenPreview: (url: string) => void;
};

/** One server in the menu: printed by a terminal, or a Terax Live Server. */
export type ServerEntry = {
  key: string;
  port: number;
  url: string;
  label: string;
  /** The SSH host a terminal's server runs on; its localhost is not ours. */
  remoteHost: string | null;
  /** Only a Live Server can be stopped and shared from here. */
  live?: {
    lanUrls: readonly string[];
    onShare: (lan: boolean) => void;
    onStop: () => void;
  };
};

/** How a server reads in the menu: its address, then what usually runs there. */
export function devServerLabel(s: DevServer): string {
  const known = portPresetLabel(s.port);
  const where = s.host ? ` on ${s.host}` : "";
  return `${s.url.replace(/^https?:\/\//, "").replace(/\/$/, "")}${known ? ` · ${known}` : ""}${where}`;
}

export function liveServerEntry(
  s: LiveServer,
  actions: { start: (lan: boolean) => void; stop: () => void },
): ServerEntry {
  return {
    key: `live:${s.id}`,
    port: s.port,
    url: s.url,
    label: `localhost:${s.port} · Live Server: ${liveServerName(s)}`,
    remoteHost: null,
    live: {
      lanUrls: s.lanUrls,
      onShare: actions.start,
      onStop: actions.stop,
    },
  };
}

const report = (e: unknown) =>
  toast.error("Live Server", { description: String(e) });

/**
 * The servers on this machine: those the terminals printed the address of
 * (Vite's `Local:` line, uvicorn's `Running on`) and the folders Terax
 * serves itself. Hidden until there is one.
 */
export function DevServersPill({ onOpenPreview }: Props) {
  const byLeaf = useDevServerStore((s) => s.byLeaf);
  const live = useLiveServerStore((s) => s.servers);
  const entries = useMemo(() => {
    const { start, stop } = useLiveServerStore.getState();
    const out: ServerEntry[] = live.map((s) =>
      liveServerEntry(s, {
        start: (lan) =>
          void start(s.root, { lan, workspace: s.workspace }).catch(report),
        stop: () => void stop(s.id).catch(report),
      }),
    );
    for (const s of listDevServers(byLeaf)) {
      // A Live Server's address printed in a terminal is the same server.
      if (s.host === null && out.some((o) => o.port === s.port)) continue;
      out.push({
        key: `${s.host ?? ""}:${s.port}`,
        port: s.port,
        url: s.url,
        label: devServerLabel(s),
        remoteHost: s.host,
      });
    }
    return out;
  }, [byLeaf, live]);
  useEffect(() => {
    const ports = Array.from(
      new Set(entries.filter((e) => !e.remoteHost).map((e) => e.port)),
    );
    void invoke("remote_report_ports", { ports }).catch(() => {});
  }, [entries]);
  const openPreview = useMemo(
    () => (url: string) =>
      onOpenPreview(isRemoteRuntime() ? remoteProxyUrl(url) : url),
    [onOpenPreview],
  );
  return <DevServersMenu entries={entries} onOpenPreview={openPreview} />;
}

export function DevServersMenu({
  entries,
  onOpenPreview,
}: Props & { entries: readonly ServerEntry[] }) {
  if (entries.length === 0) return null;
  const first = entries[0];
  const title =
    entries.length === 1
      ? `Server: ${first.label}`
      : `${entries.length} servers`;

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <button
          type="button"
          title={title}
          aria-label={title}
          className="terax-pill-in flex h-6 shrink-0 items-center gap-1.5 rounded-sm px-1.5 text-[11px] tabular-nums text-muted-foreground outline-none hover:bg-accent hover:text-foreground data-[state=open]:bg-accent data-[state=open]:text-foreground"
        >
          <span className="size-1.5 shrink-0 rounded-full bg-emerald-500" />
          <span>:{first.port}</span>
          {entries.length > 1 ? <span>+{entries.length - 1}</span> : null}
        </button>
      </DropdownMenuTrigger>
      <DropdownMenuContent
        align="start"
        side="top"
        className={cn(
          "max-w-80 min-w-56",
          statusMenuClass,
          statusMenuScrollClass,
        )}
      >
        {entries.map((entry, i) => (
          <Fragment key={entry.key}>
            {i > 0 ? <DropdownMenuSeparator /> : null}
            <ServerItems entry={entry} onOpenPreview={onOpenPreview} />
          </Fragment>
        ))}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

function copy(url: string) {
  void writeClipboardText(url).then(
    () => toast.success("Address copied"),
    (e: unknown) => toast.error(String(e)),
  );
}

function ServerItems({
  entry,
  onOpenPreview,
}: {
  entry: ServerEntry;
  onOpenPreview: (url: string) => void;
}) {
  // A server's localhost is not this machine's: opening it here would show
  // whatever runs on that port locally.
  const remote = entry.remoteHost !== null;
  const why = remote
    ? `Runs on ${entry.remoteHost}; forward the port to open it here`
    : undefined;
  const live = entry.live;
  const shared = (live?.lanUrls.length ?? 0) > 0;
  return (
    <>
      <DropdownMenuLabel className="select-text text-[11px] font-normal text-muted-foreground">
        {entry.label}
      </DropdownMenuLabel>
      <DropdownMenuItem
        disabled={remote}
        title={why}
        onSelect={() => onOpenPreview(entry.url)}
      >
        <HugeiconsIcon icon={Globe02Icon} size={12} strokeWidth={1.75} />
        Open in Preview
      </DropdownMenuItem>
      <DropdownMenuItem
        disabled={remote}
        title={why}
        onSelect={() => void openExternalUrl(entry.url)}
      >
        <HugeiconsIcon icon={LinkSquare02Icon} size={12} strokeWidth={1.75} />
        Open in Browser
      </DropdownMenuItem>
      <DropdownMenuItem onSelect={() => copy(entry.url)}>
        <HugeiconsIcon icon={Copy01Icon} size={12} strokeWidth={1.75} />
        Copy Address
      </DropdownMenuItem>
      {live ? (
        <>
          {live.lanUrls.map((url) => (
            <DropdownMenuItem key={url} onSelect={() => copy(url)}>
              <HugeiconsIcon icon={Copy01Icon} size={12} strokeWidth={1.75} />
              Copy {url.replace(/^https?:\/\//, "").replace(/\/$/, "")}
            </DropdownMenuItem>
          ))}
          <DropdownMenuItem
            title={
              shared
                ? "Only this computer can open it again"
                : "Other devices on your network can open it, by IP address"
            }
            onSelect={() => live.onShare(!shared)}
          >
            <HugeiconsIcon icon={Wifi01Icon} size={12} strokeWidth={1.75} />
            {shared ? "Stop Sharing on Network" : "Share on Local Network"}
          </DropdownMenuItem>
          <DropdownMenuItem variant="destructive" onSelect={live.onStop}>
            <HugeiconsIcon icon={StopCircleIcon} size={12} strokeWidth={1.75} />
            Stop Server
          </DropdownMenuItem>
        </>
      ) : null}
    </>
  );
}
