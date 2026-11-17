import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { TabErrorBoundary, tabErrorMessage } from "./TabErrorBoundary";

describe("tabErrorMessage", () => {
  it("shows the underlying message for an Error", () => {
    expect(tabErrorMessage(new Error("bad docx"))).toBe("bad docx");
  });

  it("stringifies non-Error throws", () => {
    expect(tabErrorMessage("oops")).toBe("oops");
  });
});

describe("TabErrorBoundary", () => {
  it("produces error state from a thrown value", () => {
    expect(
      TabErrorBoundary.getDerivedStateFromError(new Error("bad docx")),
    ).toEqual({
      error: "bad docx",
    });
  });

  it("renders an in-tab fallback when error state is set", () => {
    const boundary = new TabErrorBoundary({ tabTitle: "report.docx" });
    boundary.state = { error: "bad docx" };
    const html = renderToStaticMarkup(boundary.render());

    expect(html).toContain("report.docx");
    expect(html).toContain("bad docx");
    expect(html).toContain("Try again");
  });

  it("renders children normally when there is no error", () => {
    const html = renderToStaticMarkup(
      createElement(
        TabErrorBoundary,
        { tabTitle: "ok.txt" },
        createElement("div", null, "tab-content"),
      ),
    );
    expect(html).toContain("tab-content");
  });
});
