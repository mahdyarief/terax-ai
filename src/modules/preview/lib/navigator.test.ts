import { afterEach, describe, expect, it, vi } from "vitest";
import {
  getPreviewNavigator,
  openUrlInPreview,
  setPreviewNavigator,
} from "./navigator";

afterEach(() => setPreviewNavigator(null));

describe("preview navigator", () => {
  it("reports no host before one is registered", () => {
    expect(getPreviewNavigator()).toBeNull();
    expect(openUrlInPreview("https://example.com")).toBe(false);
  });

  it("routes the url to the registered host", () => {
    const openUrl = vi.fn(() => true);
    setPreviewNavigator({ openUrl });

    expect(openUrlInPreview("https://example.com/a?b=1")).toBe(true);
    expect(openUrl).toHaveBeenCalledWith("https://example.com/a?b=1");
  });

  it("falls back when the host declines the url", () => {
    setPreviewNavigator({ openUrl: () => false });
    expect(openUrlInPreview("javascript:alert(1)")).toBe(false);
  });

  it("falls back when the host throws", () => {
    setPreviewNavigator({
      openUrl: () => {
        throw new Error("host unavailable");
      },
    });
    expect(openUrlInPreview("https://example.com")).toBe(false);
  });

  it("clears the host on teardown", () => {
    setPreviewNavigator({ openUrl: () => true });
    setPreviewNavigator(null);
    expect(openUrlInPreview("https://example.com")).toBe(false);
  });
});
