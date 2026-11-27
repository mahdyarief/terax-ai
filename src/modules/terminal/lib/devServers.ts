import { create } from "zustand";

/**
 * A local server a terminal printed the address of, such as Vite's
 * `Local: http://localhost:5173/`. `host` is the SSH host the terminal runs
 * on, or null on this machine (WSL forwards localhost here).
 */
export type DevServer = {
  readonly leafId: number;
  readonly port: number;
  readonly url: string;
  readonly host: string | null;
};

/** A loopback URL with a port. 0.0.0.0 and [::] are how servers say "all". */
const LOOPBACK_URL =
  /\b(https?):\/\/(localhost|127\.0\.0\.1|0\.0\.0\.0|\[::1?\]):(\d{1,5})(?!\d|\.\d)([/?#][^\s'"<>`)\]]*)?/gi;

/** CSI, OSC (up to BEL or ST), and two-character escapes. */
const ESCAPES =
  /\x1b\[[0-?]*[ -/]*[@-~]|\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)|\x1b[ -/]*[0-~]/g;

/** Longest line kept waiting for its end before it is scanned as it is. */
const MAX_PENDING = 4096;

/** Finds loopback server addresses in text with escapes already removed. */
export function findLoopbackUrls(
  text: string,
): { port: number; url: string }[] {
  const found: { port: number; url: string }[] = [];
  for (const m of text.matchAll(LOOPBACK_URL)) {
    const port = Number(m[3]);
    if (port < 1 || port > 65535) continue;
    const rawHost = m[2].toLowerCase();
    const host =
      rawHost === "0.0.0.0" || rawHost === "[::]" ? "localhost" : rawHost;
    // Sentence punctuation after a URL is not part of it.
    const path = (m[4] ?? "/").replace(/[.,;:!]+$/, "") || "/";
    found.push({ port, url: `${m[1].toLowerCase()}://${host}:${port}${path}` });
  }
  return found;
}

/**
 * Reads a terminal's output as it arrives and returns the loopback addresses
 * on each line once the line is complete, so an address split across chunks
 * is read whole. Colors inside an address (Vite bolds the port) are removed
 * first.
 */
export class DevServerUrlScanner {
  private readonly decoder = new TextDecoder();
  private pending = "";

  push(bytes: Uint8Array): { port: number; url: string }[] {
    const text = this.pending + this.decoder.decode(bytes, { stream: true });
    const end = Math.max(text.lastIndexOf("\n"), text.lastIndexOf("\r"));
    let complete: string;
    if (end >= 0) {
      complete = text.slice(0, end);
      this.pending = text.slice(end + 1);
    } else {
      complete = "";
      this.pending = text;
    }
    if (this.pending.length > MAX_PENDING) {
      complete += this.pending;
      this.pending = "";
    }
    // Most output has no address at all; skip the work for it.
    if (!complete.includes("://") && !complete.includes("\x1b")) return [];
    return findLoopbackUrls(complete.replace(ESCAPES, ""));
  }
}

/** Addresses kept per terminal; a server prints a handful at most. */
const MAX_PER_TERMINAL = 6;

type DevServerStore = {
  byLeaf: ReadonlyMap<number, readonly DevServer[]>;
  add: (
    leafId: number,
    found: readonly { port: number; url: string }[],
    host: string | null,
  ) => void;
  clear: (leafId: number) => void;
};

export const useDevServerStore = create<DevServerStore>((set, get) => ({
  byLeaf: new Map(),
  add: (leafId, found, host) => {
    const current = get().byLeaf.get(leafId) ?? [];
    const next = [...current];
    for (const f of found) {
      if (next.length >= MAX_PER_TERMINAL) break;
      if (next.some((s) => s.port === f.port)) continue;
      next.push({ leafId, port: f.port, url: f.url, host });
    }
    if (next.length === current.length) return;
    set((s) => {
      const byLeaf = new Map(s.byLeaf);
      byLeaf.set(leafId, next);
      return { byLeaf };
    });
  },
  clear: (leafId) => {
    if (!get().byLeaf.has(leafId)) return;
    set((s) => {
      const byLeaf = new Map(s.byLeaf);
      byLeaf.delete(leafId);
      return { byLeaf };
    });
  },
}));

/**
 * Every server the terminals know of, in the order they were found. Two
 * terminals printing the same address show it once.
 */
export function listDevServers(
  byLeaf: ReadonlyMap<number, readonly DevServer[]>,
): DevServer[] {
  const out: DevServer[] = [];
  for (const servers of byLeaf.values()) {
    for (const s of servers) {
      if (!out.some((o) => o.port === s.port && o.host === s.host)) out.push(s);
    }
  }
  return out;
}
