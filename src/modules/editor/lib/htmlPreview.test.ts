import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createHtmlPreviewPublisher } from "./htmlPreview";

function setup(delayMs = 250) {
  const publish = vi.fn(async (html: string) => `token:${html}`);
  const drop = vi.fn(async () => {});
  const onReady = vi.fn();
  const onError = vi.fn();
  const publisher = createHtmlPreviewPublisher({
    publish,
    drop,
    onReady,
    onError,
    delayMs,
  });
  return { publisher, publish, drop, onReady, onError };
}

describe("createHtmlPreviewPublisher", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("coalesces rapid edits into a single publish", async () => {
    const { publisher, publish, onReady } = setup();
    publisher.request("<p>a");
    publisher.request("<p>ab");
    publisher.request("<p>abc");
    await vi.advanceTimersByTimeAsync(249);
    expect(publish).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(1);
    expect(publish).toHaveBeenCalledTimes(1);
    expect(publish).toHaveBeenCalledWith("<p>abc");
    expect(onReady).toHaveBeenCalledWith("token:<p>abc");
  });

  it("drops the superseded token when a newer publish ships", async () => {
    const { publisher, drop, onReady } = setup();
    publisher.request("first");
    await vi.advanceTimersByTimeAsync(250);
    publisher.request("second");
    await vi.advanceTimersByTimeAsync(250);
    expect(onReady).toHaveBeenLastCalledWith("token:second");
    expect(drop).toHaveBeenCalledWith("token:first");
  });

  it("drops a token that resolves after disposal", async () => {
    const { publisher, drop, onReady } = setup();
    publisher.request("stale");
    await vi.advanceTimersByTimeAsync(250);
    expect(onReady).toHaveBeenCalledTimes(1);
    publisher.dispose();
    expect(drop).toHaveBeenCalledWith("token:stale");
  });

  it("reports publish failures without a ready token", async () => {
    const { publisher, onReady, onError, publish } = setup();
    publish.mockRejectedValueOnce(new Error("frame limit exceeded"));
    publisher.request("big");
    await vi.advanceTimersByTimeAsync(250);
    expect(onReady).not.toHaveBeenCalled();
    expect(onError).toHaveBeenCalledWith("frame limit exceeded");
  });

  it("ignores requests after disposal", async () => {
    const { publisher, publish } = setup();
    publisher.dispose();
    publisher.request("late");
    await vi.advanceTimersByTimeAsync(250);
    expect(publish).not.toHaveBeenCalled();
  });
});