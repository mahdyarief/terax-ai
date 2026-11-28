import { describe, expect, it } from "vitest";
import {
  hostChildPath,
  hostParentPath,
  isHostRoot,
  useBrowserFolderPicker,
} from "./folderPickerStore";

describe("host paths", () => {
  it("walks POSIX paths", () => {
    expect(hostParentPath("/home/dy/code")).toBe("/home/dy");
    expect(hostParentPath("/home/")).toBe("/");
    expect(hostParentPath("/")).toBe("/");
    expect(hostChildPath("/home/dy", "code")).toBe("/home/dy/code");
    expect(hostChildPath("/", "home")).toBe("/home");
  });

  it("walks Windows paths", () => {
    expect(hostParentPath("C:\\Users\\dy")).toBe("C:\\Users");
    expect(hostParentPath("C:\\Users")).toBe("C:\\");
    expect(hostParentPath("C:\\")).toBe("C:\\");
    expect(hostChildPath("C:\\", "Users")).toBe("C:\\Users");
    expect(hostChildPath("C:\\Users", "dy")).toBe("C:\\Users\\dy");
  });

  it("knows the roots", () => {
    expect(isHostRoot("/")).toBe(true);
    expect(isHostRoot("C:\\")).toBe(true);
    expect(isHostRoot("/home")).toBe(false);
  });
});

describe("useBrowserFolderPicker", () => {
  it("resolves the pick, and a new pick cancels the older one", async () => {
    const store = useBrowserFolderPicker.getState();
    const first = store.open("/home");
    const second = useBrowserFolderPicker.getState().open("/tmp");
    await expect(first).resolves.toBeNull();
    expect(useBrowserFolderPicker.getState().request?.defaultPath).toBe("/tmp");
    useBrowserFolderPicker.getState().finish("/tmp/x");
    await expect(second).resolves.toBe("/tmp/x");
    expect(useBrowserFolderPicker.getState().request).toBeNull();
  });
});
