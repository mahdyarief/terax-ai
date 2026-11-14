import { describe, expect, it } from "vitest";
import { MRU_TAB_BUDGET, planTabRetention } from "./planTabRetention";
import type {
  EditorTab,
  MarkdownTab,
  PreviewTab,
  Tab,
  TerminalTab,
} from "./useTabs";

const terminal: TerminalTab = {
  id: 1,
  kind: "terminal",
  spaceId: "one",
  title: "shell",
  paneTree: { kind: "leaf", id: 2 },
  activeLeafId: 2,
};

function editor(id: number, over: Partial<EditorTab> = {}): EditorTab {
  return {
    id,
    kind: "editor",
    spaceId: "one",
    title: `e${id}`,
    path: `/e${id}.rs`,
    dirty: false,
    preview: false,
    ...over,
  };
}

function markdown(id: number, over: Partial<MarkdownTab> = {}): MarkdownTab {
  return {
    id,
    kind: "markdown",
    spaceId: "one",
    title: `m${id}`,
    path: `/m${id}.md`,
    ...over,
  };
}

function preview(id: number, over: Partial<PreviewTab> = {}): PreviewTab {
  return {
    id,
    kind: "preview",
    spaceId: "one",
    title: `p${id}`,
    url: `http://p${id}`,
    ...over,
  };
}

const coldSet = (tabs: Tab[]) =>
  new Set(tabs.filter((t) => t.cold === true).map((t) => t.id));

describe("planTabRetention", () => {
  it("returns null when nothing changes", () => {
    const tabs = [terminal, editor(3), editor(4), editor(5)];
    const recent = [5, 4, 3, 1];
    expect(planTabRetention(tabs, recent, 5)).toBeNull();
  });

  it("keeps 3 most recent + active + dirty editors warm", () => {
    const tabs = [
      terminal,
      editor(3),
      editor(4),
      editor(5),
      editor(6, { dirty: true }),
    ];
    const recent = [6, 5, 4, 3, 1];
    // 6/5/4 are the budget, 3 is active, 6 is dirty → nothing becomes cold.
    expect(planTabRetention(tabs, recent, 3)).toBeNull();
    expect(coldSet(tabs)).toEqual(new Set());
  });

  it("marks old clean previously-used editor/markdown/preview cold", () => {
    const tabs = [
      terminal,
      editor(3),
      editor(4),
      editor(5),
      markdown(7),
      preview(8),
    ];
    // Budget = 5,4,3. 7/8 were used earlier but fell out of the budget.
    const recent = [5, 4, 3, 7, 8, 1];
    const next = planTabRetention(tabs, recent, 5);
    expect(next).not.toBeNull();
    expect(coldSet(next as Tab[])).toEqual(new Set([7, 8]));
  });

  it("never marks a terminal cold", () => {
    const tabs = [terminal, editor(3), editor(4), editor(5)];
    // Both the terminal (1) and editor 5 are out of budget and previously
    // used; only the editor may go cold.
    const recent = [4, 3, 1, 5];
    const next = planTabRetention(tabs, recent, 4);
    expect(next).not.toBeNull();
    expect(coldSet(next as Tab[])).toEqual(new Set([5]));
  });

  it("keeps a never-visited cold restored tab cold and warms it once active", () => {
    const restored = [terminal, editor(3, { cold: true }), editor(4, { cold: true })];

    // Never used, not active → no flag changes.
    expect(planTabRetention(restored, [], 1)).toBeNull();

    // Activating the restored tab warms it; the other stays cold.
    const warmed = planTabRetention(restored, [3], 3);
    expect(warmed).not.toBeNull();
    expect(coldSet(warmed as Tab[])).toEqual(new Set([4]));
  });

  it("does not mutate input", () => {
    const tabs = [terminal, editor(3, { cold: true }), editor(4)];
    const recent = [4, 3, 1];
    const before = JSON.stringify(tabs);
    const result = planTabRetention(tabs, recent, 4);
    expect(JSON.stringify(tabs)).toBe(before);
    expect(result).not.toBe(tabs);
    expect(MRU_TAB_BUDGET).toBe(3);
  });
});
