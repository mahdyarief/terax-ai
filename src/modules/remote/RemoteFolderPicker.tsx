import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { cn } from "@/lib/utils";
import { ArrowUp01Icon, Folder01Icon } from "@hugeicons/core-free-icons";
import { HugeiconsIcon } from "@hugeicons/react";
import { useCallback, useEffect, useRef, useState } from "react";
import { toast } from "sonner";
import {
  type Browse,
  browseRemoteDirs,
  childPath,
  describeState,
  ensureConnected,
  fetchRemoteHome,
  parentPath,
  type RemoteState,
  useRemoteStore,
} from "./connections";

/** "Open Folder" on a server: connects, then browses from the home folder. */
export function RemoteFolderPicker() {
  const host = useRemoteStore((s) => s.folderPickerHost);
  const close = useRemoteStore((s) => s.openFolderPicker);
  if (!host) return null;
  return <Picker key={host} host={host} onClose={() => close(null)} />;
}

export type Listing =
  | { phase: "connecting" }
  | { phase: "loading"; path: string | null }
  | { phase: "ready"; browse: Browse }
  | { phase: "error"; message: string; path: string | null };

function Picker({ host, onClose }: { host: string; onClose: () => void }) {
  const state = useRemoteStore((s) => s.hosts[host]);
  const handler = useRemoteStore((s) => s.folderHandler);
  const [listing, setListing] = useState<Listing>({ phase: "connecting" });
  const [typed, setTyped] = useState("");
  const [opening, setOpening] = useState(false);
  // Only the latest request may update the listing.
  const seq = useRef(0);

  const browse = useCallback(
    (path: string | null) => {
      const mine = ++seq.current;
      setListing({ phase: "loading", path });
      const target = path === null ? fetchRemoteHome(host) : Promise.resolve(path);
      target
        .then((p) => browseRemoteDirs(host, p))
        .then((result) => {
          if (mine !== seq.current) return;
          setListing({ phase: "ready", browse: result });
          setTyped(result.path);
        })
        .catch((e) => {
          if (mine !== seq.current) return;
          setListing({ phase: "error", message: String(e), path });
        });
    },
    [host],
  );

  useEffect(() => {
    let live = true;
    ensureConnected(host)
      .then(() => live && browse(null))
      .catch(
        (e) =>
          live &&
          setListing({ phase: "error", message: String(e.message ?? e), path: null }),
      );
    return () => {
      live = false;
    };
  }, [host, browse]);

  const open = async () => {
    if (listing.phase !== "ready" || !handler) return;
    setOpening(true);
    try {
      await handler(host, listing.browse.path);
      onClose();
    } catch (e) {
      toast.error(`SSH: ${host}`, { description: String(e) });
      setOpening(false);
    }
  };

  return (
    <Dialog open onOpenChange={(next) => !next && onClose()}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>Open Folder on {host}</DialogTitle>
          <DialogDescription>
            Pick a folder on the server. It opens as a new space.
          </DialogDescription>
        </DialogHeader>
        <form
          className="flex gap-2"
          onSubmit={(e) => {
            e.preventDefault();
            const path = typed.trim();
            if (path) browse(path);
          }}
        >
          <Input
            value={typed}
            onChange={(e) => setTyped(e.target.value)}
            placeholder="/home/me/project"
            className="font-mono text-xs"
            disabled={listing.phase === "connecting"}
            aria-label="Folder path"
          />
          <Button type="submit" variant="outline" disabled={listing.phase === "connecting"}>
            Go
          </Button>
        </form>
        <FolderListing
          listing={listing}
          state={state}
          onUp={(path) => browse(parentPath(path))}
          onEnter={(path) => browse(path)}
          onRetry={() => browse(listing.phase === "error" ? listing.path : null)}
        />
        <DialogFooter>
          <Button variant="outline" onClick={onClose}>
            Cancel
          </Button>
          <Button
            onClick={() => void open()}
            disabled={listing.phase !== "ready" || opening || !handler}
          >
            {opening ? "Opening…" : "Open"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

/** The picker's body for each state; pure, so its states are tested. */
export function FolderListing({
  listing,
  state,
  onUp,
  onEnter,
  onRetry,
}: {
  listing: Listing;
  state: RemoteState | undefined;
  onUp: (path: string) => void;
  onEnter: (path: string) => void;
  onRetry: () => void;
}) {
  const frame =
    "h-72 overflow-auto rounded-md border bg-muted/30 p-1 text-[12px]";
  if (listing.phase === "connecting") {
    const label = state ? describeState(state).label : "Connecting…";
    return (
      <div className={cn(frame, "grid place-items-center text-muted-foreground")}>
        {label}
      </div>
    );
  }
  if (listing.phase === "loading") {
    return (
      <div className={cn(frame, "grid place-items-center text-muted-foreground")}>
        Loading…
      </div>
    );
  }
  if (listing.phase === "error") {
    return (
      <div className={cn(frame, "flex flex-col items-center justify-center gap-2 p-4 text-center")}>
        <span className="text-red-600 dark:text-red-400">{listing.message}</span>
        <Button size="sm" variant="outline" onClick={onRetry}>
          Try again
        </Button>
      </div>
    );
  }
  const { browse } = listing;
  return (
    <div className={frame} role="listbox" aria-label={browse.path}>
      <div className="truncate px-2 py-1 font-mono text-muted-foreground" title={browse.path}>
        {browse.path}
      </div>
      {browse.path !== "/" ? (
        <Row icon={ArrowUp01Icon} label=".." onClick={() => onUp(browse.path)} />
      ) : null}
      {browse.dirs.length === 0 ? (
        <div className="px-2 py-1 text-muted-foreground">No folders here.</div>
      ) : (
        browse.dirs.map((name) => (
          <Row
            key={name}
            icon={Folder01Icon}
            label={name}
            onClick={() => onEnter(childPath(browse.path, name))}
          />
        ))
      )}
    </div>
  );
}

function Row({
  icon,
  label,
  onClick,
}: {
  icon: typeof Folder01Icon;
  label: string;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="flex w-full items-center gap-2 rounded-sm px-2 py-1 text-left hover:bg-accent"
    >
      <HugeiconsIcon icon={icon} size={14} strokeWidth={1.75} />
      <span className="truncate">{label}</span>
    </button>
  );
}
