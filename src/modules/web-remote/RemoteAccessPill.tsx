import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { openSettingsWindow } from "@/modules/settings/openSettingsWindow";
import { Wifi01Icon } from "@hugeicons/core-free-icons";
import { HugeiconsIcon } from "@hugeicons/react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useEffect, useState } from "react";

type Info = {
  running: boolean;
  viewers: number;
  https: boolean;
  urls: string[];
};

/** How often the viewer count is read while the listener is on. */
const POLL_MS = 10_000;

/**
 * The status bar's sign that a browser can reach this machine, with how many
 * streams are open. Hidden while remote access is off.
 */
export function RemoteAccessPill() {
  const [info, setInfo] = useState<Info | null>(null);

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

  if (!info?.running) return null;
  const viewers = info.viewers;

  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <button
          type="button"
          onClick={() => void openSettingsWindow("remote")}
          className="flex shrink-0 items-center gap-1 rounded-full bg-emerald-500/15 px-2 py-0.5 text-[11px] font-medium text-emerald-700 dark:text-emerald-400"
        >
          <HugeiconsIcon icon={Wifi01Icon} size={12} strokeWidth={2} />
          Remote
          {viewers > 0 ? ` · ${viewers}` : ""}
        </button>
      </TooltipTrigger>
      <TooltipContent
        side="top"
        className="max-w-64 text-[11px] leading-relaxed"
      >
        {viewers === 0
          ? "Remote access is on; no browser is connected."
          : `${viewers} browser ${viewers === 1 ? "stream is" : "streams are"} open.`}
        {info.urls[0] ? ` ${info.urls[0]}` : ""}
      </TooltipContent>
    </Tooltip>
  );
}
