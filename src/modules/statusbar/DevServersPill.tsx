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
import { cn } from "@/lib/utils";
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
} from "@hugeicons/core-free-icons";
import { HugeiconsIcon } from "@hugeicons/react";
import { Fragment, useMemo } from "react";
import { toast } from "sonner";

type Props = {
  /** Opens the address in a Terax preview tab. */
  onOpenPreview: (url: string) => void;
};

/** How a server reads in the menu: its address, then what usually runs there. */
export function devServerLabel(s: DevServer): string {
  const known = portPresetLabel(s.port);
  const where = s.host ? ` on ${s.host}` : "";
  return `${s.url.replace(/^https?:\/\//, "").replace(/\/$/, "")}${known ? ` · ${known}` : ""}${where}`;
}

/**
 * The local servers the terminals printed the address of (Vite's `Local:`
 * line, uvicorn's `Running on`), with actions to open each one. Hidden until
 * a terminal prints one; an address goes away when its command ends.
 */
export function DevServersPill({ onOpenPreview }: Props) {
  const byLeaf = useDevServerStore((s) => s.byLeaf);
  const servers = useMemo(() => listDevServers(byLeaf), [byLeaf]);
  return <DevServersMenu servers={servers} onOpenPreview={onOpenPreview} />;
}

export function DevServersMenu({
  servers,
  onOpenPreview,
}: Props & { servers: readonly DevServer[] }) {
  if (servers.length === 0) return null;
  const first = servers[0];
  const title =
    servers.length === 1
      ? `Dev server: ${devServerLabel(first)}`
      : `${servers.length} dev servers`;

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
          {servers.length > 1 ? <span>+{servers.length - 1}</span> : null}
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
        {servers.map((s, i) => (
          <Fragment key={`${s.host ?? ""}:${s.port}`}>
            {i > 0 ? <DropdownMenuSeparator /> : null}
            <DevServerItems server={s} onOpenPreview={onOpenPreview} />
          </Fragment>
        ))}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

function DevServerItems({
  server,
  onOpenPreview,
}: {
  server: DevServer;
  onOpenPreview: (url: string) => void;
}) {
  // A server's localhost is not this machine's: opening it here would show
  // whatever runs on that port locally.
  const remote = server.host !== null;
  const why = remote
    ? `Runs on ${server.host}; forward the port to open it here`
    : undefined;
  return (
    <>
      <DropdownMenuLabel className="select-text text-[11px] font-normal text-muted-foreground">
        {devServerLabel(server)}
      </DropdownMenuLabel>
      <DropdownMenuItem
        disabled={remote}
        title={why}
        onSelect={() => onOpenPreview(server.url)}
      >
        <HugeiconsIcon icon={Globe02Icon} size={12} strokeWidth={1.75} />
        Open in Preview
      </DropdownMenuItem>
      <DropdownMenuItem
        disabled={remote}
        title={why}
        onSelect={() => void openExternalUrl(server.url)}
      >
        <HugeiconsIcon icon={LinkSquare02Icon} size={12} strokeWidth={1.75} />
        Open in Browser
      </DropdownMenuItem>
      <DropdownMenuItem
        onSelect={() =>
          void writeClipboardText(server.url).then(
            () => toast.success("Address copied"),
            (e: unknown) => toast.error(String(e)),
          )
        }
      >
        <HugeiconsIcon icon={Copy01Icon} size={12} strokeWidth={1.75} />
        Copy Address
      </DropdownMenuItem>
    </>
  );
}
