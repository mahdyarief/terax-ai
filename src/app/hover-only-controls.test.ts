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

const HIDDEN = new Set(["opacity-0", "hidden", "invisible"]);
const SHOWN = "(opacity-|(inline-)?(flex|block|grid)$|visible$)";
const ON_HOVER = new RegExp(`^(group|peer)-hover(/[a-z0-9_-]+)?:${SHOWN}`);
const ON_FOCUS = new RegExp(
  `^(focus-visible|focus-within|(group|peer)-focus[a-z0-9_-]*(/[a-z0-9_-]+)?):${SHOWN}`,
);

/** Class strings that hide an element until its group or peer is hovered. */
function hoverOnlyClassStrings(source: string): string[] {
  const strings = source.match(/"[^"\n]*"|`[^`]*`/g) ?? [];
  return strings.filter((value) => {
    const classes = value.slice(1, -1).split(/\s+/);
    return (
      classes.some((name) => HIDDEN.has(name)) &&
      classes.some((name) => ON_HOVER.test(name)) &&
      !classes.some((name) => ON_FOCUS.test(name))
    );
  });
}

describe("hover-revealed controls", () => {
  it("spots a control that only hover can reveal", () => {
    const found = (markup: string) => hoverOnlyClassStrings(markup).length;
    expect(found('<b className="opacity-0 group-hover:opacity-100" />')).toBe(1);
    expect(found('<b className="hidden group-hover/row:flex" />')).toBe(1);
    expect(found('<b className="invisible peer-hover:visible" />')).toBe(1);
    expect(
      found(
        '<b className="opacity-0 group-hover/row:opacity-100 group-focus-within/row:opacity-100" />',
      ),
    ).toBe(0);
    expect(
      found('<b className="hidden group-hover:flex group-focus-within:flex" />'),
    ).toBe(0);
    expect(found('<b className="opacity-55 group-hover:opacity-95" />')).toBe(0);
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
