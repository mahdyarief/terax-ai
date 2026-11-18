// Whether the reader hid the document outline. One setting for every preview,
// remembered across tabs and restarts.

const KEY = "terax.markdown.outlineHidden";

type OutlineStorage = Pick<Storage, "getItem" | "setItem" | "removeItem">;

function defaultStorage(): OutlineStorage | null {
  try {
    return typeof localStorage === "undefined" ? null : localStorage;
  } catch {
    return null;
  }
}

export function readOutlineHidden(
  storage: OutlineStorage | null = defaultStorage(),
): boolean {
  try {
    return storage?.getItem(KEY) === "1";
  } catch {
    return false;
  }
}

export function writeOutlineHidden(
  hidden: boolean,
  storage: OutlineStorage | null = defaultStorage(),
): void {
  try {
    if (hidden) storage?.setItem(KEY, "1");
    else storage?.removeItem(KEY);
  } catch {
    // Storage can be unavailable; the choice then lasts for this pane only.
  }
}
