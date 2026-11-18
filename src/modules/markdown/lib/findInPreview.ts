// Find-in-page for the rendered preview. Matching is done per text node, so a
// phrase split by inline formatting ("foo **bar**") is not found as one match.

export type FindMatch = {
  /** Index into the list of text nodes. */
  node: number;
  start: number;
  end: number;
};

/** Upper bound on highlighted matches, so a one-letter query stays cheap. */
export const FIND_MATCH_LIMIT = 2000;

export function findMatches(
  texts: readonly string[],
  query: string,
  limit = FIND_MATCH_LIMIT,
): FindMatch[] {
  const needle = query.toLowerCase();
  const matches: FindMatch[] = [];
  if (!needle) return matches;
  for (let node = 0; node < texts.length; node++) {
    const haystack = texts[node].toLowerCase();
    let from = haystack.indexOf(needle);
    while (from >= 0) {
      matches.push({ node, start: from, end: from + needle.length });
      if (matches.length >= limit) return matches;
      from = haystack.indexOf(needle, from + needle.length);
    }
  }
  return matches;
}

/** Next match index when stepping forwards or backwards, wrapping around. */
export function stepMatch(current: number, total: number, delta: 1 | -1): number {
  if (total <= 0) return -1;
  if (current < 0) return delta === 1 ? 0 : total - 1;
  return (current + delta + total) % total;
}
