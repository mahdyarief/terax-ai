import { CopyButton } from "@/components/ui/copy-button";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { isRemoteRuntime } from "@/lib/remoteRuntime";
import { SettingRow } from "@/settings/components/SettingRow";
import { SectionHeader } from "@/settings/components/SectionHeader";
import {
  readRemoteOptions,
  writeRemoteOptions,
} from "@/modules/web-remote/lastOptions";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useState } from "react";
import { toast } from "sonner";

type Bind = "loopback" | "tailscale" | "all";

type RemoteInfo = {
  running: boolean;
  bind: Bind | null;
  port: number | null;
  urls: string[];
  viewers: number;
  https: boolean;
  allowPlainLan: boolean;
};

type Grant = { input: boolean; tabs: boolean; app: boolean };

type PairedDevice = {
  id: string;
  name: string;
  grant: Grant;
  createdAt: number;
  lastSeen: number;
};

type Pairing = { code: string; url: string; expiresInSec: number };

type AuditEntry = {
  timestampMs: number;
  deviceId: string;
  deviceName: string;
  cmd: string;
  class: string;
};

const BIND_LABEL: Record<Bind, string> = {
  loopback: "This computer only",
  tailscale: "Tailscale",
  all: "All interfaces",
};

const BIND_HINT: Record<Bind, string> = {
  loopback: "Only this machine can reach it.",
  tailscale: "This machine's 100.x address, encrypted by WireGuard.",
  all: "Every interface. Plain HTTP there is readable by the local network.",
};

function when(ms: number): string {
  if (!ms) return "never";
  return new Date(ms).toLocaleString();
}

/** Device times come from the store in seconds since the epoch. */
function whenSeconds(seconds: number): string {
  return when(seconds * 1000);
}

/**
 * Remote access: the listener, its pairings and the devices that may watch —
 * or type — into this machine's terminals.
 */
/**
 * Grants, revoking and the listener are managed on the desktop only: the
 * bridge refuses those commands from a browser, so a paired device cannot
 * widen its own access. A browser may list devices and pair new ones.
 */
export function RemoteSection() {
  return isRemoteRuntime() ? <RemoteSectionInBrowser /> : <DesktopRemoteSection />;
}

/** Seen within this long counts as connected now. */
const CONNECTED_MS = 2 * 60 * 1000;

export function grantSummary(grant: Grant): string {
  const parts = [
    grant.input ? "type" : null,
    grant.tabs ? "tabs" : null,
    grant.app ? "full app" : null,
  ].filter(Boolean);
  return parts.length ? parts.join(", ") : "watch only";
}

/**
 * In a browser: the paired devices and a pairing code for another one.
 * Grants, revoking and the listener stay on the machine itself, so a paired
 * device cannot widen its own access; a device paired from here starts
 * watch-only.
 */
function RemoteSectionInBrowser() {
  const [devices, setDevices] = useState<PairedDevice[]>([]);
  const [me, setMe] = useState<string | null>(null);
  const [pairing, setPairing] = useState<Pairing | null>(null);
  const [busy, setBusy] = useState(false);

  const refresh = useCallback(() => {
    void invoke<PairedDevice[]>("remote_devices_list")
      .then(setDevices)
      .catch(() => undefined);
  }, []);

  useEffect(() => {
    refresh();
    void fetch("/api/me")
      .then((r) => (r.ok ? r.json() : null))
      .then((info: { id?: string } | null) => setMe(info?.id ?? null))
      .catch(() => undefined);
    const timer = window.setInterval(refresh, 15_000);
    return () => window.clearInterval(timer);
  }, [refresh]);

  const pair = async () => {
    setBusy(true);
    try {
      setPairing(await invoke<Pairing>("remote_pairing_start"));
      refresh();
    } catch (e) {
      toast.error("Could not create a pairing code", { description: String(e) });
    } finally {
      setBusy(false);
    }
  };

  const now = Date.now();
  return (
    <div className="flex flex-col gap-6">
      <SectionHeader
        title="Remote access"
        description="Pair another device and see which devices are paired. Grants, revoking and the listener are managed in Terax on the machine itself."
      />

      <div className="flex flex-col gap-2">
        <div className="flex items-center justify-between">
          <Label>Pair a device</Label>
          <Button
            size="sm"
            variant="outline"
            disabled={busy}
            onClick={() => void pair()}
          >
            {pairing ? "New code" : "Pair a device"}
          </Button>
        </div>
        {pairing ? (
          <SettingRow
            title={pairing.code}
            description={`Expires in ${Math.round(pairing.expiresInSec / 60)} minutes. Open the link on the new device and give it a name. It starts watch-only.`}
          >
            <div className="flex items-center gap-2">
              <code className="max-w-72 truncate rounded bg-muted px-2 py-1 text-[11px]">
                {pairing.url}
              </code>
              <CopyButton text={pairing.url} label="Copy link" />
            </div>
          </SettingRow>
        ) : (
          <p className="px-1 text-[11px] text-muted-foreground">
            Each code works once, for ten minutes. Earlier codes keep working.
          </p>
        )}
      </div>

      <div className="flex flex-col gap-2">
        <Label>Devices</Label>
        {devices.length === 0 ? (
          <p className="px-1 text-[11px] text-muted-foreground">
            No device has paired yet.
          </p>
        ) : (
          devices.map((device) => {
            const connected = now - device.lastSeen * 1000 < CONNECTED_MS;
            return (
              <SettingRow
                key={device.id}
                title={device.id === me ? `${device.name} (this device)` : device.name}
                description={`Paired ${whenSeconds(device.createdAt)} · last seen ${whenSeconds(device.lastSeen)}`}
              >
                <div className="flex items-center gap-3 text-[11px] text-muted-foreground">
                  <span>{grantSummary(device.grant)}</span>
                  <span
                    className={
                      connected
                        ? "rounded bg-emerald-500/15 px-1.5 py-0.5 text-emerald-600 dark:text-emerald-400"
                        : "rounded bg-muted px-1.5 py-0.5"
                    }
                  >
                    {connected ? "Connected" : "Offline"}
                  </span>
                </div>
              </SettingRow>
            );
          })
        )}
      </div>
    </div>
  );
}

function DesktopRemoteSection() {
  const [info, setInfo] = useState<RemoteInfo | null>(null);
  const [devices, setDevices] = useState<PairedDevice[]>([]);
  const [audit, setAudit] = useState<AuditEntry[]>([]);
  const [pairing, setPairing] = useState<Pairing | null>(null);
  // The last start, so the form matches what the status bar would turn on.
  const [last] = useState(readRemoteOptions);
  const [bind, setBind] = useState<Bind>(last.bind);
  const [port, setPort] = useState(last.port);
  const [plainLan, setPlainLan] = useState(last.allowPlainLan);
  const [https, setHttps] = useState(last.https);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      const [status, list, entries] = await Promise.all([
        invoke<RemoteInfo>("remote_access_status"),
        invoke<PairedDevice[]>("remote_devices_list"),
        invoke<AuditEntry[]>("remote_audit_log").catch(() => []),
      ]);
      setInfo(status);
      setDevices(list);
      setAudit(entries);
      if (status.bind) setBind(status.bind);
      if (status.port) setPort(status.port);
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : String(failure));
    }
  }, []);

  useEffect(() => {
    void refresh();
    const unlisten = listen<RemoteInfo>("terax:remote-access", (event) => {
      setInfo(event.payload);
      void invoke<PairedDevice[]>("remote_devices_list")
        .then(setDevices)
        .catch(() => undefined);
    });
    return () => {
      void unlisten.then((stop) => stop());
    };
  }, [refresh]);

  const run = async (action: () => Promise<unknown>) => {
    setBusy(true);
    setError(null);
    try {
      await action();
      await refresh();
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : String(failure));
    } finally {
      setBusy(false);
    }
  };

  const start = () =>
    run(async () => {
      const options = {
        bind,
        port,
        allowPlainLan: plainLan,
        https: https && bind === "tailscale",
      };
      await invoke("remote_access_start", options);
      writeRemoteOptions(options);
    });

  const stop = () => {
    setPairing(null);
    return run(() => invoke("remote_access_stop"));
  };

  const pair = () =>
    run(async () => {
      const next = await invoke<Pairing>("remote_pairing_start");
      setPairing(next);
    });

  const update = (device: PairedDevice, grant: Grant) =>
    run(() =>
      invoke("remote_device_update", {
        id: device.id,
        grant,
      }),
    );

  const revoke = (device: PairedDevice) => {
    toast(`Revoked ${device.name}`);
    return run(() => invoke("remote_device_revoke", { id: device.id }));
  };

  const running = info?.running === true;

  return (
    <div className="flex flex-col gap-6">
      <SectionHeader
        title="Remote access"
        description="Watch and type into this machine's terminal tabs from a browser, over Tailscale or the local network."
      />

      {error ? (
        <p className="rounded-lg border border-destructive/40 bg-destructive/10 px-3 py-2 text-[11.5px] text-destructive">
          {error}
        </p>
      ) : null}

      <div className="flex flex-col gap-2">
        <Label>Server</Label>
        <SettingRow
          title={running ? "Remote access is on" : "Remote access is off"}
          description={
            running && info
              ? `${info.viewers} connected ${info.viewers === 1 ? "viewer" : "viewers"}${info.https ? " · HTTPS" : ""}`
              : "Nothing listens until you turn it on."
          }
        >
          <Switch
            checked={running}
            disabled={busy}
            onCheckedChange={(next) => void (next ? start() : stop())}
          />
        </SettingRow>

        <SettingRow title="Bind" description={BIND_HINT[bind]}>
          <Select
            value={bind}
            onValueChange={(value) => setBind(value as Bind)}
            disabled={running || busy}
          >
            <SelectTrigger className="h-8 w-48 text-[12px]">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {(Object.keys(BIND_LABEL) as Bind[]).map((id) => (
                <SelectItem key={id} value={id}>
                  {BIND_LABEL[id]}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </SettingRow>

        <SettingRow
          title="Port"
          description="The port the browser opens. Terax uses 7681 by default."
        >
          <Input
            type="number"
            min={1}
            max={65535}
            value={port}
            disabled={running || busy}
            onChange={(event) => setPort(Number(event.target.value) || 7681)}
            className="h-8 w-24 text-[12px]"
          />
        </SettingRow>

        {bind === "tailscale" ? (
          <SettingRow
            title="Use HTTPS (tailscale serve)"
            description="Tailscale terminates TLS on 443 and proxies here, so the browser gets a real certificate. Refused if serve is already configured."
          >
            <Switch
              checked={https}
              disabled={running || busy}
              onCheckedChange={setHttps}
            />
          </SettingRow>
        ) : null}

        {bind === "all" ? (
          <SettingRow
            title="Allow plain HTTP on the local network"
            description="Without this, binding every interface is refused until you opt in."
          >
            <Switch
              checked={plainLan}
              disabled={running || busy}
              onCheckedChange={setPlainLan}
            />
          </SettingRow>
        ) : null}

        {running && info ? (
          <SettingRow
            title="Open it at"
            description="On another device on the same network."
          >
            <div className="flex items-center gap-2">
              <code className="max-w-64 truncate rounded bg-muted px-2 py-1 text-[11px]">
                {info.urls[0] ?? "—"}
              </code>
              {info.urls[0] ? (
                <CopyButton text={info.urls[0]} label="Copy" />
              ) : null}
            </div>
          </SettingRow>
        ) : null}
      </div>

      <div className="flex flex-col gap-2">
        <div className="flex items-center justify-between">
          <Label>Pair a device</Label>
          <Button
            size="sm"
            variant="outline"
            disabled={!running || busy}
            onClick={() => void pair()}
          >
            {pairing ? "New code" : "Pair a device"}
          </Button>
        </div>
        {pairing ? (
          <SettingRow
            title={pairing.code}
            description={`Expires in ${Math.round(pairing.expiresInSec / 60)} minutes. Open the link on the new device and give it a name.`}
          >
            <div className="flex items-center gap-2">
              <code className="max-w-72 truncate rounded bg-muted px-2 py-1 text-[11px]">
                {pairing.url}
              </code>
              <CopyButton text={pairing.url} label="Copy link" />
            </div>
          </SettingRow>
        ) : (
          <p className="px-1 text-[11px] text-muted-foreground">
            A pairing code works once, for ten minutes.
          </p>
        )}
      </div>

      <div className="flex flex-col gap-2">
        <Label>Devices</Label>
        {devices.length === 0 ? (
          <p className="px-1 text-[11px] text-muted-foreground">
            No device has paired yet.
          </p>
        ) : (
          <>
            <p className="px-1 text-[11px] text-muted-foreground">
              Full app access lets this device run anything you can.
            </p>
            {devices.map((device) => (
            <SettingRow
              key={device.id}
              title={device.name}
              description={`Paired ${whenSeconds(device.createdAt)} · last seen ${whenSeconds(device.lastSeen)}`}
            >
              <div className="flex items-center gap-4">
                <span className="flex items-center gap-1.5 text-[11px] text-muted-foreground">
                  Type
                  <Switch
                    checked={device.grant.input}
                    onCheckedChange={(next) =>
                      void update(device, { ...device.grant, input: next })
                    }
                  />
                </span>
                <span className="flex items-center gap-1.5 text-[11px] text-muted-foreground">
                  Tabs
                  <Switch
                    checked={device.grant.tabs}
                    onCheckedChange={(next) =>
                      void update(device, { ...device.grant, tabs: next })
                    }
                  />
                </span>
                <span className="flex items-center gap-1.5 text-[11px] text-muted-foreground">
                  App
                  <Switch
                    checked={device.grant.app}
                    onCheckedChange={(next) =>
                      void update(device, { ...device.grant, app: next })
                    }
                  />
                </span>
                <Button
                  size="sm"
                  variant="ghost"
                  onClick={() => void revoke(device)}
                >
                  Revoke
                </Button>
              </div>
            </SettingRow>
          ))}
          </>
        )}
      </div>

      {audit.length > 0 ? (
        <div className="flex flex-col gap-2">
          <Label>Recent remote commands</Label>
          <div className="max-h-44 overflow-y-auto rounded-md border border-border/60 bg-card/40 px-3 py-2 text-[11px]">
            {audit.slice(0, 50).map((entry, idx) => (
              <div
                key={`${entry.timestampMs}-${entry.cmd}-${idx}`}
                className="flex items-center justify-between gap-2 py-0.5 text-muted-foreground"
              >
                <span className="truncate font-mono text-foreground">
                  {entry.cmd}
                </span>
                <span className="shrink-0">
                  {entry.deviceName} · {entry.class} · {when(entry.timestampMs)}
                </span>
              </div>
            ))}
          </div>
        </div>
      ) : null}
    </div>
  );
}
