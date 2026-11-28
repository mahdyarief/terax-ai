export function isRemoteRuntime(): boolean {
  return Boolean(
    (globalThis as { __TERAX_REMOTE__?: boolean }).__TERAX_REMOTE__,
  );
}

export async function waitForRemoteSession(): Promise<void> {
  const ready = (globalThis as { __TERAX_REMOTE_READY__?: Promise<void> })
    .__TERAX_REMOTE_READY__;
  if (ready) {
    await ready;
  }
}
