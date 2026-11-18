import { useMemo } from "react";
import { parseDelimitedPreview } from "./lib/csvPreview";

type Props = {
  content: string;
  delimiter: "," | ";" | "\t";
  /** Opens the file as plain text; omitted when that is not possible. */
  onEdit?: () => void;
  editLabel?: string;
  /** Set when `content` is only the start of a larger file. */
  partialOf?: string;
};

/** "First 200 rows" when cut short, otherwise the real count; header excluded. */
export function csvSummary(
  dataRows: number,
  columns: number,
  truncated: boolean,
  partialOf?: string,
): string {
  const rows =
    truncated || partialOf
      ? `First ${dataRows} rows`
      : `${dataRows} ${dataRows === 1 ? "row" : "rows"}`;
  const tail = partialOf ? ` · of a ${partialOf} file` : "";
  return `${rows} · ${columns} columns${tail}`;
}

export function CsvPreview({
  content,
  delimiter,
  onEdit,
  editLabel = "Edit as text",
  partialOf,
}: Props) {
  const preview = useMemo(
    () => parseDelimitedPreview(content, delimiter),
    [content, delimiter],
  );
  const [header = [], ...body] = preview.rows;

  return (
    <div className="flex h-full min-h-0 w-full flex-col bg-background">
      <div className="flex shrink-0 items-center justify-between border-b border-border px-3 py-2 text-xs text-muted-foreground">
        <span>
          {csvSummary(
            body.length,
            preview.columns,
            preview.truncated,
            partialOf,
          )}
        </span>
        {onEdit && (
          <button
            type="button"
            onClick={onEdit}
            className="rounded border border-border px-2 py-1 text-foreground hover:bg-accent"
          >
            {editLabel}
          </button>
        )}
      </div>
      <div className="min-h-0 flex-1 overflow-auto">
        <table className="min-w-full border-separate border-spacing-0 text-left text-xs">
          <thead className="sticky top-0 z-[1] bg-muted text-foreground">
            <tr>
              {header.slice(0, preview.columns).map((cell, index) => (
                <th
                  // The number and order of table columns are fixed per file.
                  // biome-ignore lint/suspicious/noArrayIndexKey: table columns have stable positions.
                  key={index}
                  scope="col"
                  className="max-w-[28rem] whitespace-pre-wrap border-b border-r border-border px-3 py-2 font-semibold"
                >
                  {cell}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {body.map((row, rowIndex) => (
              <tr
                // biome-ignore lint/suspicious/noArrayIndexKey: preview rows do not reorder.
                key={`${rowIndex}-${row.join("\u0000")}`}
                className="even:bg-muted/30"
              >
                {Array.from({ length: preview.columns }, (_, columnIndex) => (
                  <td
                    // biome-ignore lint/suspicious/noArrayIndexKey: fixed table column positions.
                    key={columnIndex}
                    className="select-text max-w-[28rem] whitespace-pre-wrap border-b border-r border-border/70 px-3 py-1.5 align-top text-foreground"
                  >
                    {row[columnIndex] ?? ""}
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
