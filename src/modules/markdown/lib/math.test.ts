import { describe, expect, it } from "vitest";
import { escapeLooseDollars, mathSyntaxOf, rehypeMarkMath } from "./math";

describe("mathSyntaxOf", () => {
  it("finds no math in ordinary documents, even with dollar amounts", () => {
    expect(mathSyntaxOf("# Notes\n\nplain text")).toBeNull();
    expect(mathSyntaxOf("It costs $5 and then $10 more.")).toBeNull();
    expect(mathSyntaxOf("Run `echo $HOME_DIR $PATH_X` first.")).toBeNull();
    expect(mathSyntaxOf("```sh\necho $A_B $C_D\n```\n")).toBeNull();
  });

  it("enables block math without reading single dollars as math", () => {
    expect(mathSyntaxOf("$$\nE = mc^2\n$$\nIt costs $5 and $10.")).toEqual({
      singleDollar: false,
    });
    expect(mathSyntaxOf("```math\nx^2\n```\n")).toEqual({ singleDollar: false });
  });

  it("enables single dollars when they clearly hold TeX", () => {
    expect(mathSyntaxOf("Euler: $e^{i\\pi} + 1 = 0$")).toEqual({
      singleDollar: true,
    });
    expect(mathSyntaxOf("Let $x_1$ be the root.")).toEqual({
      singleDollar: true,
    });
  });
});

describe("escapeLooseDollars", () => {
  it("escapes dollar amounts and leaves real math alone", () => {
    expect(escapeLooseDollars("It costs $5 and $10.")).toBe(
      "It costs \\$5 and \\$10.",
    );
    expect(escapeLooseDollars("Euler $e^{i\\pi} + 1 = 0$ here")).toBe(
      "Euler $e^{i\\pi} + 1 = 0$ here",
    );
    expect(escapeLooseDollars("Paid $5, then $x_1$ holds")).toBe(
      "Paid \\$5, then $x_1$ holds",
    );
  });

  it("does not touch code, display math or escaped dollars", () => {
    const source = [
      "Use `$HOME` and $ alone",
      "```sh",
      "echo $A $B",
      "```",
      "$$",
      "x + y",
      "$$",
      "inline $$z$$ and \\$9",
    ].join("\n");
    expect(escapeLooseDollars(source)).toBe(
      source.replace("and $ alone", "and \\$ alone"),
    );
  });

  it("keeps the line count", () => {
    const source = "a $1\n\nb $2\n";
    expect(escapeLooseDollars(source).split("\n")).toHaveLength(4);
  });
});

describe("rehypeMarkMath", () => {
  const code = (classes: string[], text: string) => ({
    type: "element",
    tagName: "code",
    properties: { className: classes },
    children: [{ type: "text", value: text }],
  });

  it("marks inline math and unwraps display math from its <pre>", () => {
    const tree = {
      type: "root",
      children: [
        {
          type: "element",
          tagName: "p",
          properties: {},
          children: [code(["language-math"], "x^2")],
        },
        {
          type: "element",
          tagName: "pre",
          properties: {},
          children: [code(["language-math"], "y = 1")],
        },
        {
          type: "element",
          tagName: "pre",
          properties: {},
          children: [code(["language-ts"], "const a = 1")],
        },
      ],
    };
    rehypeMarkMath()(tree);
    const [paragraph, display, plain] = tree.children as Array<{
      tagName: string;
      properties: Record<string, unknown>;
      children: Array<{ properties: Record<string, unknown> }>;
    }>;
    expect(paragraph.children[0].properties.dataMath).toBe("inline");
    expect(display.tagName).toBe("code");
    expect(display.properties.dataMath).toBe("display");
    expect(plain.tagName).toBe("pre");
    expect(plain.children[0].properties.dataMath).toBeUndefined();
  });
});
