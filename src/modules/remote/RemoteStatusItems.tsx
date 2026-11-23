import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { cn } from "@/lib/utils";
import { toast } from "sonner";
import {
  describeState,
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

/** One status bar item per host that is not idle. */
export function RemoteStatusItems() {
  const hosts = useRemoteStore((s) => s.hosts);
  const shown = Object.entries(hosts)
    .filter(([, state]) => state.state !== "idle")
    .sort(([a], [b]) => a.localeCompare(b));
  return (
    <>
      {shown.map(([host, state]) => (
        <RemoteStatusItem key={host} host={host} state={state} />
      ))}
    </>
  );
}

function RemoteStatusItem({ host, state }: { host: string; state: RemoteState }) {
  const connect = useRemoteStore((s) => s.connect);
  const disconnect = useRemoteStore((s) => s.disconnect);
  const showLog = useRemoteStore((s) => s.showLog);
  const report = (e: unknown) =>
    toast.error(`SSH: ${host}`, { description: String(e) });
  const { detail } = describeState(state);
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <button
          type="button"
          className="flex h-6 shrink-0 items-center rounded-sm px-1.5 outline-none hover:bg-accent data-[state=open]:bg-accent"
        >
          <RemoteStatusLabel host={host} state={state} />
        </button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start" className="max-w-80 min-w-48">
        <DropdownMenuLabel className="text-[11px] font-normal text-muted-foreground">
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
        <DropdownMenuItem onSelect={() => showLog(host)}>Show log</DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

/** The item's face: a dot for the tone, the host, the state. */
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
      <span className={cn(tone === "error" && "text-red-600 dark:text-red-400")}>
        {label}
      </span>
    </span>
  );
}
