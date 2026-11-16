import {
  SPACE_TABS_ATTACHED_EVENT,
  type TabsAttachedPayload,
} from "./crossWindowSync";

type Listen = (
  event: string,
  fn: (event: { payload: TabsAttachedPayload }) => void,
) => Promise<() => void>;

/**
 * Wait for the target window to ACK a handoff. Each call registers its own
 * listener and filters by `handoffId`, so concurrent moves cannot cross ACKs.
 */
export function createAttachWaiter({
  listen,
  timeoutMs = 10000,
}: {
  listen: Listen;
  timeoutMs?: number;
}): { waitForAttach: (handoffId: string) => Promise<TabsAttachedPayload> } {
  return {
    async waitForAttach(handoffId) {
      let settled = false;
      let unlisten: (() => void) | null = null;
      let resolveAck!: (payload: TabsAttachedPayload) => void;
      let rejectAck!: (error: Error) => void;
      const ack = new Promise<TabsAttachedPayload>((resolve, reject) => {
        resolveAck = resolve;
        rejectAck = reject;
      });

      const stop = () => {
        if (unlisten) {
          const off = unlisten;
          unlisten = null;
          off();
        }
      };

      const timer = setTimeout(() => {
        if (settled) return;
        settled = true;
        stop();
        rejectAck(new Error("attach timeout"));
      }, timeoutMs);

      unlisten = await listen(SPACE_TABS_ATTACHED_EVENT, (event) => {
        if (settled) return;
        if (event.payload?.handoffId !== handoffId) return;
        settled = true;
        clearTimeout(timer);
        stop();
        resolveAck(event.payload);
      });
      // The timeout may have fired while `listen` was still registering.
      if (settled) stop();

      return ack;
    },
  };
}
