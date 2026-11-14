import { diagnosticCount, forEachDiagnostic } from "@codemirror/lint";
import type { Extension } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { type DiagnosticItem, useDiagnosticsStore } from "./diagnosticsStore";

export function diagnosticsReporter(getPath: () => string): Extension {
  return EditorView.updateListener.of((update) => {
    if (
      !update.docChanged &&
      !update.transactions.some((tr) => tr.effects.length > 0)
    ) {
      return;
    }
    const state = update.state;
    const total = diagnosticCount(state);
    let errors = 0;
    let warnings = 0;
    const items: DiagnosticItem[] = [];
    if (total > 0) {
      forEachDiagnostic(state, (d) => {
        if (d.severity === "error") errors += 1;
        else if (d.severity === "warning") warnings += 1;
        const line = state.doc.lineAt(d.from);
        items.push({
          severity:
            d.severity === "error"
              ? "error"
              : d.severity === "info"
                ? "info"
                : "warning",
          message: d.message,
          line: line.number,
          column: d.from - line.from + 1,
        });
      });
    }
    useDiagnosticsStore
      .getState()
      .report(getPath(), { errors, warnings, items });
  });
}
