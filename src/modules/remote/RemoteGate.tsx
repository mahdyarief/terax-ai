import { Button } from "@/components/ui/button";
import { CopyButton } from "@/components/ui/copy-button";
import type { WorkspaceEnv } from "@/modules/workspace";
import { invoke } from "@tauri-apps/api/core";
import { Fragment, type ReactNode, useEffect, useState } from "react";
import {
  describeState,
  ensureConnected,
  type RemoteState,
  useRemoteStore,
} from "./connections";

/**
 * Shows `children` (the explorer) on a server only while it is connected
 * and the space's folder is authorized there; otherwise the state, the
 * reason and Connect. Each new connection remounts the children, so they
 * reload what may have changed while it was down.
 */
export function RemoteGate({
  env,
  root,
  children,
}: {
  env: WorkspaceEnv;
  root: string | null;
  children: ReactNode;
}) {
  if (env.kind !== "ssh") return <>{children}</>;
  return (
    <Gate host={env.host} env={env} root={root}>
      {children}
    </Gate>
  );
}

function Gate({
  host,
  env,
  root,
  children,
}: {
  host: string;
  env: WorkspaceEnv;
  root: string | null;
  children: ReactNode;
}) {
  const state = useRemoteStore((s) => s.hosts[host]);
  const epoch = useRemoteStore((s) => s.readyEpochs[host] ?? 0);
  const [authorized, setAuthorized] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const key = `${epoch}:${root ?? ""}`;
  const ready = state?.state === "ready";

  useEffect(() => {
    if (!ready || !root) return;
    let live = true;
    invoke<string>("workspace_authorize", { path: root, workspace: env })
      .then(() => {
        if (live) {
          setAuthorized(key);
          setError(null);
        }
      })
      .catch((e) => live && setError(String(e)));
    return () => {
      live = false;
    };
  }, [ready, root, env, key]);

  if (ready && authorized === key) {
    return <Fragment key={key}>{children}</Fragment>;
  }
  return (
    <RemoteGateNotice
      host={host}
      state={state}
      error={error}
      onConnect={() => {
        setError(null);
        ensureConnected(host).catch((e) => setError(String(e.message ?? e)));
      }}
    />
  );
}

/** What the explorer shows while the server is not usable; pure. */
export function RemoteGateNotice({
  host,
  state,
  error,
  onConnect,
}: {
  host: string;
  state: RemoteState | undefined;
  error: string | null;
  onConnect: () => void;
}) {
  const busy =
    state?.state === "connecting" ||
    state?.state === "installing" ||
    state?.state === "handshaking";
  const { label, detail } = state
    ? describeState(state)
    : { label: "Not connected", detail: undefined };
  const reason = error ?? detail;
  const showLog = useRemoteStore((s) => s.showLog);
  return (
    <div className="flex h-full flex-col items-center justify-center gap-2 p-4 text-center text-[12px]">
      <div className="font-medium">SSH: {host}</div>
      <div className="text-muted-foreground">{state?.state === "ready" ? "Opening folder…" : label}</div>
      {reason ? (
        <div className="select-text max-w-full break-words text-red-600 dark:text-red-400">
          {reason}
        </div>
      ) : null}
      <div className="flex flex-wrap items-center justify-center gap-2">
        {busy || state?.state === "ready" ? null : (
          <Button size="sm" onClick={onConnect}>
            Connect
          </Button>
        )}
        {reason ? (
          <CopyButton text={`SSH: ${host}\n${label}\n${reason}`} />
        ) : null}
        <Button size="sm" variant="outline" onClick={() => showLog(host)}>
          Show log
        </Button>
      </div>
    </div>
  );
}
