import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { expect, it, vi } from "vitest";

vi.mock("@/components/ui/popover", () => ({
  Popover: ({ children }: { children: React.ReactNode }) => children,
  PopoverTrigger: ({ children }: { children: React.ReactNode }) => children,
  PopoverContent: ({ children }: { children: React.ReactNode }) => children,
}));
vi.mock("@/modules/shortcuts", () => ({ useShortcutLabel: () => null }));
let currentWindowLabel = "main";
let currentHostWindowLabel = "main";

vi.mock("./lib/useSpaces", () => ({
  useSpaces: (select: (state: unknown) => unknown) =>
    select({
      spaces: [
        {
          id: "space-1",
          name: "Main",
          root: null,
          env: { kind: "local" },
          createdAt: 0,
          updatedAt: 0,
          hostWindowLabel: currentHostWindowLabel,
        },
      ],
      activeId: "space-1",
      setActive: vi.fn(),
      rename: vi.fn(),
      windowLabel: currentWindowLabel,
    }),
}));
vi.mock("@hugeicons/react", () => ({
  HugeiconsIcon: () => null,
}));

import { SpaceSwitcher } from "./SpaceSwitcher";

function renderSwitcher() {
  return renderToStaticMarkup(
    createElement(SpaceSwitcher, {
      open: true,
      onOpenChange: vi.fn(),
      tabs: [],
      onNewSpace: vi.fn(),
      onDeleteSpace: vi.fn(),
      onOpenSpaceInWindow: vi.fn(),
      onBringSpaceToWindow: vi.fn(),
      onNewTabInSpace: vi.fn(),
      onJumpTab: vi.fn(),
      onCloseTab: vi.fn(),
      onMoveTabToSpace: vi.fn(),
      onReorderTab: vi.fn(),
      onReorderSpaces: vi.fn(),
    }),
  );
}

it("renders Space actions without hover-only visibility classes", () => {
  const html = renderSwitcher();

  const noDragClasses = [
    ...html.matchAll(/<[^>]*data-no-drag[^>]*class="([^"]*)"/g),
    ...html.matchAll(/<[^>]*class="([^"]*)"[^>]*data-no-drag/g),
  ].map((m) => m[1]);

  expect(noDragClasses.length).toBeGreaterThan(0);
  for (const cls of noDragClasses) {
    expect(cls.split(/\s+/)).not.toContain("hidden");
  }

  expect(html).toContain('aria-label="Rename space"');
  expect(html).toContain('aria-label="Open in new window"');
  expect(html).not.toContain("group-hover:hidden");
});

it("hides window-opening actions in a dedicated space window", () => {
  currentWindowLabel = "space-1";
  currentHostWindowLabel = "space-1";

  const html = renderSwitcher();

  expect(html).not.toContain('aria-label="Open in new window"');
  expect(html).not.toContain('aria-label="Focus window"');
  expect(html).not.toContain('aria-label="Bring back to this window"');
  expect(html).toContain('aria-label="Rename space"');

  currentWindowLabel = "main";
  currentHostWindowLabel = "main";
});
