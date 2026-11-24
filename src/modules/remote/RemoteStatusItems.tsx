import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuSub,
  DropdownMenuSubContent,
  DropdownMenuSubTrigger,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { statusMenuClass, statusMenuScrollClass } from "@/components/ui/menu";
import { cn } from "@/lib/utils";
import { ServerStack03Icon } from "@hugeicons/core-free-icons";
import { HugeiconsIcon } from "@hugeicons/react";
import { toast } from "sonner";
import {
  describeState,
  openSshPicker,
  type RemoteState,
  type StateTone,
  useRemoteStore,
} from "./connections";

const TONE_DOT: Record<StateTone, string> = {
  busy: "bg-sky-500 animate-pulse",
  ok: "bg-emerald-500",
  warn: "bg-amber-500",
  error: "bg-red-500",
};

/** The tone that needs the most attention wins the status bar's face. */
const TONE_RANK: Record<StateTone, number> = {
  ok: 0,
  busy: 1,
  warn: 2,
  error: 3,
};

/** The hosts the SSH menu lists (every one that is not idle), sorted, and
 * the tone of the item that stands for all of them. */
export function sshSummary(hosts: Record<string, RemoteState>): {
  shown: [string, RemoteState][];
  tone: StateTone | null;
} {
  const shown = Object.entries(hosts)
    .filter(([, state]) => state.state !== "idle")
    .sort(([a], [b]) => a.localeCompare(b));
  let tone: StateTone | null = null;
  for (const [, state] of shown) {
    const next = describeState(state).tone;
    if (tone === null || TONE_RANK[next] > TONE_RANK[tone]) tone = next;
  }
  return { shown, tone };
}

/**
 * One status bar item for SSH, beside the Windows/WSL one: its menu lists
 * every connection (its state, Reconnect, Disconnect, Show log) and
 * connects to a new host.
 */
export function RemoteStatusItems() {
  const hosts = useRemoteStore((s) => s.hosts);
  const { shown, tone } = sshSummary(hosts);
  const only = shown.length === 1 ? shown[0] : null;
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <button
          type="button"
          title="SSH connections"
          className="flex h-6 shrink-0 items-center gap-1 rounded-sm px-1.5 text-[11px] text-muted-foreground outline-none hover:bg-accent hover:text-foreground data-[state=open]:bg-accent data-[state=open]:text-foreground"
        >
          {only ? (
            <RemoteStatusLabel host={only[0]} state={only[1]} />
          ) : (
            <span className="flex items-center gap-1.5">
              {tone ? (
                <span
                  data-tone={tone}
                  className={cn(
                    "size-1.5 shrink-0 rounded-full",
                    TONE_DOT[tone],
                  )}
                />
              ) : (
                <HugeiconsIcon
                  icon={ServerStack03Icon}
                  size={12}
                  strokeWidth={1.75}
                />
              )}
              <span>
                {shown.length === 0 ? "SSH" : `SSH: ${shown.length} hosts`}
              </span>
            </span>
          )}
        </button>
      </DropdownMenuTrigger>
      <DropdownMenuContent
        align="start"
        className={cn(
          "max-w-80 min-w-56",
          statusMenuClass,
          statusMenuScrollClass,
        )}
      >
        {shown.length === 0 ? (
          <DropdownMenuItem disabled>No SSH connections</DropdownMenuItem>
        ) : (
          shown.map(([host, state]) => (
            <RemoteHostRow key={host} host={host} state={state} />
          ))
        )}
        <DropdownMenuSeparator />
        <DropdownMenuItem onSelect={openSshPicker}>
          <HugeiconsIcon
            icon={ServerStack03Icon}
            size={12}
            strokeWidth={1.75}
          />
          Connect to SSH Host...
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

function RemoteHostRow({ host, state }: { host: string; state: RemoteState }) {
  const connect = useRemoteStore((s) => s.connect);
  const disconnect = useRemoteStore((s) => s.disconnect);
  const showLog = useRemoteStore((s) => s.showLog);
  const report = (e: unknown) =>
    toast.error(`SSH: ${host}`, { description: String(e) });
  const { detail } = describeState(state);
  return (
    <DropdownMenuSub>
      <DropdownMenuSubTrigger>
        <RemoteStatusLabel host={host} state={state} />
      </DropdownMenuSubTrigger>
      <DropdownMenuSubContent
        className={cn(
          "max-w-80 min-w-48",
          statusMenuClass,
          statusMenuScrollClass,
        )}
      >
        <DropdownMenuLabel className="select-text text-[11px] font-normal text-muted-foreground">
          {detail ? `${host}: ${detail}` : host}
        </DropdownMenuLabel>
        <DropdownMenuSeparator />
        <DropdownMenuItem onSelect={() => void connect(host).catch(report)}>
          {state.state === "ready" ? "Reconnect" : "Connect now"}
        </DropdownMenuItem>
        <DropdownMenuItem onSelect={() => void disconnect(host).catch(report)}>
          Disconnect
        </DropdownMenuItem>
        <DropdownMenuSeparator />
        <DropdownMenuItem onSelect={() => showLog(host)}>
          Show log
        </DropdownMenuItem>
      </DropdownMenuSubContent>
    </DropdownMenuSub>
  );
}

/** A connection's face: a dot for the tone, the host, the state. */
export function RemoteStatusLabel({
  host,
  state,
}: {
  host: string;
  state: RemoteState;
}) {
  const { label, tone, detail } = describeState(state);
  return (
    <span
      className="flex min-w-0 items-center gap-1.5 text-[11px] text-muted-foreground"
      title={detail ? `${host}: ${label} ${detail}` : `${host}: ${label}`}
      data-tone={tone}
    >
      <span className={cn("size-1.5 shrink-0 rounded-full", TONE_DOT[tone])} />
      <span className="max-w-28 truncate">SSH: {host}</span>
      <span
        className={cn(tone === "error" && "text-red-600 dark:text-red-400")}
      >
        {label}
      </span>
    </span>
  );
}
