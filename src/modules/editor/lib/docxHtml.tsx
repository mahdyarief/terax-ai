import { createElement, type ReactNode } from "react";

/**
 * Turns mammoth's HTML for a Word document into React elements, keeping only
 * an allowlist of tags and attributes. Nothing is inserted as raw HTML.
 */

const ALLOWED_TAGS = new Set([
  "a",
  "blockquote",
  "br",
  "caption",
  "code",
  "del",
  "em",
  "h1",
  "h2",
  "h3",
  "h4",
  "h5",
  "h6",
  "hr",
  "img",
  "li",
  "ol",
  "p",
  "pre",
  "s",
  "strong",
  "sub",
  "sup",
  "table",
  "tbody",
  "td",
  "th",
  "thead",
  "tr",
  "u",
  "ul",
]);

/**
 * Void elements may not be given children at all, not even an empty array:
 * React throws (minified error #137) as soon as `children` is not null.
 */
const VOID_TAGS = new Set(["br", "hr", "img"]);

const TEXT_NODE = 3;
const ELEMENT_NODE = 1;

/** The part of a DOM node the renderer reads, so it runs without a DOM. */
export type DocNode = {
  readonly nodeType: number;
  readonly textContent: string | null;
  readonly tagName?: string;
  readonly childNodes: ArrayLike<DocNode>;
  getAttribute?(name: string): string | null;
};

export function renderDocxNodes(nodes: ArrayLike<DocNode>): ReactNode[] {
  return Array.from(nodes).map((node, index) =>
    renderNode(node, `docx-${index}`),
  );
}

function renderChildren(node: DocNode, key: string): ReactNode[] {
  return Array.from(node.childNodes).map((child, index) =>
    renderNode(child, `${key}-${index}`),
  );
}

function renderNode(node: DocNode, key: string): ReactNode {
  if (node.nodeType === TEXT_NODE) return node.textContent;
  if (node.nodeType !== ELEMENT_NODE || !node.tagName) return null;
  const attr = (name: string) => node.getAttribute?.(name) ?? null;

  const tag = node.tagName.toLowerCase();
  if (!ALLOWED_TAGS.has(tag)) return renderChildren(node, key);

  if (tag === "img") {
    const src = attr("src") ?? "";
    if (!/^data:image\/(png|jpeg|gif|webp);base64,/i.test(src)) return null;
    return (
      <img
        key={key}
        src={src}
        alt={attr("alt") ?? ""}
        className="max-h-[70vh] max-w-full object-contain"
      />
    );
  }
  if (VOID_TAGS.has(tag)) return createElement(tag, { key });

  const children = renderChildren(node, key);
  if (tag === "a") {
    const href = safeLink(attr("href"));
    return href ? (
      <a key={key} href={href} target="_blank" rel="noopener noreferrer">
        {children}
      </a>
    ) : (
      children
    );
  }

  const props: Record<string, unknown> = { key };
  if (tag === "td" || tag === "th") {
    for (const name of ["colspan", "rowspan"]) {
      const value = attr(name);
      if (value && /^\d{1,3}$/.test(value)) {
        props[name === "colspan" ? "colSpan" : "rowSpan"] = Number(value);
      }
    }
  }
  return createElement(tag, props, ...children);
}

function safeLink(value: string | null): string | null {
  if (!value) return null;
  try {
    const url = new URL(value);
    return ["http:", "https:", "mailto:"].includes(url.protocol) ? value : null;
  } catch {
    return null;
  }
}
