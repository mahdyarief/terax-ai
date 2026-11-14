export const MAX_ROWS = 201;
export const MAX_COLUMNS = 40;
export const MAX_SHEETS = 20;

export type PreviewSheet = {
  name: string;
  rows: unknown[][];
  columns: number;
  truncated: boolean;
};

type WorkbookSheet = { sheet?: string; data?: unknown[][] };

/**
 * read-excel-file returns `data: undefined` for chart sheets and empty
 * worksheets, so those must collapse to an empty preview instead of throwing.
 */
export function buildPreviewSheets(workbook: WorkbookSheet[]): {
  sheets: PreviewSheet[];
  truncated: boolean;
} {
  const sheets = workbook.slice(0, MAX_SHEETS).map(({ sheet, data }) => {
    const source: unknown[][] = Array.isArray(data) ? data : [];
    const rows = source
      .slice(0, MAX_ROWS)
      .map((row) => (Array.isArray(row) ? row.slice(0, MAX_COLUMNS) : []));
    return {
      name: sheet ?? "",
      rows,
      columns: Math.min(
        MAX_COLUMNS,
        rows.reduce((max, row) => Math.max(max, row.length), 0),
      ),
      truncated:
        source.length > MAX_ROWS ||
        source.some((row) => Array.isArray(row) && row.length > MAX_COLUMNS),
    };
  });
  return { sheets, truncated: workbook.length > MAX_SHEETS };
}
