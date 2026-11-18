const RELOAD_FLAG = "terax:tauri-bridge-reload";

type BridgeEnv = {
  hasMetadata: () => boolean;
  storage: Pick<Storage, "getItem" | "setItem" | "removeItem"> | null;
  reload: () => void;
  sleep: (ms: number) => Promise<void>;
  now: () => number;
};

function browserEnv(): BridgeEnv {
  let storage: BridgeEnv["storage"] = null;
  try {
    storage = window.sessionStorage;
  } catch {
    storage = null;
  }
  return {
    hasMetadata: () =>
      Boolean(
        (
          window as unknown as {
            __TAURI_INTERNALS__?: { metadata?: { currentWindow?: unknown } };
          }
        ).__TAURI_INTERNALS__?.metadata?.currentWindow,
      ),
    storage,
    reload: () => window.location.reload(),
    sleep: (ms) => new Promise((resolve) => setTimeout(resolve, ms)),
    now: () => Date.now(),
  };
}

/**
 * Resolve once Tauri's per-window metadata is present.
 *
 * On Windows, WebView2 registers a new window's initialization script
 * asynchronously, so the first document of a freshly built window (a space
 * window) can run before the script that defines
 * `__TAURI_INTERNALS__.metadata`. `getCurrentWindow()` then throws, the window
 * never boots, and the handoff that opened it times out. The script is
 * registered by the time we notice, so one reload picks it up. A session flag
 * keeps this to a single reload; it is cleared once the bridge is seen.
 *
 * Returns false only when the bridge is still missing after that one reload.
 */
export async function ensureTauriBridge(
  env: BridgeEnv = browserEnv(),
  waitMs = 1500,
): Promise<boolean> {
  const deadline = env.now() + waitMs;
  while (!env.hasMetadata() && env.now() < deadline) {
    await env.sleep(25);
  }
  if (env.hasMetadata()) {
    env.storage?.removeItem(RELOAD_FLAG);
    return true;
  }
  if (env.storage && env.storage.getItem(RELOAD_FLAG) !== "1") {
    env.storage.setItem(RELOAD_FLAG, "1");
    env.reload();
    // Never resolve: the page is going away.
    await new Promise<never>(() => {});
  }
  return false;
}
