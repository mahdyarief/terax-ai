import { EditorSelection, EditorState } from "@codemirror/state";
import { describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn() }));
vi.mock("./languageDefinitions", () => ({ LANGUAGES: [] }));

import { linkPasteSpec, toggleBold, toggleItalic } from "./markdownExtras";

/** `[` and `]` mark the selection; the result is written the same way. */
function run(
  command: typeof toggleBold,
  marked: string,
): string {
  const from = marked.indexOf("[");
  const to = marked.indexOf("]") - 1;
  let state = EditorState.create({
    doc: marked.replace("[", "").replace("]", ""),
    selection: EditorSelection.range(from, to),
  });
  command({
    state,
    dispatch: (transaction) => {
      state = transaction.state;
    },
  });
  const { from: start, to: end } = state.selection.main;
  const text = state.doc.toString();
  return `${text.slice(0, start)}[${text.slice(start, end)}]${text.slice(end)}`;
}

describe("markdown emphasis shortcuts", () => {
  it("wraps and unwraps bold", () => {
    expect(run(toggleBold, "a [word] b")).toBe("a **[word]** b");
    expect(run(toggleBold, "a **[word]** b")).toBe("a [word] b");
  });

  it("wraps and unwraps italic", () => {
    expect(run(toggleItalic, "a [word] b")).toBe("a *[word]* b");
    expect(run(toggleItalic, "a *[word]* b")).toBe("a [word] b");
  });

  it("keeps bold and italic independent", () => {
    expect(run(toggleItalic, "**[word]**")).toBe("***[word]***");
    expect(run(toggleItalic, "***[word]***")).toBe("**[word]**");
    expect(run(toggleBold, "*[word]*")).toBe("***[word]***");
    expect(run(toggleBold, "***[word]***")).toBe("*[word]*");
  });

  it("inserts an empty pair at a bare cursor", () => {
    expect(run(toggleBold, "a [] b")).toBe("a **[]** b");
  });
});

describe("pasting a URL over a selection", () => {
  const stateWith = (doc: string, from: number, to: number) =>
    EditorState.create({ doc, selection: EditorSelection.range(from, to) });

  it("turns the selected text into a link", () => {
    const state = stateWith("see the docs here", 8, 12);
    const spec = linkPasteSpec(state, " https://example.com/a ");
    expect(spec).not.toBeNull();
    expect(state.update(spec ?? {}).state.doc.toString()).toBe(
      "see the [docs](https://example.com/a) here",
    );
  });

  it("pastes normally otherwise", () => {
    expect(linkPasteSpec(stateWith("docs", 0, 0), "https://a.dev")).toBeNull();
    expect(linkPasteSpec(stateWith("docs", 0, 4), "not a url")).toBeNull();
    expect(
      linkPasteSpec(stateWith("https://old.dev", 0, 15), "https://a.dev"),
    ).toBeNull();
    expect(linkPasteSpec(stateWith("a\nb", 0, 3), "https://a.dev")).toBeNull();
  });
});
