import { create } from "zustand";

export type DiagnosticItem = {
  severity: "error" | "warning" | "info";
  message: string;
  line: number;
  column: number;
};

export type DiagnosticCounts = {
  errors: number;
  warnings: number;
  items?: readonly DiagnosticItem[];
};

type State = {
  byPath: Record<string, DiagnosticCounts>;
  report: (path: string, counts: DiagnosticCounts | null) => void;
};

function sameItems(
  a: readonly DiagnosticItem[] | undefined,
  b: readonly DiagnosticItem[] | undefined,
): boolean {
  if (a === b) return true;
  if (!a || !b || a.length !== b.length) return false;
  return a.every(
    (item, index) =>
      item.severity === b[index].severity &&
      item.message === b[index].message &&
      item.line === b[index].line &&
      item.column === b[index].column,
  );
}

export const useDiagnosticsStore = create<State>((set) => ({
  byPath: {},
  report: (path, counts) =>
    set((s) => {
      const prev = s.byPath[path];
      if (
        counts &&
        prev &&
        prev.errors === counts.errors &&
        prev.warnings === counts.warnings &&
        sameItems(prev.items, counts.items)
      ) {
        return s;
      }
      if (!counts && !prev) return s;
      const byPath = { ...s.byPath };
      if (counts) byPath[path] = counts;
      else delete byPath[path];
      return { byPath };
    }),
}));
