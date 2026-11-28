interface TauriInternals {
  callbacks?: {
    get: (id: number) => ((data: unknown) => void) | undefined;
  };
  invoke: (cmd: string, args: Record<string, unknown>) => Promise<unknown>;
}

(() => {
  const tauri = (window as unknown as { __TAURI_INTERNALS__?: TauriInternals })
    .__TAURI_INTERNALS__;
  if (!tauri || !tauri.callbacks) {
    console.error("remote-relay: __TAURI_INTERNALS__.callbacks missing");
    return;
  }
  const callbacks = tauri.callbacks;
  const origGet = callbacks.get.bind(callbacks);

  function sanitize(val: unknown): unknown {
    if (val instanceof ArrayBuffer) {
      return Array.from(new Uint8Array(val));
    }
    if (
      val &&
      typeof val === "object" &&
      "message" in val &&
      (val as { message: unknown }).message instanceof ArrayBuffer
    ) {
      const obj = val as { message: ArrayBuffer };
      return { ...obj, message: Array.from(new Uint8Array(obj.message)) };
    }
    return val;
  }

  callbacks.get = (id: number) => {
    const local = origGet(id);
    if (local) return local;
    return (data: unknown) => {
      tauri
        .invoke("remote_relay_push", {
          id,
          payload: sanitize(data),
        })
        .catch((err: unknown) => console.error("remote_relay_push failed", err));
    };
  };
})();
