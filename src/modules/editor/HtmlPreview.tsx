import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { useEffect, useRef, useState } from "react";
import { createHtmlPreviewPublisher } from "./lib/htmlPreview";

type Props = {
  html: string;
};

/**
 * Renders HTML from the live editor buffer in an isolated frame.
 *
 * The document is served over the `terax-doc` scheme with a strict CSP header.
 * `sandbox` omits `allow-same-origin`, so the frame gets an opaque origin and
 * its scripts cannot reach this window, Tauri IPC, or storage.
 */
export function HtmlPreview({ html }: Props) {
  const [src, setSrc] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const publisher = useRef<ReturnType<typeof createHtmlPreviewPublisher> | null>(
    null,
  );

  useEffect(() => {
    const instance = createHtmlPreviewPublisher({
      publish: (source) =>
        invoke<string>("fs_publish_html", { html: source }),
      drop: async (token) => {
        try {
          await invoke("fs_drop_html", { token });
        } catch {
          // A consumed or evicted token is already gone.
        }
      },
      onReady: (token) => {
        setSrc(convertFileSrc(token, "terax-doc"));
        setError(null);
      },
      onError: (message) => setError(message),
    });
    publisher.current = instance;
    return () => {
      instance.dispose();
      publisher.current = null;
    };
  }, []);

  useEffect(() => {
    publisher.current?.request(html);
  }, [html]);

  if (error) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-1 px-6 text-center">
        <div className="text-sm text-destructive">Preview unavailable</div>
        <div className="text-xs text-muted-foreground">{error}</div>
      </div>
    );
  }
  if (!src) {
    return (
      <div className="flex h-full items-center justify-center text-xs text-muted-foreground">
        Rendering preview...
      </div>
    );
  }
  return (
    <iframe
      key={src}
      src={src}
      title="HTML preview"
      sandbox="allow-scripts"
      referrerPolicy="no-referrer"
      className="h-full w-full border-none bg-white"
    />
  );
}