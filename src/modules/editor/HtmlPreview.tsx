import { checkReadable } from "@/modules/ai/lib/security";
import { previewBoundary } from "@/modules/markdown/lib/assetPath";
import { currentWorkspaceEnv } from "@/modules/workspace";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { useEffect, useRef, useState } from "react";
import { maybeInlineHtmlAssets } from "./lib/inlineHtmlAssets";
import { createHtmlPreviewPublisher } from "./lib/htmlPreview";

type Props = {
  html: string;
  /** Absolute path of the HTML file, used to resolve relative assets (CSS, images). */
  path?: string | null;
  /** Folder tree local assets may be inlined from; defaults to the doc folder. */
  workspaceRoot?: string | null;
};

type ReadResult =
  | { kind: "text"; content: string }
  | { kind: "binary"; size: number }
  | { kind: "toolarge"; size: number; limit: number };

type ReadBinaryResult =
  | { kind: "binary"; content: string; size: number }
  | { kind: "toolarge"; size: number; limit: number };

async function readTextForPreview(path: string): Promise<string> {
  const result = await invoke<ReadResult>("fs_read_file", {
    path,
    workspace: currentWorkspaceEnv(),
  });
  if (result.kind === "text") return result.content;
  throw new Error(result.kind === "binary" ? "binary file" : "file too large");
}

async function readBinaryForPreview(path: string): Promise<string> {
  const result = await invoke<ReadBinaryResult>("fs_read_binary", {
    path,
    workspace: currentWorkspaceEnv(),
  });
  if (result.kind === "binary") return result.content;
  throw new Error("file too large");
}

/**
 * Renders HTML from the live editor buffer in an isolated frame.
 *
 * The document is served over the `terax-doc` scheme with a strict CSP header.
 * `sandbox` omits `allow-same-origin`, so the frame gets an opaque origin and
 * its scripts cannot reach this window, Tauri IPC, or storage.
 */
export function HtmlPreview({ html, path, workspaceRoot }: Props) {
  const [src, setSrc] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const publisher = useRef<ReturnType<typeof createHtmlPreviewPublisher> | null>(
    null,
  );
  const pathRef = useRef(path);
  pathRef.current = path;
  const workspaceRootRef = useRef(workspaceRoot);
  workspaceRootRef.current = workspaceRoot;

  useEffect(() => {
    const instance = createHtmlPreviewPublisher({
      publish: async (source) => {
        const docPath = pathRef.current;
        const inlined = await maybeInlineHtmlAssets(
          source,
          docPath,
          {
            readText: readTextForPreview,
            readBinary: readBinaryForPreview,
          },
          docPath
            ? {
                boundary: previewBoundary(docPath, workspaceRootRef.current ?? null),
                isReadable: (abs) => checkReadable(abs).ok,
              }
            : undefined,
        );
        return invoke<string>("fs_publish_html", { html: inlined });
      },
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
  }, [html, path, workspaceRoot]);

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