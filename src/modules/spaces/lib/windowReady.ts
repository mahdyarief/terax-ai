const ready = new Set<string>();
const waiters = new Map<string, Array<() => void>>();

/** Mark a window as having mounted its cross-window listeners. */
export function noteWindowReady(label: string): void {
  if (!label) return;
  ready.add(label);
  const pending = waiters.get(label);
  if (!pending) return;
  waiters.delete(label);
  for (const resolve of pending) resolve();
}

export function isWindowReady(label: string): boolean {
  return ready.has(label);
}

/**
 * Resolve once `label` has announced readiness. New space windows are built
 * asynchronously by the backend, so a move can otherwise race the listener.
 */
export function waitForWindowReady(
  label: string,
  timeoutMs = 10000,
): Promise<void> {
  if (isWindowReady(label)) return Promise.resolve();
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      const pending = waiters.get(label);
      if (pending) {
        const index = pending.indexOf(done);
        if (index >= 0) pending.splice(index, 1);
        if (pending.length === 0) waiters.delete(label);
      }
      reject(new Error("window ready timeout"));
    }, timeoutMs);
    const done = () => {
      clearTimeout(timer);
      resolve();
    };
    const pending = waiters.get(label) ?? [];
    pending.push(done);
    waiters.set(label, pending);
  });
}

/** Test seam: forget every announcement and waiter. */
export function resetWindowReady(): void {
  ready.clear();
  waiters.clear();
}
