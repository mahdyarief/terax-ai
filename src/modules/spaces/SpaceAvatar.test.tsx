import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { SpaceAvatar } from "./SpaceAvatar";

const avatar = (env?: Parameters<typeof SpaceAvatar>[0]["space"]["env"]) =>
  renderToStaticMarkup(
    createElement(SpaceAvatar, { space: { name: "app", env } }),
  );

describe("a space's avatar", () => {
  it("marks a server's space", () =>
    expect(avatar({ kind: "ssh", host: "dypod" })).toContain(
      "data-space-server",
    ));

  it("leaves this machine's spaces as they were", () => {
    expect(avatar({ kind: "local" })).not.toContain("data-space-server");
    expect(avatar({ kind: "wsl", distro: "Ubuntu" })).not.toContain(
      "data-space-server",
    );
    expect(avatar()).not.toContain("relative");
  });
});
