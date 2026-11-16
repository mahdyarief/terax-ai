import { describe, expect, it, vi } from "vitest";
import { createAttachWaiter } from "./attachWaiter";
import type { TabsAttachedPayload } from "./crossWindowSync";

type Listener = (event: { payload: TabsAttachedPayload }) => void;

function harness() {
  const listeners: Listener[] = [];
  const unlisten = vi.fn(() => {
    const i = listeners.indexOf(listener);
    if (i >= 0) listeners.splice(i, 1);
  });
  let listener: Listener = () => {};
  const listen = vi.fn(async (_event: string, fn: Listener) => {
    listener = fn;
    listeners.push(fn);
    return unlisten;
  });
  return {
    listen,
    unlisten,
    emit: (payload: TabsAttachedPayload) => listener({ payload }),
  };
}

function ack(handoffId: string): TabsAttachedPayload {
  return { handoffId, replays: [] };
}

describe("createAttachWaiter", () => {
  it("ignores ACKs for other handoffs", async () => {
    const h = harness();
    const waiter = createAttachWaiter({ listen: h.listen, timeoutMs: 30 });

    const pending = waiter.waitForAttach("h1");
    // Registration involves several awaits inside the waiter.
    await new Promise((resolve) => setTimeout(resolve, 0));
    h.emit(ack("other"));

    await expect(pending).rejects.toThrow("attach timeout");
    expect(h.unlisten).toHaveBeenCalledTimes(1);
  });

  it("resolves with the matching ACK and unlistens", async () => {
    const h = harness();
    const waiter = createAttachWaiter({ listen: h.listen });

    const pending = waiter.waitForAttach("h1");
    await new Promise((resolve) => setTimeout(resolve, 0));
    h.emit(ack("h1"));

    await expect(pending).resolves.toMatchObject({ handoffId: "h1" });
    expect(h.unlisten).toHaveBeenCalledTimes(1);
  });

  it("rejects on timeout and unlistens", async () => {
    const h = harness();
    const waiter = createAttachWaiter({ listen: h.listen, timeoutMs: 5 });

    await expect(waiter.waitForAttach("h1")).rejects.toThrow("attach timeout");
    expect(h.unlisten).toHaveBeenCalledTimes(1);
  });
});
