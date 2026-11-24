import { Button } from "@/components/ui/button";
import { CopyButton } from "@/components/ui/copy-button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { useCallback, useEffect, useState } from "react";
import { fetchRemoteLog, useRemoteStore } from "./connections";

/** The last lines of a host's connection log: ssh's stderr and our steps. */
export function RemoteLogDialog() {
  const host = useRemoteStore((s) => s.logHost);
  const showLog = useRemoteStore((s) => s.showLog);
  if (!host) return null;
  return <Log key={host} host={host} onClose={() => showLog(null)} />;
}

function Log({ host, onClose }: { host: string; onClose: () => void }) {
  const [lines, setLines] = useState<string[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const refresh = useCallback(() => {
    fetchRemoteLog(host)
      .then((l) => {
        setLines(l);
        setError(null);
      })
      .catch((e) => setError(String(e)));
  }, [host]);
  useEffect(refresh, [refresh]);

  return (
    <Dialog open onOpenChange={(next) => !next && onClose()}>
      <DialogContent className="sm:max-w-2xl">
        <DialogHeader>
          <DialogTitle>SSH log: {host}</DialogTitle>
          <DialogDescription>
            The last 500 lines from ssh and the connection. Answers to prompts
            are never logged.
          </DialogDescription>
        </DialogHeader>
        <RemoteLogLines lines={lines} error={error} />
        <DialogFooter>
          <CopyButton
            label="Copy log"
            size="default"
            text={error ?? `SSH log: ${host}\n${(lines ?? []).join("\n")}`}
          />
          <Button variant="outline" onClick={refresh}>
            Refresh
          </Button>
          <Button onClick={onClose}>Close</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

export function RemoteLogLines({
  lines,
  error,
}: {
  lines: string[] | null;
  error: string | null;
}) {
  const text = error
    ? error
    : lines === null
      ? "Loading…"
      : lines.length === 0
        ? "Nothing logged yet."
        : lines.join("\n");
  return (
    <pre className="select-text max-h-[50vh] overflow-auto whitespace-pre-wrap break-all rounded-md bg-muted p-3 font-mono text-[11px] leading-relaxed">
      {text}
    </pre>
  );
}
