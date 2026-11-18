// YAML frontmatter is metadata, not prose. Rendered as markdown it turns into a
// rule followed by a huge "key: value" heading, so the preview lifts it out.

export type Frontmatter = {
  /** The YAML between the fences, without them. */
  raw: string;
  /** Top-level `key: value` pairs, or null when the YAML is not that simple. */
  fields: Array<[key: string, value: string]> | null;
};

export type SplitDocument = {
  frontmatter: Frontmatter | null;
  /** The document without its frontmatter. */
  body: string;
  /** Lines removed from the top, so body line numbers map back to the file. */
  lineOffset: number;
};

const FENCE = /^---[ \t]*$/;
const CLOSING_FENCE = /^(?:---|\.\.\.)[ \t]*$/;
const FIELD = /^([A-Za-z_][\w.-]*):[ \t]*(.*)$/;

function unquote(value: string): string {
  const quoted = /^(["'])(.*)\1$/.exec(value);
  return quoted ? quoted[2] : value;
}

function simpleFields(lines: string[]): Frontmatter["fields"] {
  const fields: Array<[string, string]> = [];
  for (const line of lines) {
    if (!line.trim() || line.trimStart().startsWith("#")) continue;
    const match = FIELD.exec(line);
    if (!match) return null;
    const value = match[2].trim();
    // An empty value or a block marker starts a nested structure.
    if (!value || value === "|" || value === ">") return null;
    fields.push([match[1], unquote(value)]);
  }
  return fields.length > 0 ? fields : null;
}

export function splitFrontmatter(source: string): SplitDocument {
  const none: SplitDocument = { frontmatter: null, body: source, lineOffset: 0 };
  const text = source.charCodeAt(0) === 0xfeff ? source.slice(1) : source;
  const lines = text.split(/\r?\n/);
  if (!FENCE.test(lines[0] ?? "")) return none;

  const close = lines.findIndex(
    (line, index) => index > 0 && CLOSING_FENCE.test(line),
  );
  if (close < 0) return none;
  const yaml = lines.slice(1, close);
  // `---` twice in a row, or prose between two rules, is not frontmatter.
  if (!yaml.some((line) => FIELD.test(line))) return none;

  return {
    frontmatter: { raw: yaml.join("\n"), fields: simpleFields(yaml) },
    body: lines.slice(close + 1).join("\n"),
    lineOffset: close + 1,
  };
}
