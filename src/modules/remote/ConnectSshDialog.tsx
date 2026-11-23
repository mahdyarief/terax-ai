import {
  Command,
  CommandDialog,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from "@/components/ui/command";
import { Clock01Icon, ServerStack03Icon } from "@hugeicons/core-free-icons";
import { HugeiconsIcon } from "@hugeicons/react";
import { useEffect, useMemo, useState } from "react";
import { toast } from "sonner";
import {
  fetchSshHosts,
  type PickerEntry,
  pickerEntries,
  useRemoteStore,
} from "./connections";

const GROUPS: { source: PickerEntry["source"]; heading: string }[] = [
  { source: "recent", heading: "Recent" },
  { source: "config", heading: "~/.ssh/config" },
  { source: "typed", heading: "Connect to" },
];

/** "Connect to SSH Host…": recent hosts, the config's, or a typed one. */
export function ConnectSshDialog() {
  const open = useRemoteStore((s) => s.pickerOpen);
  const setOpen = useRemoteStore((s) => s.setPickerOpen);
  if (!open) return null;
  return <Picker onClose={() => setOpen(false)} />;
}

function Picker({ onClose }: { onClose: () => void }) {
  const recents = useRemoteStore((s) => s.recents);
  const connect = useRemoteStore((s) => s.connect);
  const [configHosts, setConfigHosts] = useState<string[]>([]);
  const [query, setQuery] = useState("");

  useEffect(() => {
    let live = true;
    fetchSshHosts()
      .then((hosts) => live && setConfigHosts(hosts))
      .catch(() => {});
    return () => {
      live = false;
    };
  }, []);

  const entries = useMemo(
    () => pickerEntries(recents, configHosts, query),
    [recents, configHosts, query],
  );

  const pick = (host: string) => {
    onClose();
    connect(host).catch((e) => toast.error(`SSH: ${host}`, { description: String(e) }));
  };

  return (
    <CommandDialog
      open
      onOpenChange={(next) => !next && onClose()}
      title="Connect to SSH Host"
      description="Pick a host from ~/.ssh/config or type user@host"
    >
      <Command shouldFilter={false}>
        <CommandInput
          value={query}
          onValueChange={setQuery}
          placeholder="Host from ~/.ssh/config, or user@host"
        />
        <CommandList>
          <CommandEmpty>
            No hosts in ~/.ssh/config. Type user@host to connect.
          </CommandEmpty>
          {GROUPS.map(({ source, heading }) => {
            const rows = entries.filter((e) => e.source === source);
            if (rows.length === 0) return null;
            return (
              <CommandGroup key={source} heading={heading}>
                {rows.map((entry) => (
                  <CommandItem
                    key={`${source}:${entry.host}`}
                    value={`${source}:${entry.host}`}
                    onSelect={() => pick(entry.host)}
                  >
                    <HugeiconsIcon
                      icon={source === "recent" ? Clock01Icon : ServerStack03Icon}
                      size={14}
                      strokeWidth={1.75}
                    />
                    <span className="truncate">{entry.host}</span>
                  </CommandItem>
                ))}
              </CommandGroup>
            );
          })}
        </CommandList>
      </Command>
    </CommandDialog>
  );
}
