import type { TocHeading } from "@/components/markdown/useMarkdownHeadings";
import { isExternalUrl } from "@/lib/external-link";
import { resolveRelativeAsset } from "./assetPath";

/**
 * GitHub's heading anchor: lower-case, inline markup dropped, everything but
 * letters, digits, spaces, hyphens and underscores removed, spaces to hyphens.
 */
export function slugify(text: string): string {
  return text
    .replace(/!?\[([^\]]*)\]\([^)]*\)/g, "$1")
    .replace(/<[^>]+>/g, "")
    .replace(/[`*~]/g, "")
    .trim()
    .toLowerCase()
    .replace(/[^\p{L}\p{N}\p{M} _-]/gu, "")
    .replace(/ /g, "-");
}

/** One anchor per heading; repeats get `-1`, `-2`, as GitHub numbers them. */
export function headingSlugs(headings: readonly TocHeading[]): string[] {
  const seen = new Map<string, number>();
  return headings.map((heading) => {
    const base = slugify(heading.text);
    const count = seen.get(base) ?? 0;
    seen.set(base, count + 1);
    return count === 0 ? base : `${base}-${count}`;
  });
}

/** The heading a `#fragment` points at, or null. */
export function headingForFragment(
  headings: readonly TocHeading[],
  fragment: string,
): TocHeading | null {
  const want = fragment.toLowerCase();
  const index = headingSlugs(headings).indexOf(want);
  return index < 0 ? null : headings[index];
}

type HastNode = {
  type: string;
  tagName?: string;
  properties?: Record<string, unknown>;
  children?: HastNode[];
};

/**
 * Copies each link's address to `data-href` before the hardening pass runs.
 * That pass rewrites relative addresses against a dummy origin, which turns
 * `./a.md` and `../a.md` alike into `/a.md`; the click handler needs the
 * address as the author wrote it.
 */
export function rehypeKeepHref() {
  const visit = (node: HastNode) => {
    if (node.type === "element" && node.tagName === "a") {
      const href = node.properties?.href;
      if (typeof href === "string") {
        node.properties = { ...node.properties, dataHref: href };
      }
    }
    for (const child of node.children ?? []) visit(child);
  };
  return (tree: HastNode) => visit(tree);
}

export type LinkTarget =
  | { kind: "external" }
  /** A heading (or element id) in the same document. */
  | { kind: "anchor"; fragment: string }
  /** Another file, addressed relative to the document. */
  | { kind: "file"; path: string; fragment: string | null }
  /** Anything else: absolute paths, `file:`, `javascript:`, malformed text. */
  | { kind: "blocked" };

function decode(text: string): string {
  try {
    return decodeURIComponent(text);
  } catch {
    return text;
  }
}

export function classifyLink(href: string, documentPath: string): LinkTarget {
  const trimmed = href.trim();
  if (!trimmed) return { kind: "blocked" };
  if (isExternalUrl(trimmed)) return { kind: "external" };
  if (trimmed.startsWith("#")) {
    return { kind: "anchor", fragment: decode(trimmed.slice(1)) };
  }
  const path = resolveRelativeAsset(documentPath, trimmed);
  if (!path) return { kind: "blocked" };
  const hash = trimmed.indexOf("#");
  return {
    kind: "file",
    path,
    fragment: hash < 0 ? null : decode(trimmed.slice(hash + 1)) || null,
  };
}
