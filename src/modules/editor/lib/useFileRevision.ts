import {
  listenFsChanged,
  parentDir,
  watchAdd,
  watchRemove,
} from "@/modules/explorer/lib/watch";
import {
  createCoalescer,
  touchesPath,
} from "@/modules/markdown/lib/liveReload";
import { currentWorkspaceEnv } from "@/modules/workspace";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { useEffect, useState } from "react";

/**
 * A counter that goes up each time `path` changes on disk (or the app writes
 * it). Previews that parse a file themselves list it as an effect dependency
 * so they re-read; a same-size rewrite is otherwise invisible to them.
 */
export function useFileRevision(path: string): number {
  const [revision, setRevision] = useState(0);

  useEffect(() => {
    let alive = true;
    let watching = false;
    const workspace = currentWorkspaceEnv();
    // The watcher reports canonical paths; the tab may hold an alias spelling.
    let canonical: string | null = null;
    const coalescer = createCoalescer(() => {
      if (alive) setRevision((value) => value + 1);
    });
    const matches = (paths: readonly string[]) =>
      touchesPath(paths, path) ||
      (canonical !== null && touchesPath(paths, canonical));
    const onTouched = (paths: readonly string[]) => {
      if (alive && watching && matches(paths)) coalescer.poke();
    };

    const dir = parentDir(path);
    const unlisten: Array<() => void> = [];
    const track = (pending: Promise<() => void>) =>
      pending.then((un) => (alive ? unlisten.push(un) : un()));

    void Promise.allSettled([
      track(listenFsChanged(onTouched)),
      track(
        getCurrentWebviewWindow().listen<{ path: string }>(
          "fs:file-written",
          (e) => onTouched([e.payload.path]),
        ),
      ),
    ]).then(() => {
      if (!alive) return;
      watching = true;
      watchAdd([dir], [path], workspace);
    });
    invoke<string>("fs_canonicalize", { path, workspace })
      .then((c) => {
        if (alive) canonical = c;
      })
      .catch(() => {});

    return () => {
      alive = false;
      coalescer.cancel();
      for (const un of unlisten) un();
      if (watching) watchRemove([dir], [path], workspace);
    };
  }, [path]);

  return revision;
}
