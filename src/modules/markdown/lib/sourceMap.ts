// Maps between a line of the markdown source and a scroll offset in the
// rendered preview. Headings are the only places where both are known, so
// positions between two headings are interpolated.

export type Anchor = {
  /** 1-based source line. */
  line: number;
  /** Scroll offset (px) of the matching rendered element. */
  offset: number;
};

/**
 * Anchors for the whole document: its start, each rendered heading, and its
 * end. Headings that go backwards in either axis are dropped, so both lookups
 * stay monotonic.
 */
export function buildAnchors(
  headings: readonly Anchor[],
  totalLines: number,
  scrollHeight: number,
): Anchor[] {
  const anchors: Anchor[] = [{ line: 1, offset: 0 }];
  for (const heading of headings) {
    const last = anchors[anchors.length - 1];
    if (heading.line > last.line && heading.offset >= last.offset) {
      anchors.push(heading);
    }
  }
  const last = anchors[anchors.length - 1];
  const endLine = Math.max(totalLines + 1, last.line + 1);
  anchors.push({ line: endLine, offset: Math.max(scrollHeight, last.offset) });
  return anchors;
}

function interpolate(
  anchors: readonly Anchor[],
  value: number,
  from: "line" | "offset",
  to: "line" | "offset",
): number {
  if (anchors.length === 0) return 0;
  if (value <= anchors[0][from]) return anchors[0][to];
  for (let i = 1; i < anchors.length; i++) {
    const before = anchors[i - 1];
    const after = anchors[i];
    if (value > after[from]) continue;
    const span = after[from] - before[from];
    const ratio = span === 0 ? 0 : (value - before[from]) / span;
    return before[to] + ratio * (after[to] - before[to]);
  }
  return anchors[anchors.length - 1][to];
}

/** Scroll offset at which `line` sits at the top of the preview. */
export function offsetForLine(anchors: readonly Anchor[], line: number): number {
  return Math.round(interpolate(anchors, line, "line", "offset"));
}

/** Source line shown at scroll offset `offset`. */
export function lineForOffset(
  anchors: readonly Anchor[],
  offset: number,
): number {
  return Math.max(1, Math.floor(interpolate(anchors, offset, "offset", "line")));
}
