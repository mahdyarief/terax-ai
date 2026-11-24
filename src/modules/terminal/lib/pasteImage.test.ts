import { describe, expect, it, vi } from "vitest";

vi.mock("sonner", () => ({ toast: { error: vi.fn() } }));

const { pasteClipboardImage } = await import("./pasteImage");
const { toast } = await import("sonner");

describe("pasting the clipboard's image", () => {
  it("pastes the saved path, quoted for the shell", async () => {
    const pasted = await pasteClipboardImage(
      { kind: "ssh", host: "dypod" },
      () => "posix",
      async () => "/home/me/.cache/terax-image/my shot.png",
    );
    expect(pasted).toBe("'/home/me/.cache/terax-image/my shot.png' ");
  });

  it("is nothing when the clipboard holds no image", async () => {
    expect(
      await pasteClipboardImage(
        { kind: "local" },
        () => "powershell",
        async () => null,
      ),
    ).toBeNull();
  });

  it("says why it could not save the image", async () => {
    const pasted = await pasteClipboardImage(
      { kind: "local" },
      () => "powershell",
      async () => {
        throw "The image is 25 MB; images up to 20 MB can be pasted.";
      },
    );
    expect(pasted).toBeNull();
    expect(toast.error).toHaveBeenCalledWith("Couldn't paste the image", {
      description: "The image is 25 MB; images up to 20 MB can be pasted.",
    });
  });

  it("still pastes a usable path for a shell without a dialect", async () => {
    const pasted = await pasteClipboardImage(
      { kind: "wsl", distro: "Ubuntu" },
      () => {
        throw new Error("unknown shell");
      },
      async () => "/mnt/c/Temp/terax-image/a.png",
    );
    expect(pasted).toBe("/mnt/c/Temp/terax-image/a.png ");
  });
});
