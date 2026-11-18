import { endpointIdFromCompatModel } from "@/modules/ai/config";
import { getCustomEndpointKey, getKey } from "@/modules/ai/lib/keyring";
import { lspFormatDocument, useLspExtension } from "@/modules/lsp";
import { usePreferencesStore } from "@/modules/settings/preferences";
import { onKeysChanged } from "@/modules/settings/store";
import { acceptCompletion, startCompletion } from "@codemirror/autocomplete";
import { redo, undo } from "@codemirror/commands";
import {
  findNext,
  findPrevious,
  gotoLine,
  openSearchPanel,
  SearchQuery,
  setSearchQuery,
} from "@codemirror/search";
import { Prec } from "@codemirror/state";
import { EditorView, keymap } from "@codemirror/view";
import { vim } from "@replit/codemirror-vim";
import { useAssetGrant } from "@/lib/useAssetGrant";
import { cn, isHtmlPath, isMarkdownPath } from "@/lib/utils";
import {
  type MarkdownViewMode,
  MarkdownViewToggle,
} from "@/modules/markdown";
import type { MarkdownViewHandle } from "@/modules/markdown/MarkdownPreviewPane";
import CodeMirror, { type ReactCodeMirrorRef } from "@uiw/react-codemirror";
import {
  forwardRef,
  lazy,
  memo,
  Suspense,
  useCallback,
  useDeferredValue,
  useEffect,
  useImperativeHandle,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { toast } from "sonner";
import {
  inlineCompletion,
  triggerInlineCompletion,
} from "./lib/autocomplete/inlineExtension";
import { diagnosticsReporter } from "./lib/diagnosticsReporter";
import { useDiagnosticsStore } from "./lib/diagnosticsStore";
import {
  buildSharedExtensions,
  DEFAULT_INDENT,
  indentCompartment,
  indentExtension,
  languageCompartment,
  lspCompartment,
  vimCompartment,
  wordWrapExtension,
  wrapCompartment,
} from "./lib/extensions";
import {
  applyFormattedContent,
  readFileText,
  resolveFormatter,
  runExternalFormatter,
} from "./lib/externalFormat";
import { detectIndentUnit } from "./lib/indent";
import { type LanguageResult, resolveLanguage } from "./lib/languageResolver";
import { FORCE_READ_LIMIT, useDocument } from "./lib/useDocument";
import { useEditorThemeExt } from "./lib/useEditorThemeExt";
import { initVimGlobals, vimHandlersExtension } from "./lib/vim";
import {
  isSvgPath,
  mediaPreviewKind,
  svgDataUrl,
  SVG_RENDER_MAX_CHARS,
} from "./lib/mediaPreview";
import { fileNameOf, ImagePreview } from "./ImagePreview";
import { CsvHeadPreview } from "./CsvHeadPreview";
import { MediaPreview } from "./MediaPreview";
import { delimitedTextKind, isDelimitedPath } from "./lib/csvPreview";
import { CsvPreview } from "./CsvPreview";
import { DocxPreview } from "./DocxPreview";
import { HtmlPreview } from "./HtmlPreview";
import { SpreadsheetPreview } from "./SpreadsheetPreview";

initVimGlobals();

// The markdown renderer is a separate chunk: only markdown files that show
// their preview next to (or instead of) the source ever load it.
const MarkdownDocumentView = lazy(() =>
  import("@/modules/markdown/MarkdownPreviewPane").then((module) => ({
    default: module.MarkdownDocumentView,
  })),
);

export type EditorPaneHandle = {
  setQuery: (q: string) => void;
  findNext: () => void;
  findPrevious: () => void;
  clearQuery: () => void;
  /** Open CodeMirror's find/replace panel. */
  openSearch: () => void;
  focus: () => void;
  getSelection: () => string | null;
  getPath: () => string;
  /** Re-read the file from disk. Skips silently if the buffer is dirty. */
  reload: () => boolean;
  /** Move the cursor to a 1-based line and center it, once content is ready. */
  gotoLine: (line: number, options?: { focus?: boolean }) => void;
  /** Apply CodeMirror's undo/redo commands. */
  undo: () => void;
  redo: () => void;
  /** Persist the buffer (format-on-save + write), same as Mod+S / :w. */
  save: () => Promise<void>;
  /** Request an AI ghost suggestion at the cursor. */
  triggerAiComplete: () => void;
  /** Open CodeMirror's completion popup. */
  triggerCodeComplete: () => void;
};

type Props = {
  path: string;
  overrideLanguage?: string | null;
  onDirtyChange?: (dirty: boolean) => void;
  onSaved?: () => void;
  onClose?: () => void;
  mediaActive?: boolean;
  /** False while this editor's tab is in the background. */
  visible?: boolean;
  /** Workspace root for scoping local assets in HTML previews. */
  workspaceRoot?: string | null;
  /** Markdown files: source, live preview, or both side by side. */
  markdownView?: MarkdownViewMode;
  /** Open a file that a link in the markdown preview points at. */
  onOpenFile?: (path: string) => void;
};

// Above this, syntax highlighting and LSP are disabled: a multi-MB lezer
// parse tree and a didOpen of that size cost far more than they give.
const SYNTAX_MAX_BYTES = 4 * 1024 * 1024;

function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / 1024 / 1024).toFixed(1)} MB`;
}

// memo: EditorStack passes identity-stable props, so background editors
// skip re-rendering entirely when App re-renders (terminal events, tab churn).
export const EditorPane = memo(
  forwardRef<EditorPaneHandle, Props>(function EditorPane(props, ref) {
    const {
      path,
      overrideLanguage,
      onDirtyChange,
      onSaved,
      onClose,
      mediaActive = true,
      visible = true,
      workspaceRoot = null,
      markdownView,
      onOpenFile,
    } = props;

    const { doc, onChange, save, reload, adoptDiskText, openAnyway } =
      useDocument({
        path,
        onDirtyChange,
      });
    const assetGrant = useAssetGrant(
      (doc.status === "binary" || doc.status === "toolarge") &&
        mediaPreviewKind(path)
        ? path
        : null,
    );
    const reloadRef = useRef(reload);
    reloadRef.current = reload;
    const adoptDiskTextRef = useRef(adoptDiskText);
    adoptDiskTextRef.current = adoptDiskText;
    const cmRef = useRef<ReactCodeMirrorRef>(null);
    const themeExt = useEditorThemeExt();
    const vimMode = usePreferencesStore((s) => s.vimMode);
    const wordWrapColumn = usePreferencesStore((s) =>
      s.editorWordWrap ? s.editorWordWrapColumn : null,
    );
    const languageRef = useRef<string | null>(null);
    const [langId, setLangId] = useState<string | null>(null);
    const [csvRawPath, setCsvRawPath] = useState<string | null>(null);
    const csvRaw = csvRawPath === path;
    const [htmlSourcePath, setHtmlSourcePath] = useState<string | null>(null);
    const htmlSource = htmlSourcePath === path;

    // Markdown: the editor stays mounted in every mode (hidden in "rendered"),
    // so switching views keeps undo history, the cursor and unsaved text. The
    // preview renders the live buffer, not the file on disk.
    const mdMode: MarkdownViewMode = isMarkdownPath(path)
      ? (markdownView ?? "raw")
      : "raw";
    const mdModeRef = useRef(mdMode);
    mdModeRef.current = mdMode;
    const previewRef = useRef<MarkdownViewHandle | null>(null);
    /** Line to show once the preview has mounted. */
    const pendingPreviewLineRef = useRef<number | null>(null);
    /** Source line at the top of the preview, as it last reported. */
    const previewLineRef = useRef(1);
    const setPreviewRef = useCallback((handle: MarkdownViewHandle | null) => {
      previewRef.current = handle;
      if (handle && pendingPreviewLineRef.current !== null) {
        handle.scrollToSourceLine(pendingPreviewLineRef.current);
        pendingPreviewLineRef.current = null;
      }
    }, []);
    const onPreviewLine = useCallback((line: number) => {
      previewLineRef.current = line;
    }, []);
    // `doc.content` is only the text as loaded: keystrokes go to a ref, not to
    // state, so the editor does not re-render on every one. The preview needs
    // the buffer itself, so it gets its own copy, updated while it is showing.
    const [liveText, setLiveText] = useState("");
    const liveTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
    useEffect(() => {
      if (doc.status === "ready") setLiveText(doc.content);
    }, [doc]);
    useEffect(
      () => () => {
        if (liveTimerRef.current) clearTimeout(liveTimerRef.current);
      },
      [],
    );
    const handleChange = useCallback(
      (next: string) => {
        onChange(next);
        if (mdModeRef.current === "raw") return;
        if (liveTimerRef.current) clearTimeout(liveTimerRef.current);
        liveTimerRef.current = setTimeout(() => setLiveText(next), 120);
      },
      [onChange],
    );
    // Deferred, so typing stays responsive while a long document re-renders.
    const previewText = useDeferredValue(liveText);
    const previewStatus = useMemo(
      () => ({ kind: "ready" as const, content: previewText }),
      [previewText],
    );
    const apiKeyRef = useRef<string | null>(null);

    useEffect(() => {
      let cancelled = false;
      const refresh = async () => {
        const s = usePreferencesStore.getState();
        const provider = s.autocompleteProvider;
        if (
          provider === "lmstudio" ||
          provider === "mlx" ||
          provider === "ollama"
        ) {
          apiKeyRef.current = null;
          return;
        }
        // OpenAI-compatible keys live in a per-endpoint keyring slot.
        if (provider === "openai-compatible") {
          const eid = endpointIdFromCompatModel(s.autocompleteModelId);
          const k = eid ? await getCustomEndpointKey(eid) : null;
          if (!cancelled) apiKeyRef.current = k;
          return;
        }
        const k = await getKey(provider);
        if (!cancelled) apiKeyRef.current = k;
      };
      void refresh();
      let unlistenKeys: (() => void) | undefined;
      void onKeysChanged(() => void refresh()).then((un) => {
        if (cancelled) un();
        else unlistenKeys = un;
      });
      const unsubPrefs = usePreferencesStore.subscribe((state, prev) => {
        if (
          state.autocompleteProvider !== prev.autocompleteProvider ||
          state.autocompleteModelId !== prev.autocompleteModelId
        ) {
          void refresh();
        }
      });
      return () => {
        cancelled = true;
        unlistenKeys?.();
        unsubPrefs();
      };
    }, []);
    // Stabilize save + onSaved via refs so the extensions array never changes
    // identity — a new identity makes @uiw/react-codemirror reconfigure the
    // whole state, wiping the language compartment.
    const saveRef = useRef(save);
    saveRef.current = save;
    const onSavedRef = useRef(onSaved);
    onSavedRef.current = onSaved;
    const onCloseRef = useRef(onClose);
    onCloseRef.current = onClose;
    const lspActiveRef = useRef(false);
    const warnedNoLspRef = useRef(false);
    const warnedNoFormatRef = useRef(false);

    const performSave = useCallback(async () => {
      const view = cmRef.current?.view;
      const prefs = usePreferencesStore.getState();
      const formatter = resolveFormatter(languageRef.current, prefs);
      if (prefs.editorFormatOnSave && formatter === "lsp" && view) {
        if (lspActiveRef.current) {
          let res: "done" | "unsupported" = "done";
          try {
            res = await lspFormatDocument(view);
          } catch (e) {
            toast.error("Language server format failed", {
              description: String(e),
            });
          }
          if (res === "unsupported" && !warnedNoFormatRef.current) {
            warnedNoFormatRef.current = true;
            toast.warning("Format on save skipped", {
              description:
                "The active language server has no formatter. Pick an external one in Settings (Ruff for Python, Prettier, rustfmt, ...).",
            });
          }
        } else if (!warnedNoLspRef.current) {
          warnedNoLspRef.current = true;
          toast.warning("Format on save skipped", {
            description:
              "No active language server for this file. Enable one in the statusbar, or pick an external formatter in Settings.",
          });
        }
      }
      // Snapshot before save: edits typed during the formatter round-trip
      // must not be clobbered by the disk read-back.
      const docAtSave = view?.state.doc;
      const saved = await saveRef.current();
      if (!saved) return;
      if (prefs.editorFormatOnSave && formatter !== "lsp") {
        const error = await runExternalFormatter(
          formatter,
          pathRef.current,
          prefs.editorCustomFormatCommand,
        );
        if (error) {
          toast.error(`${formatter} format failed`, { description: error });
        } else {
          const readBack = await readFileText(pathRef.current);
          if (readBack !== null && view && view.state.doc === docAtSave) {
            applyFormattedContent(
              view,
              adoptDiskTextRef.current(readBack.text, readBack.mtime),
            );
          }
        }
      }
      onSavedRef.current?.();
    }, []);
    const performSaveRef = useRef(performSave);
    performSaveRef.current = performSave;

    const pathRef = useRef(path);
    pathRef.current = path;

    const pendingLineRef = useRef<{
      path: string;
      line: number;
      focus: boolean;
    } | null>(null);
    const pendingFocusRef = useRef<string | null>(null);
    const statusRef = useRef(doc.status);
    useLayoutEffect(() => {
      statusRef.current = doc.status;
    }, [doc.status]);

    useEffect(() => {
      if (pendingLineRef.current?.path !== path) {
        pendingLineRef.current = null;
      }
      if (pendingFocusRef.current !== path) {
        pendingFocusRef.current = null;
      }
    }, [path]);

    const focusWhenRendered = useCallback(
      (view: EditorView, targetPath: string) => {
        requestAnimationFrame(() => {
          if (cmRef.current?.view === view && pathRef.current === targetPath) {
            view.focus();
          }
        });
      },
      [],
    );

    const applyPendingGoto = useCallback(() => {
      const view = cmRef.current?.view;
      const pending = pendingLineRef.current;
      if (!view || pending == null || statusRef.current !== "ready") return;
      if (pending.path !== path) {
        pendingLineRef.current = null;
        return;
      }
      const target = Math.max(1, Math.min(pending.line, view.state.doc.lines));
      const at = view.state.doc.line(target).from;
      view.dispatch({
        selection: { anchor: at },
        effects: EditorView.scrollIntoView(at, { y: "center" }),
      });
      if (pending.focus) focusWhenRendered(view, pending.path);
      pendingLineRef.current = null;
    }, [focusWhenRendered, path]);

    const applyPendingFocus = useCallback(() => {
      const view = cmRef.current?.view;
      const pendingPath = pendingFocusRef.current;
      if (!view || pendingPath === null || statusRef.current !== "ready")
        return;
      pendingFocusRef.current = null;
      if (pendingPath === path) focusWhenRendered(view, pendingPath);
    }, [focusWhenRendered, path]);

    useEffect(() => {
      if (doc.status !== "ready") return;
      applyPendingGoto();
      applyPendingFocus();
    }, [doc.status, applyPendingFocus, applyPendingGoto]);

    const extensions = useMemo(
      () => [
        // basicSetup is added before user extensions by @uiw/react-codemirror,
        // so we must elevate vim's precedence to win the keymap.
        vimCompartment.of(
          usePreferencesStore.getState().vimMode ? Prec.highest(vim()) : [],
        ),
        wrapCompartment.of(
          wordWrapExtension(
            usePreferencesStore.getState().editorWordWrap
              ? usePreferencesStore.getState().editorWordWrapColumn
              : null,
          ),
        ),
        vimHandlersExtension(() => ({
          save: () => {
            void performSaveRef.current();
          },
          close: () => onCloseRef.current?.(),
        })),
        ...buildSharedExtensions(),
        indentCompartment.of(DEFAULT_INDENT),
        languageCompartment.of([]),
        lspCompartment.of([]),
        diagnosticsReporter(() => pathRef.current),
        // Before inlineCompletion so an open popup wins Tab over the ghost.
        Prec.highest(keymap.of([{ key: "Tab", run: acceptCompletion }])),
        inlineCompletion({
          getPrefs: () => {
            const s = usePreferencesStore.getState();
            const p = s.autocompleteProvider;
            // autocompleteModelId holds the compat- id of the chosen endpoint.
            const compatEp =
              p === "openai-compatible"
                ? s.customEndpoints.find(
                    (e) =>
                      e.id === endpointIdFromCompatModel(s.autocompleteModelId),
                  )
                : undefined;
            const modelId =
              p === "lmstudio"
                ? s.lmstudioModelId
                : p === "mlx"
                  ? s.mlxModelId
                  : p === "ollama"
                    ? s.ollamaModelId
                    : p === "openai-compatible"
                      ? (compatEp?.modelId ?? "")
                      : p === "openrouter"
                        ? s.openrouterModelId
                        : s.autocompleteModelId;
            return {
              enabled: s.autocompleteEnabled,
              trigger: s.autocompleteTrigger,
              provider: p,
              modelId,
              apiKey: apiKeyRef.current,
              lmstudioBaseURL: s.lmstudioBaseURL,
              mlxBaseURL: s.mlxBaseURL,
              ollamaBaseURL: s.ollamaBaseURL,
              openaiCompatibleBaseURL:
                compatEp?.baseURL ?? s.openaiCompatibleBaseURL,
            };
          },
          getPath: () => pathRef.current,
          getLanguage: () => languageRef.current,
        }),
        keymap.of([
          {
            key: "Mod-s",
            preventDefault: true,
            run: () => {
              void performSaveRef.current();
              return true;
            },
          },
          { key: "Ctrl-g", run: gotoLine },
        ]),
      ],
      [],
    );

    useEffect(() => {
      const view = cmRef.current?.view;
      if (!view) return;
      view.dispatch({
        effects: vimCompartment.reconfigure(vimMode ? Prec.highest(vim()) : []),
      });
    }, [vimMode]);

    useEffect(() => {
      const view = cmRef.current?.view;
      if (!view) return;
      view.dispatch({
        effects: wrapCompartment.reconfigure(wordWrapExtension(wordWrapColumn)),
      });
    }, [wordWrapColumn]);

    useEffect(() => {
      if (doc.status !== "ready") return;
      const view = cmRef.current?.view;
      if (!view) return;
      view.dispatch({
        effects: indentCompartment.reconfigure(
          indentExtension(detectIndentUnit(doc.content)),
        ),
      });
    }, [doc]);

    /** 1-based source line at the top of the editor's viewport. */
    const topEditorLine = useCallback(() => {
      const view = cmRef.current?.view;
      if (!view) return 1;
      const block = view.lineBlockAtHeight(view.scrollDOM.scrollTop);
      return view.state.doc.lineAt(block.from).number;
    }, []);

    // Carry the reading position across a view switch.
    const prevMdModeRef = useRef(mdMode);
    useLayoutEffect(() => {
      const prev = prevMdModeRef.current;
      prevMdModeRef.current = mdMode;
      if (prev === mdMode) return;
      const view = cmRef.current?.view;
      if (prev === "rendered" && view) {
        // The hidden editor lost its scroll offset; reopen it where the
        // preview was being read.
        const line = Math.max(
          1,
          Math.min(previewLineRef.current, view.state.doc.lines),
        );
        view.requestMeasure();
        view.dispatch({
          effects: EditorView.scrollIntoView(view.state.doc.line(line).from, {
            y: "start",
          }),
        });
      } else if (prev === "raw") {
        // Nothing fed the preview while it was hidden; give it the buffer.
        if (view) setLiveText(view.state.doc.toString());
        const line = topEditorLine();
        if (previewRef.current) previewRef.current.scrollToSourceLine(line);
        else pendingPreviewLineRef.current = line;
      }
    }, [mdMode, topEditorLine]);

    // Side by side, the preview follows the editor as it scrolls.
    useEffect(() => {
      if (mdMode !== "split" || doc.status !== "ready") return;
      const scroller = cmRef.current?.view?.scrollDOM;
      if (!scroller) return;
      let frame = 0;
      const onScroll = () => {
        if (frame) return;
        frame = requestAnimationFrame(() => {
          frame = 0;
          previewRef.current?.scrollToSourceLine(topEditorLine());
        });
      };
      scroller.addEventListener("scroll", onScroll, { passive: true });
      return () => {
        scroller.removeEventListener("scroll", onScroll);
        cancelAnimationFrame(frame);
      };
    }, [mdMode, doc.status, topEditorLine]);

    const lspExt = useLspExtension(path, langId, doc.status === "ready");
    useEffect(() => {
      lspActiveRef.current = lspExt !== null;
      const view = cmRef.current?.view;
      if (!view) return;
      view.dispatch({
        effects: lspCompartment.reconfigure(lspExt ?? []),
      });
    }, [lspExt]);

    useEffect(
      () => () => useDiagnosticsStore.getState().report(pathRef.current, null),
      [],
    );

    // Warm the language chunk while the file is still being read; the
    // ready-gated effect below then resolves from cache.
    useEffect(() => {
      const resolvePath = overrideLanguage ? `dummy.${overrideLanguage}` : path;
      void resolveLanguage(resolvePath).catch(() => {});
    }, [path, overrideLanguage]);

    const documentSize = doc.status === "ready" ? doc.size : 0;

    useEffect(() => {
      const ext =
        overrideLanguage || (path.split(".").pop()?.toLowerCase() ?? null);
      languageRef.current = ext;
      if (doc.status !== "ready") return;
      if (documentSize > SYNTAX_MAX_BYTES) {
        setLangId(null);
        const view = cmRef.current?.view;
        view?.dispatch({ effects: languageCompartment.reconfigure([]) });
        return;
      }
      let cancelled = false;
      const resolve = async (): Promise<LanguageResult> => {
        const resolvePath = overrideLanguage
          ? `dummy.${overrideLanguage}`
          : path;
        return (
          (await resolveLanguage(resolvePath)) ?? { ext: [], name: "", id: "" }
        );
      };
      void resolve().then((result) => {
        if (cancelled) return;
        if (result.id) languageRef.current = result.id;
        setLangId(result.id || ext);
        const view = cmRef.current?.view;
        if (!view) return;
        view.dispatch({
          effects: languageCompartment.reconfigure(result.ext),
        });
      });
      return () => {
        cancelled = true;
      };
    }, [path, doc.status, documentSize, overrideLanguage]);

    useImperativeHandle(
      ref,
      () => ({
        setQuery: (q: string) => {
          const view = cmRef.current?.view;
          if (!view) return;
          view.dispatch({
            effects: setSearchQuery.of(
              new SearchQuery({ search: q, caseSensitive: false }),
            ),
          });
          if (q) findNext(view);
        },
        findNext: () => {
          const view = cmRef.current?.view;
          if (view) findNext(view);
        },
        findPrevious: () => {
          const view = cmRef.current?.view;
          if (view) findPrevious(view);
        },
        clearQuery: () => {
          const view = cmRef.current?.view;
          if (!view) return;
          view.dispatch({
            effects: setSearchQuery.of(new SearchQuery({ search: "" })),
          });
        },
        openSearch: () => {
          // With the source hidden, search the preview instead.
          if (mdModeRef.current === "rendered") {
            previewRef.current?.openFind();
            return;
          }
          const view = cmRef.current?.view;
          if (view) openSearchPanel(view);
        },
        focus: () => {
          pendingFocusRef.current = path;
          applyPendingFocus();
        },
        getSelection: () => {
          const view = cmRef.current?.view;
          if (!view) return null;
          const { from, to } = view.state.selection.main;
          if (from === to) return null;
          return view.state.sliceDoc(from, to);
        },
        getPath: () => path,
        reload: () => reloadRef.current(),
        gotoLine: (line: number, options) => {
          pendingLineRef.current = {
            path,
            line,
            focus: options?.focus ?? true,
          };
          applyPendingGoto();
        },
        undo: () => {
          const view = cmRef.current?.view;
          if (view) undo(view);
        },
        redo: () => {
          const view = cmRef.current?.view;
          if (view) redo(view);
        },
        save: () => performSaveRef.current(),
        triggerAiComplete: () => {
          const view = cmRef.current?.view;
          if (view) triggerInlineCompletion(view);
        },
        triggerCodeComplete: () => {
          const view = cmRef.current?.view;
          if (!view) return;
          view.focus();
          startCompletion(view);
        },
      }),
      [path, applyPendingFocus, applyPendingGoto],
    );

    if (doc.status === "loading") {
      return (
        <div className="flex h-full items-center justify-center text-xs text-muted-foreground">
          Loading…
        </div>
      );
    }
    if (doc.status === "error") {
      return (
        <div className="flex h-full items-center justify-center px-6 text-center text-xs text-destructive">
          {doc.message}
        </div>
      );
    }
    if (doc.status === "binary" || doc.status === "toolarge") {
      const kind = mediaPreviewKind(path);
      const isImage = kind === "image";
      const isVideo = kind === "video";
      const isAudio = kind === "audio";
      const isPdf = kind === "pdf";
      const isDocx = kind === "docx";
      const isXlsx = kind === "xlsx";

      // Office documents are read over IPC, so they need no asset grant.
      if (isDocx || isXlsx) {
        return (
          // The previews scroll themselves; a centering, padded, scrolling
          // wrapper around them doubled the scrollbars.
          <div className="h-full min-h-0 bg-background">
            {isDocx && <DocxPreview path={path} size={doc.size} />}
            {isXlsx && <SpreadsheetPreview path={path} size={doc.size} />}
          </div>
        );
      }

      if (isImage || isVideo || isAudio || isPdf) {
        const assetUrl = assetGrant.urlFor(path);
        if (!assetUrl)
          return (
            <div className="p-4 text-xs text-muted-foreground">
              {assetGrant.error ? (
                <>
                  <p role="alert">
                    Media preview unavailable: {assetGrant.error}
                  </p>
                  <button
                    type="button"
                    className="mt-2 underline"
                    onClick={assetGrant.retry}
                  >
                    Retry
                  </button>
                </>
              ) : (
                "Loading media preview…"
              )}
            </div>
          );
        if (isImage) {
          return (
            <ImagePreview
              src={assetUrl}
              name={fileNameOf(path)}
              sizeLabel={formatBytes(doc.size)}
            />
          );
        }
        return (
          <div className="flex h-full min-h-0 flex-col items-center justify-center bg-background p-4 overflow-auto">
            {(kind === "video" || kind === "audio") && (
              <MediaPreview kind={kind} src={assetUrl} active={mediaActive} />
            )}
            {isPdf && (
              <iframe
                src={assetUrl}
                className="w-full h-full border-none"
                title={fileNameOf(path)}
              />
            )}
          </div>
        );
      }

      const canForce =
        doc.status === "toolarge" && doc.size <= FORCE_READ_LIMIT;
      // A table preview only needs the start of the file, whatever its size.
      if (doc.status === "toolarge" && isDelimitedPath(path)) {
        return (
          <CsvHeadPreview
            path={path}
            sizeLabel={formatBytes(doc.size)}
            onOpenAnyway={canForce ? openAnyway : undefined}
          />
        );
      }
      return (
          <TextHeadPreview
            path={path}
            sizeLabel={formatBytes(doc.size)}
            onOpenAnyway={canForce ? openAnyway : undefined}
          />
        );
      }
      return (
        <div className="flex h-full flex-col items-center justify-center gap-1 px-6 text-center">
          <div className="text-sm text-foreground">
            {doc.status === "binary" ? "Binary file" : "File too large"}
          </div>
          <div className="text-xs text-muted-foreground">
            {formatBytes(doc.size)} ·{" "}
            {canForce ? "syntax features disabled" : "preview not supported"}
          </div>
          {canForce && (
            <button
              type="button"
              onClick={openAnyway}
              className="mt-2 rounded-md border border-border bg-muted/60 px-3 py-1 text-xs text-foreground hover:bg-accent"
            >
              Open anyway
            </button>
          )}
        </div>
      );
    }

    const delimiter =
      doc.status === "ready" ? delimitedTextKind(path, doc.content) : null;
    if (doc.status === "ready" && delimiter && !csvRaw) {
      return (
        <CsvPreview
          content={doc.content}
          delimiter={delimiter}
          onEdit={() => setCsvRawPath(path)}
        />
      );
    }

    // SVG is text, so it lands here rather than in the binary branch above.
    // Very large ones stay as source: encoding them for <img> on every
    // keystroke would stall the editor.
    const isSvg =
      doc.status === "ready" &&
      isSvgPath(path) &&
      doc.content.length <= SVG_RENDER_MAX_CHARS;
    if (doc.status === "ready" && isSvg && !htmlSource) {
      return (
        <div className="relative flex h-full min-h-0 flex-col zoom-exempt">
          <MarkdownViewToggle
            mode="rendered"
            onChange={(mode) => setHtmlSourcePath(mode === "raw" ? path : null)}
          />
          <ImagePreview
            src={svgDataUrl(doc.content)}
            name={fileNameOf(path)}
            sizeLabel={formatBytes(doc.size)}
            reserveToggleSpace
          />
        </div>
      );
    }

    const isHtml = doc.status === "ready" && isHtmlPath(path);
    return (
      <div className="relative flex h-full min-h-0 flex-col zoom-exempt">
        {isSvg && (
          <MarkdownViewToggle
            mode="raw"
            onChange={(mode) => setHtmlSourcePath(mode === "raw" ? path : null)}
          />
        )}
        {isHtml && (
          <MarkdownViewToggle
            mode={htmlSource ? "raw" : "rendered"}
            onChange={(mode) => setHtmlSourcePath(mode === "raw" ? path : null)}
          />
        )}
        {isHtml && !htmlSource ? (
          <div className="h-full min-h-0 flex-1">
            <HtmlPreview html={doc.content} path={path} workspaceRoot={workspaceRoot} />
          </div>
        ) : (
          <div className="flex min-h-0 flex-1">
            <CodeMirror
              ref={cmRef}
              value={doc.content}
              onChange={handleChange}
              theme={themeExt}
              extensions={extensions}
              height="100%"
              className={cn(
                "terax-code-editor min-h-0 min-w-0 flex-1 overflow-hidden",
                mdMode === "rendered" && "hidden",
              )}
              basicSetup={{
                lineNumbers: true,
                highlightActiveLineGutter: true,
                foldGutter: true,
                bracketMatching: true,
                closeBrackets: true,
                autocompletion: true,
                highlightActiveLine: true,
                highlightSelectionMatches: true,
                searchKeymap: true,
              }}
            />
            {mdMode !== "raw" && (
              <div
                className={cn(
                  "min-h-0 min-w-0 flex-1",
                  mdMode === "split" && "border-s border-border",
                )}
              >
                <Suspense fallback={null}>
                  <MarkdownDocumentView
                    ref={setPreviewRef}
                    path={path}
                    status={previewStatus}
                    workspaceRoot={workspaceRoot}
                    visible={visible}
                    onOpenFile={onOpenFile}
                    onSourceLineChange={onPreviewLine}
                    reserveCorner
                  />
                </Suspense>
              </div>
            )}
          </div>
        )}
      </div>
    );
  }),
);
