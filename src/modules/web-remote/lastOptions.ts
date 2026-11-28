// How remote access was last turned on, so the status bar can turn it back
// on in one click with the same bind, port and HTTPS choice as Settings.

const KEY = "terax.remoteAccess.last";

export type RemoteBindChoice = "loopback" | "tailscale" | "all";

export type RemoteStartOptions = {
  bind: RemoteBindChoice;
  port: number;
  allowPlainLan: boolean;
  https: boolean;
};

export const DEFAULT_REMOTE_OPTIONS: RemoteStartOptions = {
  bind: "loopback",
  port: 7681,
  allowPlainLan: false,
  https: false,
};

type OptionStorage = Pick<Storage, "getItem" | "setItem">;

function defaultStorage(): OptionStorage | null {
  try {
    return typeof localStorage === "undefined" ? null : localStorage;
  } catch {
    return null;
  }
}

const BINDS: readonly RemoteBindChoice[] = ["loopback", "tailscale", "all"];

/** Anything malformed falls back to the safe default, never a wider bind. */
export function normalizeRemoteOptions(value: unknown): RemoteStartOptions {
  if (!value || typeof value !== "object") return DEFAULT_REMOTE_OPTIONS;
  const raw = value as Record<string, unknown>;
  const bind = BINDS.includes(raw.bind as RemoteBindChoice)
    ? (raw.bind as RemoteBindChoice)
    : "loopback";
  const port =
    typeof raw.port === "number" &&
    Number.isInteger(raw.port) &&
    raw.port >= 1024 &&
    raw.port <= 65535
      ? raw.port
      : DEFAULT_REMOTE_OPTIONS.port;
  return {
    bind,
    port,
    allowPlainLan: bind === "all" && raw.allowPlainLan === true,
    https: bind === "tailscale" && raw.https === true,
  };
}

export function readRemoteOptions(
  storage: OptionStorage | null = defaultStorage(),
): RemoteStartOptions {
  try {
    const stored = storage?.getItem(KEY);
    return normalizeRemoteOptions(stored ? JSON.parse(stored) : null);
  } catch {
    return DEFAULT_REMOTE_OPTIONS;
  }
}

export function writeRemoteOptions(
  options: RemoteStartOptions,
  storage: OptionStorage | null = defaultStorage(),
): void {
  try {
    storage?.setItem(KEY, JSON.stringify(normalizeRemoteOptions(options)));
  } catch {
    // Storage can be unavailable; the status bar then starts with defaults.
  }
}
