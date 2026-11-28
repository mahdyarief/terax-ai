export interface RemoteSessionState {
  id: string;
  label: string;
  /** The host runs `--remote-serve`: nobody uses its own window. */
  headless?: boolean;
  /** The paired device's name, for the space a browser session creates. */
  deviceName?: string;
}

export interface TauriInternalsLike {
  invoke: (
    cmd: string,
    args?: unknown,
    options?: { headers?: Record<string, string> },
  ) => Promise<unknown>;
  transformCallback: (
    callback?: (data: unknown) => void,
    once?: boolean,
  ) => number;
  unregisterCallback: (id: number) => void;
  runCallback: (id: number, data: unknown) => void;
  callbacks: Map<number, (data: unknown) => void>;
  convertFileSrc: (filePath: string, protocol?: string) => string;
  metadata: {
    currentWindow: { label: string };
    currentWebview: { label: string };
  };
  plugins: {
    path: {
      sep: string;
      delimiter: string;
    };
  };
}

export interface TauriEventPluginInternalsLike {
  unregisterListener: (event: string, eventId: number) => void;
}

const LOCAL_NOOP_COMMANDS = new Set([
  "plugin:window|show",
  "plugin:window|hide",
  "plugin:window|set_focus",
  "plugin:window|unminimize",
  "plugin:window|minimize",
  "plugin:window|toggle_maximize",
  "plugin:window|maximize",
  "plugin:window|unmaximize",
  "plugin:window|start_dragging",
  "plugin:window|close",
  "plugin:window|destroy",
  "plugin:window|set_resizable",
  "plugin:window|set_always_on_top",
  "plugin:window|set_decorations",
  "plugin:window|set_shadow",
  "plugin:window|set_effects",
  "plugin:webview|webview_show",
  "plugin:webview|webview_hide",
  "plugin:webview|set_webview_focus",
]);

function isRawBody(args: unknown): args is Uint8Array | ArrayBuffer {
  return args instanceof Uint8Array || args instanceof ArrayBuffer;
}

export function decodeRelayPayload(payload: unknown): unknown {
  if (!payload || typeof payload !== "object") return payload;
  const obj = payload as Record<string, unknown>;
  if (obj.__rawPayload === true && Array.isArray(obj.data)) {
    return new Uint8Array(obj.data as number[]).buffer;
  }
  if (obj.__rawMessage === true && Array.isArray(obj.message)) {
    const { __rawMessage: _, ...rest } = obj;
    return {
      ...rest,
      message: new Uint8Array(obj.message as number[]).buffer,
    };
  }
  return payload;
}

export function createTauriInternals(options: {
  getSession: () => RemoteSessionState;
  waitForSession: () => Promise<RemoteSessionState>;
  fetchFn?: typeof fetch;
}): {
  tauriInternals: TauriInternalsLike;
  eventPluginInternals: TauriEventPluginInternalsLike;
} {
  const fetchImpl = options.fetchFn ?? globalThis.fetch.bind(globalThis);
  const callbacks = new Map<number, (data: unknown) => void>();
  let nextCallbackId = 1;

  function unregisterCallback(id: number): void {
    callbacks.delete(id);
  }

  function transformCallback(
    callback?: (data: unknown) => void,
    once = false,
  ): number {
    const id = nextCallbackId++;
    callbacks.set(id, (data: unknown) => {
      if (once) {
        unregisterCallback(id);
      }
      if (callback) {
        callback(data);
      }
    });
    return id;
  }

  function runCallback(id: number, data: unknown): void {
    const cb = callbacks.get(id);
    if (cb) {
      cb(data);
    }
  }

  const metadata = {
    currentWindow: { label: "remote-0" },
    currentWebview: { label: "remote-0" },
  };

  async function invoke(
    cmd: string,
    args?: unknown,
    invokeOptions?: { headers?: Record<string, string> },
  ): Promise<unknown> {
    if (cmd === "plugin:window|set_title") {
      const value =
        args && typeof args === "object" && "value" in args
          ? (args as { value?: unknown }).value
          : undefined;
      if (typeof value === "string" && typeof document !== "undefined") {
        document.title = value;
      }
      return null;
    }
    if (
      cmd === "plugin:window|is_maximized" ||
      cmd === "plugin:window|is_minimized" ||
      cmd === "plugin:window|is_fullscreen"
    ) {
      return false;
    }
    if (cmd === "plugin:window|is_focused") {
      return typeof document !== "undefined" ? document.hasFocus() : true;
    }
    if (cmd === "plugin:window|get_all_windows") {
      return [metadata.currentWindow.label];
    }
    if (cmd === "plugin:webview|get_all_webviews") {
      return [
        {
          label: metadata.currentWebview.label,
          windowLabel: metadata.currentWindow.label,
        },
      ];
    }
    if (LOCAL_NOOP_COMMANDS.has(cmd)) {
      return null;
    }
    if (cmd === "plugin:opener|open_url") {
      const url =
        args && typeof args === "object" && "url" in args
          ? (args as { url?: unknown }).url
          : undefined;
      if (typeof url === "string" && typeof window !== "undefined") {
        window.open(url, "_blank", "noopener,noreferrer");
      }
      return null;
    }

    const session = await options.waitForSession();
    const extraHeaders: Record<string, string> = {};
    if (invokeOptions?.headers) {
      extraHeaders["X-Terax-Invoke-Headers"] = JSON.stringify(
        invokeOptions.headers,
      );
    }
    let response: Response;
    if (isRawBody(args)) {
      const body =
        args instanceof Uint8Array
          ? (args.buffer.slice(
              args.byteOffset,
              args.byteOffset + args.byteLength,
            ) as ArrayBuffer)
          : args;
      response = await fetchImpl("/api/app/invoke", {
        method: "POST",
        headers: {
          "X-Terax-Remote": "1",
          "X-Terax-Session": session.id,
          "X-Terax-Cmd": cmd,
          "Content-Type": "application/octet-stream",
          ...extraHeaders,
        },
        body,
      });
    } else {
      response = await fetchImpl("/api/app/invoke", {
        method: "POST",
        headers: {
          "X-Terax-Remote": "1",
          "Content-Type": "application/json",
          ...extraHeaders,
        },
        body: JSON.stringify({
          session: session.id,
          cmd,
          args: args ?? {},
        }),
      });
    }

    const contentType = response.headers.get("content-type") ?? "";
    if (!response.ok) {
      if (contentType.includes("application/json")) {
        throw await response.json();
      }
      throw new Error(await response.text());
    }
    if (contentType.includes("application/octet-stream")) {
      return response.arrayBuffer();
    }
    if (contentType.includes("application/json")) {
      return response.json();
    }
    return response.text();
  }

  function convertFileSrc(filePath: string, protocol = "asset"): string {
    const session = options.getSession();
    return `/scheme/${encodeURIComponent(protocol)}/${encodeURIComponent(
      session.id,
    )}/${encodeURIComponent(filePath)}`;
  }

  const tauriInternals: TauriInternalsLike = {
    invoke,
    transformCallback,
    unregisterCallback,
    runCallback,
    callbacks,
    convertFileSrc,
    metadata,
    plugins: {
      path: {
        sep: "/",
        delimiter: ":",
      },
    },
  };

  const eventPluginInternals: TauriEventPluginInternalsLike = {
    unregisterListener(event: string, eventId: number): void {
      void invoke("plugin:event|unlisten", { event, eventId }).catch(() => {});
    },
  };

  return { tauriInternals, eventPluginInternals };
}
