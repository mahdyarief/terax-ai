/**
 * The SSE messages a remote terminal receives, applied to a Ghostty model.
 *
 * The server sends a snapshot after `meta` on a fresh connection, and only the
 * events after the browser's `Last-Event-ID` on a reconnect. Nothing here
 * decides what to replay: the model is reset exactly when a snapshot says so,
 * which is what keeps a reconnect from duplicating output.
 */

export type Rgb = readonly [number, number, number];

export type RemoteTheme = {
  readonly foreground: Rgb;
  readonly background: Rgb;
  readonly cursor: Rgb;
  readonly palette: readonly Rgb[];
};

export type MetaMessage = {
  readonly cols: number;
  readonly rows: number;
  readonly truncated: boolean;
  readonly theme: RemoteTheme | null;
};

/** The part of the Ghostty model a feed touches. */
export type FeedTerminal = {
  write(bytes: Uint8Array): void;
  resize(cols: number, rows: number): void;
  clear(): void;
};

export function decodeBase64(text: string): Uint8Array {
  const binary = atob(text);
  const bytes = new Uint8Array(binary.length);
  for (let at = 0; at < binary.length; at += 1) {
    bytes[at] = binary.charCodeAt(at);
  }
  return bytes;
}

export class RemoteTerminalFeed {
  cols = 80;
  rows = 24;
  truncated = false;
  theme: RemoteTheme | null = null;
  /** The replay sequence of the last applied event. */
  sequence = 0;
  exited: { code: number } | null = null;

  constructor(private readonly terminal: FeedTerminal) {}

  meta(message: MetaMessage): void {
    const resized = message.cols !== this.cols || message.rows !== this.rows;
    this.cols = message.cols;
    this.rows = message.rows;
    this.truncated = message.truncated;
    this.theme = message.theme;
    if (resized) this.terminal.resize(this.cols, this.rows);
  }

  /** A full rebuild: the log no longer reaches back to where this viewer was. */
  snapshot(id: string | null, data: string): void {
    this.terminal.clear();
    this.terminal.write(decodeBase64(data));
    this.sequence = sequenceOf(id, this.sequence);
  }

  /** New output only; a reconnect resumes here without repeating anything. */
  data(id: string | null, data: string): void {
    this.terminal.write(decodeBase64(data));
    this.sequence = sequenceOf(id, this.sequence);
  }

  resize(message: { cols: number; rows: number }): void {
    if (message.cols === this.cols && message.rows === this.rows) return;
    this.cols = message.cols;
    this.rows = message.rows;
    this.terminal.resize(this.cols, this.rows);
  }

  exit(message: { code: number }): void {
    this.exited = { code: message.code };
  }
}

function sequenceOf(id: string | null, fallback: number): number {
  const parsed = id === null ? Number.NaN : Number(id);
  return Number.isFinite(parsed) ? parsed : fallback;
}
