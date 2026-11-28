import {
  createTauriInternals,
  decodeRelayPayload,
  type RemoteSessionState,
} from "./internals";

const SESSION_STORAGE_KEY = "terax:remote-session";
const BANNER_ID = "terax-remote-reconnect-banner";
const SUPERSEDED_ID = "terax-remote-superseded";

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

function setReconnectBanner(visible: boolean): void {
  if (typeof document === "undefined") return;
  const existing = document.getElementById(BANNER_ID);
  if (!visible) {
    existing?.remove();
    return;
  }
  if (existing || !document.body) return;
  const banner = document.createElement("div");
  banner.id = BANNER_ID;
  banner.textContent = "Reconnecting to Terax...";
  Object.assign(banner.style, {
    position: "fixed",
    top: "8px",
    left: "50%",
    transform: "translateX(-50%)",
    zIndex: "99999",
    padding: "6px 12px",
    borderRadius: "6px",
    background: "#b45309",
    color: "#fff",
    fontSize: "12px",
    fontWeight: "600",
    fontFamily: "system-ui, sans-serif",
    boxShadow: "0 4px 12px rgba(0, 0, 0, 0.3)",
  });
  document.body.appendChild(banner);
}

/** Another tab of this device opened the app and took over its session. */
function showSupersededNotice(): void {
  if (typeof document === "undefined" || !document.body) return;
  setReconnectBanner(false);
  if (document.getElementById(SUPERSEDED_ID)) return;
  const overlay = document.createElement("div");
  overlay.id = SUPERSEDED_ID;
  Object.assign(overlay.style, {
    position: "fixed",
    inset: "0",
    zIndex: "100000",
    display: "flex",
    flexDirection: "column",
    alignItems: "center",
    justifyContent: "center",
    gap: "12px",
    background: "rgba(0, 0, 0, 0.75)",
    color: "#fff",
    fontFamily: "system-ui, sans-serif",
    fontSize: "14px",
  });
  const message = document.createElement("p");
  message.textContent = "Terax was opened in another tab on this device.";
  const button = document.createElement("button");
  button.type = "button";
  button.textContent = "Use it here";
  Object.assign(button.style, {
    padding: "6px 14px",
    borderRadius: "6px",
    border: "0",
    background: "#fff",
    color: "#111",
    fontWeight: "600",
    cursor: "pointer",
  });
  button.addEventListener("click", () => window.location.reload());
  overlay.append(message, button);
  document.body.appendChild(overlay);
}

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
      setReconnectBanner(false);
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
        tauriInternals.runCallback(msg.id, decodeRelayPayload(msg.payload));
      } catch (err) {
        console.error("remote-runtime: invalid callback payload", err);
      }
    });
    source.addEventListener("reset", () => {
      window.location.reload();
    });
    source.addEventListener("superseded", () => {
      // Stop here: reconnecting would open a session and take it back.
      // "Use it here" reloads without the old id, which takes over again.
      source.close();
      sessionStorage.removeItem(SESSION_STORAGE_KEY);
      showSupersededNotice();
      if (!opened) {
        opened = true;
        resolve();
      }
    });
    source.onerror = () => {
      if (opened) {
        setReconnectBanner(true);
      } else {
        opened = true;
        resolve();
      }
    };
  });
}

(
  globalThis as { __TERAX_REMOTE_READY__?: Promise<void> }
).__TERAX_REMOTE_READY__ = bootstrap().catch((err) => {
  console.error("remote-runtime bootstrap failed", err);
});
