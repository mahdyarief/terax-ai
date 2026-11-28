import { CopyButton } from "@/components/ui/copy-button";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import { Switch } from "@/components/ui/switch";
import { openSettingsWindow } from "@/modules/settings/openSettingsWindow";
import { Wifi01Icon, WifiOff01Icon } from "@hugeicons/core-free-icons";
import { HugeiconsIcon } from "@hugeicons/react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useEffect, useState } from "react";
import { readRemoteOptions, writeRemoteOptions } from "./lastOptions";

type Info = {
  running: boolean;
  viewers: number;
  https: boolean;
  urls: string[];
};

/** How often the viewer count is read while the listener is on. */
const POLL_MS = 10_000;

const BIND_LABEL = {
  loopback: "this computer only",
  tailscale: "Tailscale",
  all: "all interfaces",
} as const;

/**
 * The status bar's remote access switch: whether a browser can reach this
 * machine, how many streams are open, and an on/off toggle that reuses the
 * last bind and port chosen in Settings.
 */
export function RemoteAccessPill() {
  const [info, setInfo] = useState<Info | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    const read = () => {
      void invoke<Info>("remote_access_status")
        .then((next) => {
          if (live) setInfo(next);
        })
        .catch(() => undefined);
    };
    read();
    const timer = setInterval(read, POLL_MS);
    const unlisten = listen<Info>("terax:remote-access", (event) => {
      setInfo(event.payload);
    });
    return () => {
      live = false;
      clearInterval(timer);
      void unlisten.then((stop) => stop());
    };
  }, []);

  const running = info?.running ?? false;
  const viewers = info?.viewers ?? 0;
  const options = readRemoteOptions();

  const toggle = async (next: boolean) => {
    setBusy(true);
    setError(null);
    try {
      if (next) {
        const started = await invoke<Info>("remote_access_start", options);
        writeRemoteOptions(options);
        setInfo(started);
      } else {
        await invoke("remote_access_stop");
        setInfo((previous) =>
          previous ? { ...previous, running: false, viewers: 0 } : previous,
        );
      }
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : String(failure));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Popover>
      <PopoverTrigger asChild>
        <button
          type="button"
          aria-label={running ? "Remote access on" : "Remote access off"}
          className={`flex shrink-0 items-center gap-1 rounded-full px-2 py-0.5 text-[11px] font-medium ${
            running
              ? "bg-emerald-500/15 text-emerald-700 dark:text-emerald-400"
              : "text-muted-foreground hover:bg-accent"
          }`}
        >
          <HugeiconsIcon
            icon={running ? Wifi01Icon : WifiOff01Icon}
            size={12}
            strokeWidth={2}
          />
          Remote
          {running && viewers > 0 ? ` · ${viewers}` : ""}
        </button>
      </PopoverTrigger>
      <PopoverContent side="top" align="end" className="w-80 gap-3 text-xs">
        <div className="flex items-center justify-between gap-3">
          <div className="min-w-0">
            <div className="text-sm font-medium">Remote access</div>
            <div className="text-muted-foreground">
              {running
                ? viewers === 0
                  ? "On. No browser is connected."
                  : `On. ${viewers} browser ${viewers === 1 ? "stream" : "streams"} open.`
                : `Off. Turns on for ${BIND_LABEL[options.bind]}, port ${options.port}.`}
            </div>
          </div>
          <Switch
            checked={running}
            disabled={busy}
            onCheckedChange={(next) => void toggle(next)}
            aria-label="Remote access"
          />
        </div>
        {running && info?.urls[0] ? (
          <div className="flex items-center gap-1 rounded-lg bg-muted px-2 py-1 font-mono">
            <span className="min-w-0 flex-1 truncate">{info.urls[0]}</span>
            <CopyButton text={info.urls[0]} />
          </div>
        ) : null}
        {error ? <div className="text-destructive">{error}</div> : null}
        <button
          type="button"
          onClick={() => void openSettingsWindow("remote")}
          className="self-start text-muted-foreground underline-offset-2 hover:text-foreground hover:underline"
        >
          Bind, pairing and devices in Settings…
        </button>
      </PopoverContent>
    </Popover>
  );
}
