import { afterEach, describe, expect, it, vi } from "vitest";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...a: unknown[]) => invoke(...a) }));

import { pickWorkspaceFolder } from "./env";
import { useBrowserFolderPicker } from "./folderPickerStore";

afterEach(() => {
  delete (globalThis as { __TERAX_REMOTE__?: boolean }).__TERAX_REMOTE__;
  invoke.mockReset();
  useBrowserFolderPicker.setState({ request: null });
});

describe("pickWorkspaceFolder", () => {
  it("opens the host's native dialog on the desktop", async () => {
    invoke.mockResolvedValue("/home/dy/code");
    await expect(pickWorkspaceFolder("/home/dy")).resolves.toBe("/home/dy/code");
    expect(invoke).toHaveBeenCalledWith("workspace_pick_folder", {
      defaultPath: "/home/dy",
    });
  });

  it("picks in the app in a browser session, never the native dialog", async () => {
    (globalThis as { __TERAX_REMOTE__?: boolean }).__TERAX_REMOTE__ = true;
    const picked = pickWorkspaceFolder("/home/dy");
    expect(useBrowserFolderPicker.getState().request?.defaultPath).toBe("/home/dy");
    useBrowserFolderPicker.getState().finish("/home/dy/code");
    await expect(picked).resolves.toBe("/home/dy/code");
    expect(invoke).not.toHaveBeenCalled();
  });

  it("resolves null when the browser dialog is cancelled", async () => {
    (globalThis as { __TERAX_REMOTE__?: boolean }).__TERAX_REMOTE__ = true;
    const picked = pickWorkspaceFolder(null);
    useBrowserFolderPicker.getState().finish(null);
    await expect(picked).resolves.toBeNull();
  });
});
