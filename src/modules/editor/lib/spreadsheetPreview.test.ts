import { describe, expect, it } from "vitest";
import {
  buildPreviewSheets,
  MAX_COLUMNS,
  MAX_ROWS,
  MAX_SHEETS,
} from "./spreadsheetPreview";

describe("buildPreviewSheets", () => {
  it("keeps chart sheets with no data from throwing", () => {
    // Real COBIT workbooks return `data: undefined` for summary/chart sheets.
    const { sheets } = buildPreviewSheets([
      { sheet: "Instructions", data: [["id", "name"]] },
      { sheet: "Step 2 Summary", data: undefined },
    ]);

    expect(sheets).toHaveLength(2);
    expect(sheets[1]).toEqual({
      name: "Step 2 Summary",
      rows: [],
      columns: 0,
      truncated: false,
    });
  });

  it("tolerates a missing sheet name", () => {
    const { sheets } = buildPreviewSheets([{ data: [["a"]] }]);
    expect(sheets[0].name).toBe("");
    expect(sheets[0].rows).toEqual([["a"]]);
  });

  it("clamps rows, columns and sheet count", () => {
    const wide = Array.from({ length: MAX_COLUMNS + 5 }, (_, index) => index);
    const tall = Array.from({ length: MAX_ROWS + 3 }, () => wide);
    const many = Array.from({ length: MAX_SHEETS + 2 }, (_, index) => ({
      sheet: `S${index}`,
      data: [wide],
    }));

    const result = buildPreviewSheets([{ sheet: "Big", data: tall }, ...many]);

    expect(result.sheets).toHaveLength(MAX_SHEETS);
    expect(result.sheets[0].rows).toHaveLength(MAX_ROWS);
    expect(result.sheets[0].rows[0]).toHaveLength(MAX_COLUMNS);
    expect(result.sheets[0].columns).toBe(MAX_COLUMNS);
    expect(result.sheets[0].truncated).toBe(true);
    expect(result.truncated).toBe(true);
  });

  it("reports no truncation for a small sheet", () => {
    const result = buildPreviewSheets([{ sheet: "Small", data: [["a", 1]] }]);
    expect(result.truncated).toBe(false);
    expect(result.sheets[0]).toMatchObject({ columns: 2, truncated: false });
  });
});
