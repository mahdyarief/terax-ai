import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

// WebView2 on a touchscreen laptop reports `hover: none` even with a mouse
// attached, and a keyboard user never hovers at all. So hover must not be the
// only way to reach a control, and hover styles must not depend on the
// `(hover: hover)` media query.

function sourceFiles(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) return sourceFiles(path);
    return entry.name.endsWith(".tsx") && !entry.name.includes(".test.")
      ? [path]
      : [];
  });
}

/** Class strings that hide an element until its group is hovered. */
function hoverOnlyClassStrings(source: string): string[] {
  const strings = source.match(/"[^"\n]*"|`[^`]*`/g) ?? [];
  return strings.filter((value) => {
    const classes = value.slice(1, -1).split(/\s+/);
    const hidden = classes.includes("opacity-0");
    const shownOnHover = classes.some((name) =>
      /^group-hover(\/[\w-]+)?:opacity-/.test(name),
    );
    const shownOnFocus = classes.some((name) =>
      /^(focus-visible|focus-within|group-focus[\w-]*(\/[\w-]+)?):opacity-/.test(
        name,
      ),
    );
    return hidden && shownOnHover && !shownOnFocus;
  });
}

describe("hover-revealed controls", () => {
  it("spots a control that only hover can reveal", () => {
    expect(
      hoverOnlyClassStrings('<b className="opacity-0 group-hover:opacity-100" />'),
    ).toHaveLength(1);
    expect(
      hoverOnlyClassStrings(
        '<b className="opacity-0 group-hover/row:opacity-100 group-focus-within/row:opacity-100" />',
      ),
    ).toHaveLength(0);
    expect(
      hoverOnlyClassStrings('<b className="opacity-55 group-hover:opacity-95" />'),
    ).toHaveLength(0);
  });

  it("are all reachable by keyboard focus too", () => {
    const offenders = sourceFiles("src").flatMap((file) =>
      hoverOnlyClassStrings(readFileSync(file, "utf8")).map(
        (value) => `${file}: ${value}`,
      ),
    );
    expect(offenders).toEqual([]);
  });

  it("hover styles do not depend on the (hover: hover) media query", () => {
    expect(readFileSync("src/styles/globals.css", "utf8")).toContain(
      "@custom-variant hover (&:hover);",
    );
  });
});
