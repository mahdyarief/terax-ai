export interface TocHeading {
  /** Unique per heading; stampHeadingIds applies it to the rendered heading. */
  id: string;
  level: number;
  text: string;
  /** 1-based source line, so repeated text still gets a unique key. */
  line: number;
}

const ATX_HEADING = /^ {0,3}(#{1,6})[ \t]+(.+)$/;
const SETEXT_UNDERLINE = /^ {0,3}(=+|-+)[ \t]*$/;
const FENCE = /^ {0,3}(`{3,}|~{3,})(.*)$/;

function stripClosingHashes(text: string): string {
  return text.replace(/[ \t]+#+[ \t]*$/, "").trim();
}

function fenceOf(line: string): { marker: string; length: number } | null {
  const match = FENCE.exec(line);
  if (!match) return null;
  return { marker: match[1][0], length: match[1].length };
}

function closesFence(line: string, fence: { marker: string; length: number }): boolean {
  const trimmed = line.trim();
  if (!trimmed.startsWith(fence.marker)) return false;
  const run = trimmed.match(new RegExp(`^\\${fence.marker}+`))?.[0].length ?? 0;
  return run >= fence.length && trimmed.slice(run).trim().length === 0;
}

/**
 * Parse the headings a GFM renderer will actually emit: fenced code is
 * skipped and setext (underlined) headings are recognised, so the outline
 * never lists a line that only looks like a heading inside a code block.
 */
export function parseHeadings(source: string): TocHeading[] {
  const lines = source.split(/\r?\n/);
  const found: Array<{ level: number; text: string; line: number }> = [];
  let fence: { marker: string; length: number } | null = null;

  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];

    if (fence) {
      if (closesFence(line, fence)) fence = null;
      continue;
    }

    const opening = fenceOf(line);
    if (opening) {
      fence = opening;
      continue;
    }

    const atx = ATX_HEADING.exec(line);
    if (atx) {
      const text = stripClosingHashes(atx[2]);
      if (text) found.push({ level: atx[1].length, text, line: i + 1 });
      continue;
    }

    const setext = SETEXT_UNDERLINE.exec(line);
    if (!setext || i === 0) continue;

    const previous = lines[i - 1];
    const text = previous.trim();
    if (
      !text ||
      ATX_HEADING.test(previous) ||
      previous.startsWith("    ") ||
      fenceOf(previous)
    ) {
      continue;
    }

    found.push({ level: setext[1][0] === "=" ? 1 : 2, text, line: i });
  }

  return found.map((heading) => ({ ...heading, id: `heading-line-${heading.line}` }));
}

/** Shallowest level in the list (6 for an empty list). */
export function minHeadingLevel(headings: TocHeading[]): number {
  return headings.reduce((min, heading) => Math.min(min, heading.level), 6);
}

/**
 * Collapse key: `level:text:n`. Unlike the ordinal id it survives edits, so a
 * collapsed section does not spring open on the next keystroke.
 */
export function headingKeys(headings: TocHeading[]): string[] {
  const seen = new Map<string, number>();
  return headings.map((heading) => {
    const base = `${heading.level}:${heading.text}`;
    const count = seen.get(base) ?? 0;
    seen.set(base, count + 1);
    return `${base}:${count}`;
  });
}

export interface OutlineRow {
  heading: TocHeading;
  key: string;
  /** 0 for the shallowest heading, driving indent and weight. */
  depth: number;
  hasChildren: boolean;
  isCollapsed: boolean;
  /** Rows folded under this one while it is collapsed. */
  hiddenCount: number;
}

export interface OutlineModel {
  rows: OutlineRow[];
  /** Every parent key, for "Collapse all". */
  parentKeys: string[];
  /** Maps a heading id to the visible row standing in for it. */
  visibleIdFor: Map<string, string>;
}

export function buildOutlineModel(
  headings: TocHeading[],
  collapsedKeys: ReadonlySet<string>,
): OutlineModel {
  const keys = headingKeys(headings);
  const shallowest = minHeadingLevel(headings);
  const rows: OutlineRow[] = [];
  const parentKeys: string[] = [];
  const visibleIdFor = new Map<string, string>();
  const open: Array<{ level: number; collapsedRow: OutlineRow | null }> = [];

  headings.forEach((heading, index) => {
    while (open.length > 0 && open[open.length - 1].level >= heading.level) {
      open.pop();
    }

    const next = headings[index + 1];
    const hasChildren = Boolean(next && next.level > heading.level);
    const key = keys[index];
    if (hasChildren) parentKeys.push(key);

    const hiddenBy = open.find((entry) => entry.collapsedRow !== null)?.collapsedRow ?? null;
    if (hiddenBy) {
      hiddenBy.hiddenCount += 1;
      visibleIdFor.set(heading.id, hiddenBy.heading.id);
      open.push({ level: heading.level, collapsedRow: null });
      return;
    }

    const isCollapsed = hasChildren && collapsedKeys.has(key);
    const row: OutlineRow = {
      heading,
      key,
      depth: Math.max(0, heading.level - shallowest),
      hasChildren,
      isCollapsed,
      hiddenCount: 0,
    };
    rows.push(row);
    visibleIdFor.set(heading.id, heading.id);
    open.push({ level: heading.level, collapsedRow: isCollapsed ? row : null });
  });

  return { rows, parentKeys, visibleIdFor };
}

/** "H1–H3", or "H1" for a single level. */
export function formatDepthLabel(level: number): string {
  return level <= 1 ? "H1" : `H1–H${level}`;
}
