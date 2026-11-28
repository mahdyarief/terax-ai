import { afterEach, describe, expect, it, vi } from "vitest";

const invokeMock = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

import { getAllKeys, getKey } from "./keyring";

describe("keyring in remote runtime", () => {
  afterEach(() => {
    delete (globalThis as { __TERAX_REMOTE__?: boolean }).__TERAX_REMOTE__;
    invokeMock.mockReset();
  });

  it("never calls secrets_get or secrets_get_all and returns placeholders only", async () => {
    (globalThis as { __TERAX_REMOTE__?: boolean }).__TERAX_REMOTE__ = true;
    invokeMock.mockImplementation(async (cmd: string, args: { accounts: string[] }) => {
      expect(cmd).toBe("secrets_has");
      return args.accounts.map((a) => a.includes("openai"));
    });

    const single = await getKey("openai");
    expect(single).toBe("__TERAX_REMOTE_KEY__:openai-api-key");

    const all = await getAllKeys();
    expect(all.openai).toBe("__TERAX_REMOTE_KEY__:openai-api-key");
    expect(all.anthropic).toBeNull();

    const cmds = invokeMock.mock.calls.map((c) => c[0]);
    expect(cmds).not.toContain("secrets_get");
    expect(cmds).not.toContain("secrets_get_all");
  });
});
