import { describe, expect, it } from "vitest";
import { parseHeadings } from "@/components/markdown/useMarkdownHeadings";
import { normalizeHeadingText, stampHeadingIds } from "./stampHeadingIds";

type FakeHeading = HTMLElement;

/** Minimal stand-in: the stamping logic only reads tagName/textContent and
 * writes id/dataset, so a plain object is enough and avoids a DOM dependency. */
function fakeRoot(entries: Array<[number, string]>): {
  root: ParentNode;
  nodes: FakeHeading[];
} {
  const nodes = entries.map(([level, text]) => {
    const el = { tagName: `H${level}`, textContent: text, id: "", dataset: {} as DOMStringMap };
    return el as unknown as FakeHeading;
  });
  const root = {
    querySelectorAll: () => nodes as unknown as NodeListOf<HTMLElement>,
  } as unknown as ParentNode;
  return { root, nodes };
}

describe("normalizeHeadingText", () => {
  it("drops inline markdown and collapses whitespace", () => {
    expect(normalizeHeadingText("**Setup**  the `app`")).toBe("setup the app");
  });

  it("keeps link text but drops the target", () => {
    expect(normalizeHeadingText("See [docs](https://x.dev/a)")).toBe("see docs");
  });

  it("drops raw html tags", () => {
    expect(normalizeHeadingText("Hello<br/>world")).toBe("helloworld");
  });
});

describe("stampHeadingIds", () => {
  it("does not let a non-heading line steal an outline entry", () => {
    const source = "# Setup\n\n**Install** the app\n\n## Usage";
    // The renderer emits an <h2> for a line the outline parser skipped.
    const { root, nodes } = fakeRoot([
      [1, "Setup"],
      [2, "Install the app"],
      [2, "Usage"],
    ]);
    const headings = parseHeadings(source);
    stampHeadingIds(root, headings);

    expect(nodes[0].id).toBe(headings[0].id);
    expect(nodes[1].id).toBe(""); // left unstamped: not in the outline
    expect(nodes[2].dataset.heading).toBe("2:Usage:0");
  });

  it("indexes repeated heading text by occurrence", () => {
    const { root, nodes } = fakeRoot([
      [2, "Notes"],
      [2, "Notes"],
    ]);
    stampHeadingIds(root, parseHeadings("## Notes\n\n## Notes"));

    expect(nodes.map((n) => n.dataset.heading)).toEqual([
      "2:Notes:0",
      "2:Notes:1",
    ]);
  });

  it("skips an unstampable heading without desyncing the rest", () => {
    const { root, nodes } = fakeRoot([
      [3, "Rendered but not in source"],
      [1, "Setup"],
      [2, "Usage"],
    ]);
    const headings = parseHeadings("# Setup\n\n## Usage");
    stampHeadingIds(root, headings);

    expect(nodes[0].id).toBe("");
    expect(nodes[1].id).toBe(headings[0].id);
    expect(nodes[2].id).toBe(headings[1].id);
  });
});
