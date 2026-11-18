import {
  Language,
  LanguageDescription,
  LanguageSupport,
} from "@codemirror/language";
import {
  EditorSelection,
  type EditorState,
  type Extension,
  type StateCommand,
  type TransactionSpec,
} from "@codemirror/state";
import {
  Decoration,
  type DecorationSet,
  EditorView,
  keymap,
  MatchDecorator,
  ViewPlugin,
  type ViewUpdate,
} from "@codemirror/view";
import { openUrl } from "@tauri-apps/plugin-opener";
import { LANGUAGES } from "./languageDefinitions";

// Fence-info strings (```ts, ```python) resolve against the same lazy
// loaders as file extensions; nothing loads until a fence names it. Lives
// in this lazy chunk so languageDefinitions stays out of the eager bundle.
let mdCodeLanguages: LanguageDescription[] | null = null;
export function markdownCodeLanguages(): LanguageDescription[] {
  if (mdCodeLanguages) return mdCodeLanguages;
  mdCodeLanguages = LANGUAGES.filter(
    (l) => l.name !== "Markdown" && l.name !== "Terax Theme",
  ).map((l) =>
    LanguageDescription.of({
      name: l.name,
      alias: l.extensions,
      extensions: l.extensions,
      load: async () => {
        const ext = await l.loader();
        if (ext instanceof LanguageSupport) return ext;
        if (ext instanceof Language) return new LanguageSupport(ext);
        throw new Error(`${l.name} is not usable inside markdown fences`);
      },
    }),
  );
  return mdCodeLanguages;
}

const URL_RE = /https?:\/\/[^\s<>"')\]]+[^\s<>"')\].,;:!?]/g;

const urlMatcher = new MatchDecorator({
  regexp: URL_RE,
  decoration: Decoration.mark({
    class: "cm-md-url",
    attributes: { title: "Cmd/Ctrl+Click to open" },
  }),
});

const urlHighlighter = ViewPlugin.fromClass(
  class {
    decorations: DecorationSet;
    constructor(view: EditorView) {
      this.decorations = urlMatcher.createDeco(view);
    }
    update(u: ViewUpdate) {
      this.decorations = urlMatcher.updateDeco(u, this.decorations);
    }
  },
  { decorations: (v) => v.decorations },
);

function urlAt(view: EditorView, pos: number): string | null {
  const line = view.state.doc.lineAt(pos);
  URL_RE.lastIndex = 0;
  let m: RegExpExecArray | null;
  while ((m = URL_RE.exec(line.text)) !== null) {
    const from = line.from + m.index;
    if (from > pos) break;
    if (pos <= from + m[0].length) return m[0];
  }
  return null;
}

const TASK_RE = /^(\s*(?:[-*+]|\d+[.)])\s+\[)([ xX])\]/;

const clickHandlers = EditorView.domEventHandlers({
  mousedown: (event, view) => {
    if (event.button !== 0) return false;
    const pos = view.posAtCoords({ x: event.clientX, y: event.clientY });
    if (pos == null) return false;

    if (event.metaKey || event.ctrlKey) {
      const url = urlAt(view, pos);
      if (!url) return false;
      void openUrl(url).catch(console.error);
      return true;
    }

    const line = view.state.doc.lineAt(pos);
    const m = TASK_RE.exec(line.text);
    if (!m) return false;
    const boxPos = line.from + m[1].length;
    if (pos < boxPos - 1 || pos > boxPos + 2) return false;
    view.dispatch({
      changes: {
        from: boxPos,
        to: boxPos + 1,
        insert: m[2] === " " ? "x" : " ",
      },
      userEvent: "input",
    });
    return true;
  },
});

const mdTheme = EditorView.theme({
  ".cm-md-url": {
    color: "var(--primary)",
    textDecoration: "underline",
    textUnderlineOffset: "2.5px",
    textDecorationColor: "color-mix(in srgb, var(--primary) 45%, transparent)",
  },
});

/** Length of the run of `char` ending just before `pos` (or starting at it). */
function runLength(
  state: EditorState,
  pos: number,
  char: string,
  direction: -1 | 1,
): number {
  let length = 0;
  for (;;) {
    const at = direction === -1 ? pos - length - 1 : pos + length;
    if (at < 0 || at >= state.doc.length) break;
    if (state.sliceDoc(at, at + 1) !== char) break;
    length += 1;
  }
  return length;
}

/**
 * Wraps each selection in an emphasis marker, or removes the marker when the
 * selection already carries it. Counting the asterisks on both sides keeps
 * bold and italic independent: `***x***` is both, so italic strips one
 * asterisk and bold strips two.
 */
function toggleEmphasis(marker: "*" | "**"): StateCommand {
  const size = marker.length;
  return ({ state, dispatch }) => {
    const spec = state.changeByRange((range) => {
      const { from, to } = range;
      const stars = Math.min(
        runLength(state, from, "*", -1),
        runLength(state, to, "*", 1),
      );
      const wrapped = size === 2 ? stars >= 2 : stars % 2 === 1;
      if (wrapped) {
        return {
          changes: [
            { from: from - size, to: from },
            { from: to, to: to + size },
          ],
          range: EditorSelection.range(from - size, to - size),
        };
      }
      return {
        changes: [
          { from, insert: marker },
          { from: to, insert: marker },
        ],
        range: EditorSelection.range(from + size, to + size),
      };
    });
    dispatch(state.update(spec, { scrollIntoView: true, userEvent: "input" }));
    return true;
  };
}

export const toggleBold = toggleEmphasis("**");
export const toggleItalic = toggleEmphasis("*");

const BARE_URL = /^https?:\/\/\S+$/;

/**
 * Pasting a URL over selected text makes a link out of it. Returns null when
 * the paste should behave normally: nothing selected, several lines selected,
 * the clipboard is not a single URL, or the selection is itself a URL.
 */
export function linkPasteSpec(
  state: EditorState,
  pasted: string,
): TransactionSpec | null {
  const url = pasted.trim();
  if (!BARE_URL.test(url) || state.selection.ranges.length !== 1) return null;
  const { from, to } = state.selection.main;
  if (from === to) return null;
  const text = state.sliceDoc(from, to);
  if (text.includes("\n") || BARE_URL.test(text.trim())) return null;
  const insert = `[${text}](${url})`;
  return {
    changes: { from, to, insert },
    selection: EditorSelection.cursor(from + insert.length),
    userEvent: "input.paste",
  };
}

const pasteHandler = EditorView.domEventHandlers({
  paste: (event, view) => {
    const spec = linkPasteSpec(
      view.state,
      event.clipboardData?.getData("text/plain") ?? "",
    );
    if (!spec) return false;
    event.preventDefault();
    view.dispatch(spec);
    return true;
  },
});

const formattingKeys = keymap.of([
  { key: "Mod-b", run: toggleBold },
  { key: "Mod-i", run: toggleItalic },
]);

export function markdownExtras(): Extension {
  return [urlHighlighter, clickHandlers, pasteHandler, formattingKeys, mdTheme];
}
