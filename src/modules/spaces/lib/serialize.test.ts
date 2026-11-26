import { describe, expect, it } from "vitest";
import type { PaneNode } from "@/modules/terminal/lib/panes";
import type { Tab } from "@/modules/tabs/lib/useTabs";
import {
  hydrateTabs,
  isHandoffMovable,
  serializeTabs,
  serializeTabsForHandoff,
  stampHandoffAttachments,
  type SerializedTab,
} from "./serialize";

function counter(start = 100): () => number {
  let n = start;
  return () => n++;
}

function leafIdsOf(node: PaneNode): number[] {
  return node.kind === "leaf" ? [node.id] : node.children.flatMap(leafIdsOf);
}

function term(over: Partial<Extract<Tab, { kind: "terminal" }>>): Tab {
  return {
    id: 1,
    kind: "terminal",
    spaceId: "s1",
    title: "shell",
    paneTree: { kind: "leaf", id: 2, cwd: "/a" },
    activeLeafId: 2,
    ...over,
  } as Tab;
}

describe("serializeTabs", () => {
  it("drops private terminals and transient kinds", () => {
    const tabs: Tab[] = [
      term({ id: 1 }),
      term({ id: 3, private: true }),
      {
        id: 5,
        kind: "git-diff",
        spaceId: "s1",
        title: "d",
        path: "/a/x",
        repoRoot: "/a",
        mode: "+",
        originalPath: null,
        preview: true,
      },
      {
        id: 7,
        kind: "editor",
        spaceId: "s1",
        title: "x",
        path: "/a/x.ts",
        dirty: false,
        preview: false,
      },
    ];
    const out = serializeTabs(tabs);
    expect(out.map((t) => t.kind)).toEqual(["terminal", "editor"]);
  });

  it("marks the active leaf in a split tree", () => {
    const tree: PaneNode = {
      kind: "split",
      id: 10,
      dir: "row",
      children: [
        { kind: "leaf", id: 11, cwd: "/a" },
        { kind: "leaf", id: 12, cwd: "/b" },
      ],
    };
    const [s] = serializeTabs([term({ paneTree: tree, activeLeafId: 12 })]);
    const node = s as Extract<SerializedTab, { kind: "terminal" }>;
    expect(node.tree.kind).toBe("split");
    if (node.tree.kind === "split") {
      expect(node.tree.children[1]).toMatchObject({ cwd: "/b", active: true });
      expect(node.tree.children[0]).not.toHaveProperty("active");
    }
  });
});

describe("hydrateTabs", () => {
  it("keeps the shell a tab was opened with, and nothing for the default", () => {
    const shell = String.raw`C:\Program Files\Git\bin\bash.exe`;
    const [own, plain] = hydrateTabs(
      serializeTabs([term({ shell }), term({})]),
      "s2",
      counter(),
    );
    expect(own.kind === "terminal" && own.shell).toBe(shell);
    expect(plain.kind === "terminal" && plain.shell).toBeUndefined();
    expect(serializeTabs([term({})])[0]).not.toHaveProperty("shell");
  });

  it("round-trips structure, cwd, blocks and active leaf", () => {
    const tree: PaneNode = {
      kind: "split",
      id: 10,
      dir: "col",
      children: [
        { kind: "leaf", id: 11, cwd: "/a" },
        { kind: "leaf", id: 12, cwd: "/b" },
      ],
    };
    const tabs: Tab[] = [
      term({
        paneTree: tree,
        activeLeafId: 12,
        blocks: true,
        customTitle: "x",
      }),
    ];
    const serialized = serializeTabs(tabs);
    const [restored] = hydrateTabs(serialized, "s2", counter());
    expect(restored.kind).toBe("terminal");
    if (restored.kind !== "terminal") return;

    expect(restored.spaceId).toBe("s2");
    expect(restored.cold).toBe(true);
    expect(restored.blocks).toBe(true);
    expect(restored.customTitle).toBe("x");
    expect(restored.paneTree.kind).toBe("split");

    const leaves = leafIdsOf(restored.paneTree);
    expect(new Set(leaves).size).toBe(2);
    expect(leaves).toContain(restored.activeLeafId);
    // active leaf was the second one, which carried /b
    expect(restored.cwd).toBe("/b");
  });

  it("keeps the sizes of a resized split, and drops ones that do not fit", () => {
    const tree = (sizes: unknown): PaneNode =>
      ({
        kind: "split",
        id: 10,
        dir: "row",
        children: [
          { kind: "leaf", id: 11 },
          { kind: "leaf", id: 12 },
        ],
        sizes,
      }) as PaneNode;
    const restore = (sizes: unknown) => {
      const [tab] = hydrateTabs(
        serializeTabs([term({ paneTree: tree(sizes), activeLeafId: 11 })]),
        "s",
        counter(),
      );
      return tab.kind === "terminal" && tab.paneTree.kind === "split"
        ? tab.paneTree.sizes
        : "not a split";
    };
    expect(restore([70, 30])).toEqual([70, 30]);
    expect(restore(undefined)).toBeUndefined();
    expect(restore([70, 20, 10])).toBeUndefined();

    // A hand-edited or older file with bad sizes still restores the panes.
    const saved = serializeTabs([term({ paneTree: tree(undefined) })]);
    const node = (saved[0] as { tree: { sizes?: unknown } }).tree;
    node.sizes = ["x", 1];
    const [tab] = hydrateTabs(saved, "s", counter());
    expect(tab.kind === "terminal" && tab.paneTree.kind).toBe("split");
    expect(
      tab.kind === "terminal" &&
        tab.paneTree.kind === "split" &&
        tab.paneTree.sizes,
    ).toBeUndefined();
  });

  it("allocates fresh, unique, monotonic ids across all tabs and leaves", () => {
    const tree: PaneNode = {
      kind: "split",
      id: 10,
      dir: "row",
      children: [
        { kind: "leaf", id: 11, cwd: "/a" },
        { kind: "leaf", id: 12, cwd: "/b" },
      ],
    };
    const serialized = serializeTabs([
      term({ id: 1, paneTree: tree, activeLeafId: 11 }),
      term({ id: 2 }),
    ]);
    const restored = hydrateTabs(serialized, "s1", counter(100));

    const ids: number[] = [];
    for (const t of restored) {
      ids.push(t.id);
      if (t.kind === "terminal") ids.push(...leafIdsOf(t.paneTree));
    }
    expect(new Set(ids).size).toBe(ids.length);
    expect(Math.min(...ids)).toBeGreaterThanOrEqual(100);
  });

  it("returns empty for corrupted input without throwing", () => {
    expect(hydrateTabs([] as SerializedTab[], "s1", counter())).toEqual([]);
    expect(
      hydrateTabs(null as unknown as SerializedTab[], "s1", counter()),
    ).toEqual([]);
  });

  it("never writes a live leaf's stale PTY attachment", () => {
    const tab = term({
      paneTree: {
        kind: "leaf",
        id: 2,
        cwd: "/a",
        ptyAttachment: { ptyId: 41, checkpoint: "128", cols: 80, rows: 24 },
      },
    });
    const [serialized] = serializeTabs([tab]);
    expect(JSON.stringify(serialized)).not.toContain("ptyAttachment");
    const [hydrated] = hydrateTabs([serialized], "s1", counter(), {
      keepPty: true,
    });
    expect(hydrated.kind === "terminal" && hydrated.cold).toBe(true);
  });

  it("drops PTY attachments when hydrating without keepPty", () => {
    const stamped = stampHandoffAttachments(serializeTabs([term({})]), [
      { ptyId: 41, checkpoint: "128", cols: 80, rows: 24, attachToken: "5" },
    ]);
    const [hydrated] = hydrateTabs(stamped, "s1", counter());
    const leaf = hydrated.kind === "terminal" ? hydrated.paneTree : null;
    expect((leaf as { ptyAttachment?: unknown }).ptyAttachment).toBeUndefined();
  });

  it("clears a stale attachment from a leaf whose PTY id is null", () => {
    const serialized: SerializedTab[] = [
      {
        kind: "terminal",
        tree: {
          kind: "leaf",
          cwd: "/a",
          ptyAttachment: { ptyId: 9, checkpoint: "1", cols: 80, rows: 24 },
        },
      },
    ];
    const stamped = stampHandoffAttachments(serialized, [], [null]);
    expect(JSON.stringify(stamped)).not.toContain("ptyAttachment");
  });

  it("carries git views and clean editors in a handoff, not dirty editors", () => {
    const tabs: Tab[] = [
      {
        id: 1,
        kind: "git-diff",
        spaceId: "s1",
        title: "a.ts",
        path: "a.ts",
        repoRoot: "/r",
        mode: "+",
        originalPath: null,
        preview: false,
      },
      { id: 2, kind: "git-history", spaceId: "s1", title: "History", repoRoot: "/r" },
      {
        id: 3,
        kind: "git-commit-file",
        spaceId: "s1",
        title: "b.ts",
        repoRoot: "/r",
        sha: "abc123",
        shortSha: "abc",
        subject: "fix",
        path: "b.ts",
        originalPath: null,
      },
      { id: 4, kind: "editor", spaceId: "s1", title: "c", path: "/c", dirty: true, preview: false },
      { id: 5, kind: "editor", spaceId: "s1", title: "d", path: "/d", dirty: false, preview: false },
    ];
    expect(tabs.map(isHandoffMovable)).toEqual([true, true, true, false, true]);
    // Persistence is unchanged: git views are not written to disk.
    expect(serializeTabs(tabs).map((t) => t.kind)).toEqual(["editor", "editor"]);
    const moved = serializeTabsForHandoff(tabs);
    expect(moved.map((t) => t.kind)).toEqual([
      "git-diff",
      "git-history",
      "git-commit-file",
      "editor",
    ]);
    const hydrated = hydrateTabs(moved, "s2", counter());
    expect(hydrated[0]).toMatchObject({ kind: "git-diff", spaceId: "s2", repoRoot: "/r", mode: "+" });
    expect(hydrated[2]).toMatchObject({ kind: "git-commit-file", sha: "abc123", subject: "fix" });
  });

  it("keeps legacy state cold when no PTY attachment exists", () => {
    const [serialized] = serializeTabs([term({})]);
    const [hydrated] = hydrateTabs([serialized], "s1", counter());
    expect(hydrated.kind === "terminal" && hydrated.cold).toBe(true);
  });

  it("stamps prepared PTY checkpoints onto terminal leaves for handoff", () => {
    const serialized = serializeTabs([term({})]);
    const stamped = stampHandoffAttachments(serialized, [
      { ptyId: 41, checkpoint: "128", cols: 80, rows: 24, attachToken: "5" },
    ]);
    const [hydrated] = hydrateTabs(stamped, "s1", counter(), {
      keepPty: true,
    });
    const leaf = hydrated.kind === "terminal" ? hydrated.paneTree : null;
    expect(leaf).toMatchObject({
      kind: "leaf",
      ptyAttachment: { ptyId: 41, checkpoint: "128", cols: 80, rows: 24 },
    });
    expect(hydrated.kind === "terminal" && hydrated.cold).toBe(false);
  });

  it("keeps cold leaves unattached when leaf PTY ids are known", () => {
    const serialized = serializeTabs([term({})]);
    const stamped = stampHandoffAttachments(
      serialized,
      [
        { ptyId: 41, checkpoint: "128", cols: 80, rows: 24, attachToken: "5" },
      ],
      [null],
    );
    const [hydrated] = hydrateTabs(stamped, "s1", counter(), {
      keepPty: true,
    });
    const leaf = hydrated.kind === "terminal" ? hydrated.paneTree : null;
    expect(leaf).toMatchObject({ kind: "leaf" });
    expect((leaf as { ptyAttachment?: unknown }).ptyAttachment).toBeUndefined();
  });

  it("hydrates editor/preview/markdown as cold with derived titles", () => {
    const serialized: SerializedTab[] = [
      { kind: "editor", path: "/a/foo.ts" },
      { kind: "preview", url: "http://localhost:5173/x" },
      { kind: "markdown", path: "/a/README.md" },
    ];
    const out = hydrateTabs(serialized, "s1", counter());
    expect(out.every((t) => t.cold === true)).toBe(true);
    expect(out.map((t) => t.title)).toEqual([
      "foo.ts",
      "localhost:5173",
      "README.md",
    ]);
  });
});
