import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { FullAppLink } from "./RemoteApp";

describe("FullAppLink", () => {
  it("links to the full app only for a device with app access", () => {
    const granted = renderToStaticMarkup(
      createElement(FullAppLink, { grant: { input: true, tabs: true, app: true } }),
    );
    expect(granted).toContain('href="/app/"');
    expect(granted).toContain("Open full app");
    for (const grant of [
      { input: true, tabs: true, app: false },
      { input: true, tabs: true },
    ]) {
      expect(renderToStaticMarkup(createElement(FullAppLink, { grant }))).toBe("");
    }
  });
});
