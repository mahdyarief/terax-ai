import { create } from "zustand";

/**
 * In a browser session the host's native folder dialog cannot open (it would
 * appear on the host's screen), so folder picks go through an in-app dialog
 * that lists the host's folders with `list_subdirs`.
 */
type Request = {
  defaultPath: string | null;
  resolve: (path: string | null) => void;
};

type State = {
  request: Request | null;
  open: (defaultPath: string | null) => Promise<string | null>;
  finish: (path: string | null) => void;
};

export const useBrowserFolderPicker = create<State>((set, get) => ({
  request: null,
  open: (defaultPath) =>
    new Promise((resolve) => {
      // Only one dialog at a time: an older pick counts as cancelled.
      get().request?.resolve(null);
      set({ request: { defaultPath, resolve } });
    }),
  finish: (path) => {
    const request = get().request;
    set({ request: null });
    request?.resolve(path);
  },
}));

function separatorOf(path: string): "/" | "\\" {
  return path.includes("\\") && !path.includes("/") ? "\\" : "/";
}

/** The parent of a host path, POSIX or Windows; a root stays itself. */
export function hostParentPath(path: string): string {
  const sep = separatorOf(path);
  if (sep === "/") {
    const trimmed = path.replace(/\/+$/, "");
    const at = trimmed.lastIndexOf("/");
    return at <= 0 ? "/" : trimmed.slice(0, at);
  }
  const trimmed = path.replace(/\\+$/, "");
  const at = trimmed.lastIndexOf("\\");
  if (at < 0) return `${trimmed}\\`;
  const parent = trimmed.slice(0, at);
  return /^[A-Za-z]:$/.test(parent) ? `${parent}\\` : parent;
}

export function hostChildPath(path: string, name: string): string {
  const sep = separatorOf(path);
  return path.endsWith(sep) ? `${path}${name}` : `${path}${sep}${name}`;
}

export function isHostRoot(path: string): boolean {
  return path === "/" || /^[A-Za-z]:\\?$/.test(path);
}
