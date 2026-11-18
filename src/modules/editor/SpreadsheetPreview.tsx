import { useEffect, useState } from "react";
import {
  MAX_OFFICE_PREVIEW_BYTES,
  readBinary,
  validateOfficeArchive,
} from "./lib/externalFormat";
import { buildPreviewSheets, type PreviewSheet } from "./lib/spreadsheetPreview";
import { useFileRevision } from "./lib/useFileRevision";

type State = {
  path: string;
  sheets?: PreviewSheet[];
  error?: string;
  truncated?: boolean;
};

// Bytes come from IPC for the same reason as DOCX: the webview cannot fetch
// `asset:`/`http://asset.localhost` under the CSP, and convertFileSrc()
// encodes the path in a form the filesystem layer cannot resolve.
export function SpreadsheetPreview({
  path,
  size,
}: {
  path: string;
  size: number;
}) {
  const [state, setState] = useState<State>({ path });
  const [activeSheet, setActiveSheet] = useState(0);
  const revision = useFileRevision(path);

  // biome-ignore lint/correctness/useExhaustiveDependencies: a new file starts on its first sheet; a reload keeps the selection.
  useEffect(() => setActiveSheet(0), [path]);

  // biome-ignore lint/correctness/useExhaustiveDependencies: revision re-reads the file after it changed on disk.
  useEffect(() => {
    let cancelled = false;
    // A reload after a disk change keeps the current render until it is done.
    setState((prev) => (prev.path === path ? prev : { path }));
    if (size > MAX_OFFICE_PREVIEW_BYTES) {
      setState({ path, error: "XLSX preview is limited to 20 MB." });
      return;
    }

    void (async () => {
      try {
        const buffer = await readBinary(path);
        if (buffer.byteLength > MAX_OFFICE_PREVIEW_BYTES) {
          throw new Error("XLSX preview is limited to 20 MB.");
        }
        validateOfficeArchive(buffer, "XLSX");
        const { default: readXlsxFile } = await import(
          "read-excel-file/browser"
        );
        // read-excel-file accepts File | Blob | ArrayBuffer; pass the buffer
        // directly rather than an object that only imitates a Blob.
        const workbook = await (readXlsxFile as any)(buffer);
        const preview = buildPreviewSheets(workbook as any);
        if (!cancelled) {
          setState({ path, ...preview });
        }
      } catch (error) {
        if (!cancelled) {
          const message =
            error instanceof Error ? error.message : String(error);
          // A reload can catch the file mid-write; keep the last good render.
          setState((prev) =>
            prev.path === path && prev.sheets
              ? prev
              : { path, error: message },
          );
        }
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [path, size, revision]);

  if (state.path !== path || (!state.sheets && !state.error)) {
    return <PreviewMessage>Loading spreadsheet…</PreviewMessage>;
  }
  if (state.error) return <PreviewMessage>{state.error}</PreviewMessage>;

  const sheets = state.sheets ?? [];
  // A reload can come back with fewer sheets than the one selected.
  const current = sheets[Math.min(activeSheet, sheets.length - 1)];
  if (!current)
    return <PreviewMessage>This workbook has no worksheets.</PreviewMessage>;

  const limited = current.truncated || state.truncated;
  return (
    <div className="flex h-full min-h-0 w-full flex-col bg-background">
      <div className="flex shrink-0 items-center justify-between gap-3 border-b border-border px-3 py-2 text-xs text-muted-foreground">
        <span>
          {limited
            ? `First ${current.rows.length} rows`
            : `${current.rows.length} rows`}{" "}
          · {current.columns} columns
          {limited ? " · preview limited" : ""}
        </span>
        {sheets.length > 1 && (
          <select
            aria-label="Worksheet"
            className="max-w-56 rounded border border-border bg-background px-2 py-1 text-foreground"
            value={activeSheet}
            onChange={(event) =>
              setActiveSheet(Number(event.currentTarget.value))
            }
          >
            {sheets.map((sheet, index) => (
              <option key={sheet.name} value={index}>
                {sheet.name}
              </option>
            ))}
          </select>
        )}
      </div>
      <div className="min-h-0 flex-1 overflow-auto">
        <table className="min-w-full border-separate border-spacing-0 text-left text-xs">
          <thead className="sticky top-0 z-[1] bg-muted text-foreground">
            <tr>
              <th className="sticky left-0 border-b border-r border-border bg-muted px-3 py-2 font-medium" />
              {Array.from({ length: current.columns }, (_, index) => (
                <th
                  // biome-ignore lint/suspicious/noArrayIndexKey: spreadsheet columns have fixed positions.
                  key={index}
                  scope="col"
                  className="max-w-[28rem] whitespace-nowrap border-b border-r border-border px-3 py-2 font-semibold"
                >
                  {columnName(index)}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {current.rows.map((row, rowIndex) => (
              <tr
                // biome-ignore lint/suspicious/noArrayIndexKey: spreadsheet rows have fixed positions.
                key={rowIndex}
                className="even:bg-muted/30"
              >
                <th
                  scope="row"
                  className="sticky left-0 border-b border-r border-border bg-muted/80 px-3 py-1.5 text-right font-normal text-muted-foreground"
                >
                  {rowIndex + 1}
                </th>
                {Array.from({ length: current.columns }, (_, columnIndex) => (
                  <td
                    // biome-ignore lint/suspicious/noArrayIndexKey: fixed spreadsheet column positions.
                    key={columnIndex}
                    className="max-w-[28rem] whitespace-pre-wrap border-b border-r border-border/70 px-3 py-1.5 align-top text-foreground"
                  >
                    {formatCell(row[columnIndex])}
                  </td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}

function columnName(index: number): string {
  let value = index + 1;
  let name = "";
  while (value > 0) {
    value -= 1;
    name = String.fromCharCode(65 + (value % 26)) + name;
    value = Math.floor(value / 26);
  }
  return name;
}

function formatCell(value: unknown): string {
  if (value == null) return "";
  if (value instanceof Date) return value.toLocaleString();
  if (
    typeof value === "string" ||
    typeof value === "number" ||
    typeof value === "boolean"
  ) {
    return String(value);
  }
  return "";
}

function PreviewMessage({ children }: { children: string }) {
  return (
    <div className="flex h-full items-center justify-center px-6 text-center text-xs text-muted-foreground">
      {children}
    </div>
  );
}
