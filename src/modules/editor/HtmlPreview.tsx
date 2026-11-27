import { checkReadable } from "@/modules/ai/lib/security";
import { previewBoundary } from "@/modules/markdown/lib/assetPath";
import { currentWorkspaceEnv } from "@/modules/workspace";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { useEffect, useRef, useState } from "react";
import {
  createHtmlPreviewPublisher,
  hasActiveContent,
  type PreviewScroll,
  readScrollMessage,
  withScrollBridge,
} from "./lib/htmlPreview";
import { isPreviewTrusted, trustPreview } from "./lib/htmlPreviewTrust";
import {
  hasRemoteScripts,
  maybeInlineHtmlAssets,
} from "./lib/inlineHtmlAssets";

type Props = {
  html: string;
  /** Absolute path of the HTML file, used to resolve relative assets (CSS, images). */
  path?: string | null;
  /** Folder tree local assets may be inlined from; defaults to the doc folder. */
  workspaceRoot?: string | null;
};

/** Why some of the page's scripts did not run: local files, or the web. */
type BlockedScripts = "local" | "remote" | null;

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
 *
 * A script can still send out whatever it can read, and what it can read
 * includes the local files inlined into the document. So scripts run only
 * when nothing local was inlined, or after the user trusted the folder; until
 * then the page is served with scripts off and a bar offers to turn them on.
 * The folder's own script files and libraries from the web (a CDN) run only
 * in a trusted folder; a page that needs them shows the same bar.
 */
export function HtmlPreview({ html, path, workspaceRoot }: Props) {
  const [src, setSrc] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [scriptsBlocked, setScriptsBlocked] = useState<BlockedScripts>(null);
  const publisher = useRef<ReturnType<typeof createHtmlPreviewPublisher> | null>(
    null,
  );
  const frame = useRef<HTMLIFrameElement | null>(null);
  const scroll = useRef<PreviewScroll>({ x: 0, y: 0 });
  // Which scripts each published token was served without, if any.
  const blockedByToken = useRef(new Map<string, BlockedScripts>());
  const pathRef = useRef(path);
  pathRef.current = path;
  const workspaceRootRef = useRef(workspaceRoot);
  workspaceRootRef.current = workspaceRoot;

  useEffect(() => {
    const blocked = blockedByToken.current;
    const instance = createHtmlPreviewPublisher({
      publish: async (source) => {
        const docPath = pathRef.current;
        const boundary = docPath
          ? previewBoundary(docPath, workspaceRootRef.current ?? null)
          : null;
        const inlined = await maybeInlineHtmlAssets(
          source,
          docPath,
          {
            readText: readTextForPreview,
            readBinary: readBinaryForPreview,
          },
          boundary
            ? { boundary, isReadable: (abs) => checkReadable(abs).ok }
            : undefined,
        );
        // Inlining leaves the markup untouched when it pulled in nothing.
        const usedLocalFiles = inlined !== source;
        const trusted = boundary !== null && isPreviewTrusted(boundary);
        const scripts = !usedLocalFiles || trusted;
        const token = await invoke<string>("fs_publish_html", {
          html: scripts ? withScrollBridge(inlined, scroll.current) : inlined,
          scripts,
          trusted,
        });
        blocked.set(
          token,
          !scripts && hasActiveContent(inlined)
            ? "local"
            : !trusted && boundary !== null && hasRemoteScripts(inlined)
              ? "remote"
              : null,
        );
        return token;
      },
      drop: async (token) => {
        blocked.delete(token);
        try {
          await invoke("fs_drop_html", { token });
        } catch {
          // A consumed or evicted token is already gone.
        }
      },
      onReady: (token) => {
        setScriptsBlocked(blocked.get(token) ?? null);
        setSrc(convertFileSrc(token, "terax-doc"));
        setError(null);
      },
      onError: (message) => setError(message),
    });
    publisher.current = instance;
    return () => {
      instance.dispose();
      publisher.current = null;
      blocked.clear();
    };
  }, []);

  // The frame reports its scroll position so the next render can restore it.
  useEffect(() => {
    const onMessage = (event: MessageEvent) => {
      if (event.source !== frame.current?.contentWindow) return;
      const next = readScrollMessage(event.data);
      if (next) scroll.current = next;
    };
    window.addEventListener("message", onMessage);
    return () => window.removeEventListener("message", onMessage);
  }, []);

  // A different document starts at the top.
  // biome-ignore lint/correctness/useExhaustiveDependencies: reset when the file changes.
  useEffect(() => {
    scroll.current = { x: 0, y: 0 };
  }, [path]);

  // biome-ignore lint/correctness/useExhaustiveDependencies: path and workspaceRoot are read through refs when publishing.
  useEffect(() => {
    publisher.current?.request(html);
  }, [html, path, workspaceRoot]);

  const allowScripts = () => {
    if (!path) return;
    trustPreview(previewBoundary(path, workspaceRoot ?? null));
    publisher.current?.request(html);
  };

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
    <div className="flex h-full min-h-0 flex-col">
      {scriptsBlocked && (
        <div className="flex shrink-0 items-center justify-between gap-3 border-b border-border bg-muted/60 py-1.5 pl-3 pr-44 text-xs text-muted-foreground">
          <span>
            {scriptsBlocked === "local"
              ? "Scripts are off: this page loads local files, and its scripts could read them and send them out."
              : "Scripts from the web are off: this page loads libraries from a CDN."}
          </span>
          <button
            type="button"
            onClick={allowScripts}
            className="shrink-0 rounded border border-border bg-background px-2 py-0.5 text-foreground hover:bg-accent"
          >
            Run scripts for this folder
          </button>
        </div>
      )}
      <iframe
        ref={frame}
        key={src}
        src={src}
        title="HTML preview"
        sandbox="allow-scripts"
        referrerPolicy="no-referrer"
        className="min-h-0 w-full flex-1 border-none bg-white"
      />
    </div>
  );
}
