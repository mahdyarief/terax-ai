// Math support for the preview. The parser only loads when a document looks
// like it contains math, and `$...$` is only read as math when the document
// clearly uses TeX inside single dollars, so "$5 and $10" stays prose.

export type MathSyntax = {
  /** Read `$x$` as inline math, not just `$$x$$` and ```math fences. */
  singleDollar: boolean;
};

const FENCED_CODE = /^ {0,3}(`{3,}|~{3,})[^\n]*\n[\s\S]*?^ {0,3}\1[`~]*[ \t]*$/gm;
const INLINE_CODE = /`[^`\n]*`/g;
const MATH_FENCE = /^ {0,3}(?:`{3,}|~{3,})[ \t]*math\b/m;
/** `$...$` on one line whose content has a TeX command, `^`, `_` or braces. */
const TEX_IN_SINGLE_DOLLARS = /(?<![\\$])\$(?!\$)[^$\n]*(?:\\[a-zA-Z]+|[\^_{}])[^$\n]*\$/;

export function mathSyntaxOf(source: string): MathSyntax | null {
  if (!source.includes("$") && !MATH_FENCE.test(source)) return null;
  const hasFence = MATH_FENCE.test(source);
  const prose = source.replace(FENCED_CODE, "").replace(INLINE_CODE, "");
  const singleDollar = TEX_IN_SINGLE_DOLLARS.test(prose);
  if (!hasFence && !singleDollar && !prose.includes("$$")) return null;
  return { singleDollar };
}

const FENCE_LINE = /^ {0,3}(`{3,}|~{3,})/;

function isSpace(char: string | undefined): boolean {
  return char === undefined || /\s/.test(char);
}

/** Index of the `$` that closes inline math opened at `open`, or -1. */
function closingDollar(line: string, open: number): number {
  for (let i = open + 2; i < line.length; i++) {
    if (line[i] === "\\") {
      i += 1;
      continue;
    }
    if (line[i] === "`") return -1;
    if (line[i] !== "$" || line[i + 1] === "$") continue;
    // As on GitHub: no space before the closing dollar, no digit after it.
    if (!isSpace(line[i - 1]) && !/\d/.test(line[i + 1] ?? "")) return i;
    // A dollar that starts a new word opens something else ("$5, then $x$"),
    // so the one being matched was never math.
    if (isSpace(line[i - 1]) && !isSpace(line[i + 1])) return -1;
  }
  return -1;
}

function escapeLine(line: string): string {
  let out = "";
  for (let i = 0; i < line.length; i++) {
    const char = line[i];
    if (char === "\\") {
      out += line.slice(i, i + 2);
      i += 1;
    } else if (char === "`") {
      // Copy a code span whole; its dollars are literal already.
      let run = 1;
      while (line[i + run] === "`") run += 1;
      const ticks = "`".repeat(run);
      const close = line.indexOf(ticks, i + run);
      const end = close < 0 ? i + run : close + run;
      out += line.slice(i, end);
      i = end - 1;
    } else if (char !== "$") {
      out += char;
    } else if (line[i + 1] === "$") {
      out += "$$";
      i += 1;
    } else {
      const close = isSpace(line[i + 1]) ? -1 : closingDollar(line, i);
      if (close < 0) out += "\\$";
      else {
        out += line.slice(i, close + 1);
        i = close;
      }
    }
  }
  return out;
}

/**
 * Escapes single dollars that do not delimit math, so "$5 and $10" stays text
 * in a document that also uses `$x$`. The math parser itself accepts any pair
 * of dollars. Code and `$$` are left alone, and no line is added or removed.
 */
export function escapeLooseDollars(source: string): string {
  let fence: string | null = null;
  return source
    .split("\n")
    .map((line) => {
      const marker = FENCE_LINE.exec(line)?.[1];
      if (fence) {
        if (marker?.[0] === fence[0] && marker.length >= fence.length) {
          fence = null;
        }
        return line;
      }
      if (marker) {
        fence = marker;
        return line;
      }
      return line.includes("$") ? escapeLine(line) : line;
    })
    .join("\n");
}

type HastNode = {
  type: string;
  tagName?: string;
  properties?: Record<string, unknown>;
  children?: HastNode[];
};

function isMathCode(node: HastNode): boolean {
  if (node.type !== "element" || node.tagName !== "code") return false;
  const classes = node.properties?.className;
  return Array.isArray(classes) && classes.includes("language-math");
}

/**
 * Marks the math the markdown parser found so the `code` renderer can typeset
 * it: `data-math="display"` for a block (and its `<pre>` wrapper is removed),
 * `data-math="inline"` otherwise. Runs after sanitizing, which keeps the
 * `language-math` class but drops the display/inline ones.
 */
export function rehypeMarkMath() {
  const visit = (node: HastNode) => {
    if (!node.children) return;
    node.children = node.children.map((child) => {
      if (child.type === "element" && child.tagName === "pre") {
        const inner = child.children?.filter((n) => n.type === "element") ?? [];
        if (inner.length === 1 && isMathCode(inner[0])) {
          inner[0].properties = { ...inner[0].properties, dataMath: "display" };
          return inner[0];
        }
      }
      if (isMathCode(child)) {
        child.properties = { ...child.properties, dataMath: "inline" };
        return child;
      }
      visit(child);
      return child;
    });
  };
  return (tree: HastNode) => visit(tree);
}
