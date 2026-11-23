import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { expect, it, vi } from "vitest";

vi.mock("@/lib/platform", () => ({ IS_WINDOWS: true }));
let env: Record<string, unknown> = { kind: "local" };
vi.mock("@/modules/workspace", () => ({
  LOCAL_WORKSPACE: { kind: "local" },
  workspaceEnvLabel: (e: { kind: string }, local: string) =>
    e.kind === "local" ? local : e.kind,
  useWorkspaceEnvStore: (select: (state: unknown) => unknown) =>
    select({
      env,
      distros: [],
      loading: false,
      error: null,
      refreshDistros: vi.fn(),
    }),
}));

const { WorkspaceEnvSelector } = await import("./WorkspaceEnvSelector");

const render = () =>
  renderToStaticMarkup(createElement(WorkspaceEnvSelector, { onSelect: vi.fn() }));

it("offers this machine's environments in a local or WSL space", () => {
  env = { kind: "local" };
  expect(render()).toContain("Windows");
  env = { kind: "wsl", distro: "Ubuntu" };
  expect(render()).toContain("wsl");
});

it("is not offered in a server's space, which keeps its host", () => {
  env = { kind: "ssh", host: "dypod" };
  expect(render()).toBe("");
});
