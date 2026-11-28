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
import { FolderListing, type Listing } from "@/modules/remote/RemoteFolderPicker";
import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useRef, useState } from "react";
import {
  hostChildPath,
  hostParentPath,
  isHostRoot,
  useBrowserFolderPicker,
} from "./folderPickerStore";
import { currentWorkspaceEnv } from "./env";

/** The browser's "Open Folder": browses the host's folders in the app. */
export function BrowserFolderPicker() {
  const request = useBrowserFolderPicker((s) => s.request);
  const finish = useBrowserFolderPicker((s) => s.finish);
  if (!request) return null;
  return (
    <Picker
      key={request.defaultPath ?? ""}
      defaultPath={request.defaultPath}
      onDone={finish}
    />
  );
}

function Picker({
  defaultPath,
  onDone,
}: {
  defaultPath: string | null;
  onDone: (path: string | null) => void;
}) {
  const [listing, setListing] = useState<Listing>({
    phase: "loading",
    path: defaultPath,
  });
  const [typed, setTyped] = useState(defaultPath ?? "");
  const seq = useRef(0);

  const browse = useCallback((path: string) => {
    const mine = ++seq.current;
    setListing({ phase: "loading", path });
    invoke<string[]>("list_subdirs", {
      path,
      showHidden: false,
      workspace: currentWorkspaceEnv(),
    })
      .then((dirs) => {
        if (mine !== seq.current) return;
        setListing({ phase: "ready", browse: { path, dirs } });
        setTyped(path);
      })
      .catch((e) => {
        if (mine !== seq.current) return;
        setListing({ phase: "error", message: String(e), path });
      });
  }, []);

  useEffect(() => {
    browse(defaultPath || "/");
  }, [browse, defaultPath]);

  const ready = listing.phase === "ready" ? listing.browse : null;
  return (
    <Dialog open onOpenChange={(next) => !next && onDone(null)}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>Open Folder</DialogTitle>
          <DialogDescription>
            Pick a folder on the machine Terax runs on.
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
            aria-label="Folder path"
          />
          <Button type="submit" variant="outline">
            Go
          </Button>
        </form>
        <FolderListing
          listing={listing}
          state={undefined}
          onUp={(path) => browse(hostParentPath(path))}
          onEnter={(path) => browse(path)}
          onRetry={() =>
            browse(listing.phase === "error" ? listing.path ?? "/" : "/")
          }
          childPath={hostChildPath}
          isRoot={isHostRoot}
        />
        <DialogFooter>
          <Button variant="outline" onClick={() => onDone(null)}>
            Cancel
          </Button>
          <Button onClick={() => ready && onDone(ready.path)} disabled={!ready}>
            Open
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
