import type { TabSnapshot } from "@/web/tabs";
import { describe, expect, it } from "vitest";
import { otherWindowTabs } from "./DesktopTabsPanel";

describe("otherWindowTabs", () => {
  it("excludes the current window and lists tabs from all other windows", () => {
    const snap: TabSnapshot = {
      version: 1,
      tabs: [
        {
          window: "main",
          id: 1,
          title: "bash",
          kind: "terminal",
          panes: [{ leafId: 1, ptyId: 10, active: true }],
        },
        {
          window: "remote-0",
          id: 2,
          title: "browser-tab",
          kind: "terminal",
          panes: [{ leafId: 2, ptyId: 11, active: true }],
        },
        {
          window: "space-1",
          id: 3,
          title: "editor",
          kind: "editor",
          panes: [],
        },
      ],
    };
    const entries = otherWindowTabs(snap, "remote-0");
    expect(entries.map((e) => `${e.windowLabel}:${e.tab.id}`)).toEqual([
      "main:1",
      "space-1:3",
    ]);
  });
});
