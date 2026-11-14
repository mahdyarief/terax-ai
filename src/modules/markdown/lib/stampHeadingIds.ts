import {
  headingKeys,
  type TocHeading,
} from "@/components/markdown/useMarkdownHeadings";

/**
 * Compare headings ignoring inline markup and whitespace, because the outline
 * is parsed from the source (`**Setup**`) while the DOM holds the rendered
 * text (`Setup`).
 */
export function normalizeHeadingText(text: string): string {
  return text
    .replace(/!?\[([^\]]*)\]\([^)]*\)/g, "$1")
    .replace(/<[^>]+>/g, "")
    .replace(/[`*_~]/g, "")
    .replace(/\s+/g, " ")
    .trim()
    .toLowerCase();
}

/**
 * Stamp outline ids onto the rendered headings.
 *
 * Streamdown renders markdown in independent blocks, so source line numbers in
 * the parsed tree are block-local and cannot serve as ids. Matching in document
 * order against the outline instead keeps click-to-scroll correct, and skipping
 * headings the parser did not list (raw HTML, say) keeps later ids aligned.
 */
export function stampHeadingIds(
  root: ParentNode,
  headings: TocHeading[],
): void {
  const nodes = Array.from(
    root.querySelectorAll<HTMLElement>("h1,h2,h3,h4,h5,h6"),
  );
  const keys = headingKeys(headings);
  let cursor = 0;

  for (const node of nodes) {
    if (cursor >= headings.length) break;
    const level = Number(node.tagName.slice(1));
    const text = normalizeHeadingText(node.textContent ?? "");
    let found = -1;
    for (let i = cursor; i < headings.length; i++) {
      const heading = headings[i];
      if (heading.level !== level) continue;
      if (normalizeHeadingText(heading.text) !== text) continue;
      found = i;
      break;
    }
    if (found === -1) continue;

    const heading = headings[found];
    node.id = heading.id;
    node.dataset.headingId = heading.id;
    node.dataset.heading = keys[found];
    node.dataset.depth = String(heading.level);
    cursor = found + 1;
  }
}
