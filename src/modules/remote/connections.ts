import { invoke } from "@tauri-apps/api/core";
import { create } from "zustand";

/** A host's connection, as the app's `remote://state` event reports it. */
export type RemoteState =
  | { state: "idle" }
  | { state: "connecting" }
  | { state: "installing"; bytes: number }
  | { state: "handshaking" }
  | { state: "ready" }
  | { state: "reconnecting"; retry: number; reason: string }
  | { state: "failed"; reason: string };

export type RemoteSnapshot = { host: string; state: RemoteState };

export const STATE_EVENT = "remote://state";

const isObject = (v: unknown): v is Record<string, unknown> =>
  typeof v === "object" && v !== null;

/** The event's payload, or null if it is not one. */
export function parseSnapshot(payload: unknown): RemoteSnapshot | null {
  if (!isObject(payload) || typeof payload.host !== "string") return null;
  const { host } = payload;
  switch (payload.state) {
    case "idle":
    case "connecting":
    case "handshaking":
    case "ready":
      return { host, state: { state: payload.state } };
    case "installing":
      return typeof payload.bytes === "number"
        ? { host, state: { state: "installing", bytes: payload.bytes } }
        : null;
    case "reconnecting":
      return typeof payload.retry === "number" &&
        typeof payload.reason === "string"
        ? {
            host,
            state: {
              state: "reconnecting",
              retry: payload.retry,
              reason: payload.reason,
            },
          }
        : null;
    case "failed":
      return typeof payload.reason === "string"
        ? { host, state: { state: "failed", reason: payload.reason } }
        : null;
    default:
      return null;
  }
}

export type StateTone = "busy" | "ok" | "warn" | "error";

/** What the status bar shows for a state. */
export function describeState(state: RemoteState): {
  label: string;
  tone: StateTone;
  detail?: string;
} {
  switch (state.state) {
    case "idle":
      return { label: "Disconnected", tone: "warn" };
    case "connecting":
      return { label: "Connecting…", tone: "busy" };
    case "installing":
      return {
        label: "Installing agent…",
        tone: "busy",
        detail: `Uploading ${formatBytes(state.bytes)}`,
      };
    case "handshaking":
      return { label: "Starting agent…", tone: "busy" };
    case "ready":
      return { label: "Connected", tone: "ok" };
    case "reconnecting":
      return { label: "Reconnecting…", tone: "warn", detail: state.reason };
    case "failed":
      return { label: "Failed", tone: "error", detail: state.reason };
  }
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

/**
 * Whether ssh would take `host` as a destination: the same rule as the app's
 * (no leading `-`, no whitespace or control characters, at most 255 bytes).
 */
export function isValidHost(host: string): boolean {
  if (host.length === 0 || host.startsWith("-")) return false;
  if (new TextEncoder().encode(host).length > 255) return false;
  return !/[\s\u0000-\u001f\u007f]/.test(host);
}

export type PickerEntry = {
  host: string;
  source: "recent" | "config" | "typed";
};

/**
 * The picker's rows: recent hosts first, then the config's, matching the
 * query; a typed destination that is neither comes last.
 */
export function pickerEntries(
  recents: readonly string[],
  configHosts: readonly string[],
  query: string,
): PickerEntry[] {
  const q = query.trim().toLowerCase();
  const matches = (host: string) => host.toLowerCase().includes(q);
  const entries: PickerEntry[] = [];
  for (const host of recents) {
    if (matches(host)) entries.push({ host, source: "recent" });
  }
  for (const host of configHosts) {
    if (!recents.includes(host) && matches(host)) {
      entries.push({ host, source: "config" });
    }
  }
  const typed = query.trim();
  if (
    isValidHost(typed) &&
    !entries.some((e) => e.host === typed) &&
    !recents.includes(typed)
  ) {
    entries.push({ host: typed, source: "typed" });
  }
  return entries;
}

const RECENTS_KEY = "terax-remote-recent-hosts";
export const MAX_RECENT_HOSTS = 8;

export function touchRecentHost(list: readonly string[], host: string) {
  return [host, ...list.filter((h) => h !== host)].slice(0, MAX_RECENT_HOSTS);
}

function loadRecents(): string[] {
  try {
    const parsed: unknown = JSON.parse(
      localStorage.getItem(RECENTS_KEY) ?? "[]",
    );
    return Array.isArray(parsed)
      ? parsed.filter((h): h is string => typeof h === "string" && isValidHost(h))
      : [];
  } catch {
    return [];
  }
}

function saveRecents(recents: string[]) {
  try {
    localStorage.setItem(RECENTS_KEY, JSON.stringify(recents));
  } catch {
    // A convenience only.
  }
}

/** What `remote_browse_dirs` returns. */
export type Browse = { path: string; dirs: string[] };

/** The parent of a server path (`/` stays `/`). */
export function parentPath(path: string): string {
  const trimmed = path.replace(/\/+$/, "");
  const at = trimmed.lastIndexOf("/");
  return at <= 0 ? "/" : trimmed.slice(0, at);
}

export function childPath(path: string, name: string): string {
  return path.endsWith("/") ? `${path}${name}` : `${path}/${name}`;
}

/** A folder's last component, for a space's name. */
export function folderName(path: string): string {
  return path.split("/").filter(Boolean).pop() ?? "/";
}

type FolderHandler = (host: string, path: string) => Promise<void>;

type Store = {
  hosts: Record<string, RemoteState>;
  /** Times each host became ready: a new connection, so views reload. */
  readyEpochs: Record<string, number>;
  /** The host whose folder picker is open. */
  folderPickerHost: string | null;
  /** Opens a picked folder as a space; set by the app. */
  folderHandler: FolderHandler | null;
  recents: string[];
  pickerOpen: boolean;
  logHost: string | null;
  apply: (snapshot: RemoteSnapshot) => void;
  load: () => Promise<void>;
  connect: (host: string) => Promise<void>;
  disconnect: (host: string) => Promise<void>;
  setPickerOpen: (open: boolean) => void;
  showLog: (host: string | null) => void;
  openFolderPicker: (host: string | null) => void;
  setFolderHandler: (handler: FolderHandler | null) => void;
};

export const useRemoteStore = create<Store>((set, get) => ({
  hosts: {},
  readyEpochs: {},
  folderPickerHost: null,
  folderHandler: null,
  recents: loadRecents(),
  pickerOpen: false,
  logHost: null,
  apply: ({ host, state }) =>
    set((s) => {
      const becameReady =
        state.state === "ready" && s.hosts[host]?.state !== "ready";
      return {
        hosts: { ...s.hosts, [host]: state },
        readyEpochs: becameReady
          ? { ...s.readyEpochs, [host]: (s.readyEpochs[host] ?? 0) + 1 }
          : s.readyEpochs,
      };
    }),
  load: async () => {
    const all = await invoke<unknown[]>("remote_state");
    for (const raw of all) {
      const snapshot = parseSnapshot(raw);
      if (snapshot) get().apply(snapshot);
    }
  },
  connect: async (host) => {
    const recents = touchRecentHost(get().recents, host);
    set({ recents });
    saveRecents(recents);
    await invoke("remote_connect", { host });
  },
  disconnect: async (host) => {
    await invoke("remote_disconnect", { host });
  },
  setPickerOpen: (pickerOpen) => set({ pickerOpen }),
  showLog: (logHost) => set({ logHost }),
  openFolderPicker: (folderPickerHost) => set({ folderPickerHost }),
  setFolderHandler: (folderHandler) => set({ folderHandler }),
}));

const ATTEMPTING = new Set(["connecting", "installing", "handshaking"]);

/**
 * Resolves once `host` is ready, connecting it if needed (a user-started
 * attempt, so ssh may ask through the askpass dialog). Rejects with ssh's
 * reason if that attempt fails.
 */
export function ensureConnected(host: string): Promise<void> {
  const store = useRemoteStore.getState();
  if (store.hosts[host]?.state === "ready") return Promise.resolve();
  return new Promise((resolve, reject) => {
    // A `failed` left from an earlier attempt is not this one's outcome.
    let attempted = false;
    const settle = (state: RemoteState | undefined): boolean => {
      if (!state) return false;
      if (state.state === "ready") {
        resolve();
        return true;
      }
      if (ATTEMPTING.has(state.state)) attempted = true;
      if (state.state === "failed" && attempted) {
        reject(new Error(state.reason));
        return true;
      }
      return false;
    };
    const stop = useRemoteStore.subscribe((s, previous) => {
      if (s.hosts[host] === previous.hosts[host]) return;
      if (settle(s.hosts[host])) stop();
    });
    store.connect(host).catch((e) => {
      stop();
      reject(e instanceof Error ? e : new Error(String(e)));
    });
  });
}

/**
 * Resolves once `host` is ready, without starting an attempt: for work that
 * a space's own connection serves (a terminal opened while its server is
 * still connecting or reconnecting). Rejects when there is no attempt to
 * wait for: disconnected, or failed with ssh's reason.
 */
export function whenServerReady(host: string): Promise<void> {
  return new Promise((resolve, reject) => {
    // Unknown means the state has not arrived yet; a restored space is
    // connecting.
    const settle = (state: RemoteState | undefined): boolean => {
      if (state?.state === "ready") {
        resolve();
        return true;
      }
      if (state?.state === "idle") {
        reject(new Error(`${host} is not connected`));
        return true;
      }
      if (state?.state === "failed") {
        reject(new Error(state.reason));
        return true;
      }
      return false;
    };
    if (settle(useRemoteStore.getState().hosts[host])) return;
    const stop = useRemoteStore.subscribe((s, previous) => {
      if (s.hosts[host] === previous.hosts[host]) return;
      if (settle(s.hosts[host])) stop();
    });
  });
}

/**
 * Runs `then` the next time `host` becomes ready (a later connection than
 * the current one, even if it is ready now: a terminal's drop can arrive
 * before the state does). Returns a function that stops waiting.
 */
export function onNextReady(host: string, then: () => void): () => void {
  const start = useRemoteStore.getState().readyEpochs[host] ?? 0;
  const stop = useRemoteStore.subscribe((s) => {
    if ((s.readyEpochs[host] ?? 0) <= start) return;
    stop();
    then();
  });
  return stop;
}

export async function fetchRemoteHome(host: string): Promise<string> {
  return invoke<string>("remote_home", { host });
}

export async function browseRemoteDirs(
  host: string,
  path: string,
): Promise<Browse> {
  return invoke<Browse>("remote_browse_dirs", { host, path });
}

export function openSshPicker() {
  useRemoteStore.getState().setPickerOpen(true);
}

export async function fetchSshHosts(): Promise<string[]> {
  return invoke<string[]>("remote_ssh_hosts");
}

export async function fetchRemoteLog(host: string): Promise<string[]> {
  return invoke<string[]>("remote_log", { host });
}
