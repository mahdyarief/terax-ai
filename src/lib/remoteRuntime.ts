export function isRemoteRuntime(): boolean {
  return Boolean(
    (globalThis as { __TERAX_REMOTE__?: boolean }).__TERAX_REMOTE__,
  );
}

/** What the host said about this browser session, once it has started. */
export type RemoteSessionInfo = {
  label: string;
  headless: boolean;
  deviceName: string | null;
};

export function remoteSessionInfo(): RemoteSessionInfo | null {
  const session = (
    globalThis as {
      __TERAX_REMOTE_SESSION__?: {
        label?: string;
        headless?: boolean;
        deviceName?: string;
      };
    }
  ).__TERAX_REMOTE_SESSION__;
  if (!isRemoteRuntime() || !session?.label) return null;
  return {
    label: session.label,
    headless: Boolean(session.headless),
    deviceName: session.deviceName?.trim() || null,
  };
}

export async function waitForRemoteSession(): Promise<void> {
  const ready = (globalThis as { __TERAX_REMOTE_READY__?: Promise<void> })
    .__TERAX_REMOTE_READY__;
  if (ready) {
    await ready;
  }
}
