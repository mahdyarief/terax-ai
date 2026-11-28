import { createTauriInternals, type RemoteSessionState } from "./internals";

const SESSION_STORAGE_KEY = "terax:remote-session";

let currentSession: RemoteSessionState = { id: "", label: "remote-0" };
let resolveSession!: (s: RemoteSessionState) => void;
const sessionPromise = new Promise<RemoteSessionState>((resolve) => {
  resolveSession = resolve;
});

const { tauriInternals, eventPluginInternals } = createTauriInternals({
  getSession: () => currentSession,
  waitForSession: () => sessionPromise,
});

(globalThis as { __TERAX_REMOTE__?: boolean }).__TERAX_REMOTE__ = true;
(
  globalThis as { __TAURI_INTERNALS__?: typeof tauriInternals }
).__TAURI_INTERNALS__ = tauriInternals;
(
  globalThis as {
    __TAURI_EVENT_PLUGIN_INTERNALS__?: typeof eventPluginInternals;
  }
).__TAURI_EVENT_PLUGIN_INTERNALS__ = eventPluginInternals;

async function bootstrap(): Promise<void> {
  const savedId = sessionStorage.getItem(SESSION_STORAGE_KEY) ?? undefined;
  const res = await fetch("/api/app/session", {
    method: "POST",
    headers: {
      "X-Terax-Remote": "1",
      "Content-Type": "application/json",
    },
    body: JSON.stringify(savedId ? { session: savedId } : {}),
  });
  if (!res.ok) {
    throw new Error(`Failed to open remote session: ${res.status}`);
  }
  const session = (await res.json()) as RemoteSessionState;
  currentSession = session;
  sessionStorage.setItem(SESSION_STORAGE_KEY, session.id);
  tauriInternals.metadata.currentWindow.label = session.label;
  tauriInternals.metadata.currentWebview.label = session.label;
  resolveSession(session);

  await new Promise<void>((resolve) => {
    const source = new EventSource(
      `/api/app/callbacks?session=${encodeURIComponent(session.id)}`,
    );
    let opened = false;
    source.onopen = () => {
      if (!opened) {
        opened = true;
        resolve();
      }
    };
    source.addEventListener("callback", (ev) => {
      try {
        const msg = JSON.parse((ev as MessageEvent<string>).data) as {
          id: number;
          payload: unknown;
        };
        tauriInternals.runCallback(msg.id, msg.payload);
      } catch (err) {
        console.error("remote-runtime: invalid callback payload", err);
      }
    });
    source.addEventListener("reset", () => {
      window.location.reload();
    });
    source.onerror = () => {
      if (!opened) {
        opened = true;
        resolve();
      }
    };
  });
}

(globalThis as { __TERAX_REMOTE_READY__?: Promise<void> }).__TERAX_REMOTE_READY__ =
  bootstrap().catch((err) => {
    console.error("remote-runtime bootstrap failed", err);
  });
