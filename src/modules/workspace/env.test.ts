import { describe, expect, it, vi } from "vitest";

vi.mock("@/modules/settings/store", () => ({ setLastWslDistro: vi.fn() }));

import {
  LOCAL_WORKSPACE,
  parseWorkspaceScopeKey,
  sameWorkspaceEnv,
  workspaceEnvLabel,
  workspaceScopeKey,
  type WorkspaceEnv,
} from "./env";

const ssh: WorkspaceEnv = { kind: "ssh", host: "dypod" };
const wsl: WorkspaceEnv = { kind: "wsl", distro: "Ubuntu" };

describe("workspace scope keys", () => {
  it("give each SSH host its own scope, apart from local and WSL", () => {
    expect(workspaceScopeKey(ssh)).toBe("ssh:dypod");
    expect(workspaceScopeKey(wsl)).toBe("wsl:Ubuntu");
    expect(workspaceScopeKey(LOCAL_WORKSPACE)).toBe("local");
  });

  it("round-trip every kind", () => {
    for (const env of [ssh, wsl, LOCAL_WORKSPACE, { kind: "ssh", host: "me@box:2222" } as const]) {
      expect(parseWorkspaceScopeKey(workspaceScopeKey(env))).toEqual(env);
    }
  });

  it("tell hosts apart", () => {
    expect(sameWorkspaceEnv(ssh, { kind: "ssh", host: "dypod" })).toBe(true);
    expect(sameWorkspaceEnv(ssh, { kind: "ssh", host: "other" })).toBe(false);
    expect(sameWorkspaceEnv(ssh, LOCAL_WORKSPACE)).toBe(false);
    expect(sameWorkspaceEnv(wsl, { kind: "wsl", distro: "Debian" })).toBe(false);
  });

  it("label a remote workspace by its host, never as this machine", () => {
    expect(workspaceEnvLabel(ssh, "Windows")).toBe("SSH: dypod");
    expect(workspaceEnvLabel(LOCAL_WORKSPACE, "Windows")).toBe("Windows");
  });
});
