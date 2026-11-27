/**
 * The remote API, as a browser sees it: a cookie the server sets, and three
 * endpoints. Nothing here touches Tauri.
 */

export type Grant = { input: boolean; tabs: boolean };

export type Device = { id: string; name: string; grant: Grant };

export class RemoteError extends Error {
  constructor(
    readonly status: number,
    message: string,
  ) {
    super(message);
  }
}

async function failure(response: Response): Promise<RemoteError> {
  const text = await response.text().catch(() => "");
  return new RemoteError(response.status, text.trim() || response.statusText);
}

/** The paired device behind this browser's cookie, or null when there is none. */
export async function fetchDevice(): Promise<Device | null> {
  const response = await fetch("/api/me", { credentials: "same-origin" });
  if (response.status === 401) return null;
  if (!response.ok) throw await failure(response);
  return (await response.json()) as Device;
}

/** Redeems a one-time pairing code and stores the device cookie. */
export async function pairDevice(
  code: string,
  deviceName: string,
): Promise<Device> {
  const response = await fetch("/api/pair", {
    method: "POST",
    credentials: "same-origin",
    headers: {
      "Content-Type": "application/json",
      "X-Terax-Remote": "1",
    },
    body: JSON.stringify({ code, deviceName }),
  });
  if (!response.ok) throw await failure(response);
  return (await response.json()) as Device;
}

async function post(path: string, body: BodyInit): Promise<void> {
  const response = await fetch(path, {
    method: "POST",
    credentials: "same-origin",
    headers: { "X-Terax-Remote": "1" },
    body,
  });
  if (!response.ok) throw await failure(response);
}

/**
 * Types raw bytes into a terminal. A paste larger than 64 KiB is split so no
 * single request is refused.
 */
export async function sendInput(
  ptyId: number,
  bytes: Uint8Array,
): Promise<void> {
  const limit = 64 * 1024;
  for (let at = 0; at < bytes.length; at += limit) {
    await post(
      `/api/pty/${ptyId}/input`,
      bytes.slice(at, Math.min(at + limit, bytes.length)),
    );
  }
}

export type TabAction =
  | { action: "new"; spaceId: string; mode: "terminal" | "blocks" }
  | { action: "close"; tabId: number };

export async function requestTabAction(action: TabAction): Promise<void> {
  await post("/api/tabs", JSON.stringify(action));
}
