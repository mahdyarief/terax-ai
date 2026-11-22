import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { type DocNode, renderDocxNodes } from "./docxHtml";

const text = (value: string): DocNode => ({
  nodeType: 3,
  textContent: value,
  childNodes: [],
});

const el = (
  tagName: string,
  children: DocNode[] = [],
  attrs: Record<string, string> = {},
): DocNode => ({
  nodeType: 1,
  textContent: null,
  tagName: tagName.toUpperCase(),
  childNodes: children,
  getAttribute: (name) => attrs[name] ?? null,
});

const render = (...nodes: DocNode[]) =>
  renderToStaticMarkup(createElement("div", null, ...renderDocxNodes(nodes)));

describe("renderDocxNodes", () => {
  it("renders line breaks and rules without children", () => {
    // A soft line break (Shift+Enter) in Word comes out of mammoth as <br>.
    // Handing it an empty children array threw React error #137.
    expect(render(el("p", [text("a"), el("br"), text("b")]), el("hr"))).toBe(
      "<div><p>a<br/>b</p><hr/></div>",
    );
  });

  it("drops a void element's stray children instead of throwing", () => {
    expect(render(el("br", [text("x")]))).toBe("<div><br/></div>");
  });

  it("unwraps tags outside the allowlist but keeps their text", () => {
    expect(render(el("span", [text("kept")]), el("script", [text("x")]))).toBe(
      "<div>keptx</div>",
    );
  });

  it("keeps only safe links and inline images", () => {
    expect(
      render(
        el("a", [text("ok")], { href: "https://example.com" }),
        el("a", [text("js")], { href: "javascript:alert(1)" }),
        el("img", [], { src: "https://example.com/x.png" }),
      ),
    ).toBe(
      '<div><a href="https://example.com" target="_blank" rel="noopener noreferrer">ok</a>js</div>',
    );
    expect(
      render(el("img", [], { src: "data:image/png;base64,AAAA", alt: "pic" })),
    ).toContain('src="data:image/png;base64,AAAA" alt="pic"');
  });

  it("passes numeric table spans only", () => {
    expect(
      render(
        el("table", [
          el("tbody", [
            el("tr", [
              el("td", [text("1")], { colspan: "2" }),
              el("td", [text("2")], { rowspan: "x" }),
            ]),
          ]),
        ]),
      ),
    ).toBe(
      '<div><table><tbody><tr><td colSpan="2">1</td><td>2</td></tr></tbody></table></div>',
    );
  });
});
