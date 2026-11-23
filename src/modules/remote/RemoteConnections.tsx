import { listen } from "@tauri-apps/api/event";
import { useEffect } from "react";
import { ConnectSshDialog } from "./ConnectSshDialog";
import { parseSnapshot, STATE_EVENT, useRemoteStore } from "./connections";
import { RemoteLogDialog } from "./RemoteLogDialog";

/** Follows the app's connection states and hosts the picker and the log. */
export function RemoteConnections() {
  useEffect(() => {
    let unlisten: (() => void) | null = null;
    let live = true;
    void listen(STATE_EVENT, (event) => {
      const snapshot = parseSnapshot(event.payload);
      if (snapshot) useRemoteStore.getState().apply(snapshot);
    }).then((stop) => {
      if (live) unlisten = stop;
      else stop();
    });
    // Connections made before this window loaded.
    useRemoteStore
      .getState()
      .load()
      .catch(() => {});
    return () => {
      live = false;
      unlisten?.();
    };
  }, []);
  return (
    <>
      <ConnectSshDialog />
      <RemoteLogDialog />
    </>
  );
}
