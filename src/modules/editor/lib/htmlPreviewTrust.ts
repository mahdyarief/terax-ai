const KEY = "terax:html-preview-trusted-folders";

// Used when storage is unavailable, and as a cache in front of it.
const session = new Set<string>();

function normalize(folder: string): string {
  return folder.replace(/\\/g, "/").replace(/\/+$/, "");
}

type TrustStorage = Pick<Storage, "getItem" | "setItem">;

function browserStorage(): TrustStorage | null {
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}

function load(storage: TrustStorage | null): string[] {
  try {
    const raw = storage?.getItem(KEY);
    const parsed: unknown = raw ? JSON.parse(raw) : [];
    return Array.isArray(parsed)
      ? parsed.filter((item): item is string => typeof item === "string")
      : [];
  } catch {
    return [];
  }
}

/**
 * Whether HTML previews under `folder` may run scripts even when they pulled
 * in local files. A script can send out anything it can read, so that is the
 * user's call, made once per folder.
 */
export function isPreviewTrusted(
  folder: string,
  storage: TrustStorage | null = browserStorage(),
): boolean {
  const key = normalize(folder);
  return session.has(key) || load(storage).includes(key);
}

export function trustPreview(
  folder: string,
  storage: TrustStorage | null = browserStorage(),
): void {
  const key = normalize(folder);
  session.add(key);
  try {
    const all = load(storage);
    if (!all.includes(key)) {
      storage?.setItem(KEY, JSON.stringify([...all, key]));
    }
  } catch {
    // The in-memory set still covers this session.
  }
}
