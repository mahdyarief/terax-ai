import { checkReadable } from "@/modules/ai/lib/security";
import {
  isInsideRoot,
  previewBoundary,
  resolveRelativeAsset,
} from "@/modules/markdown/lib/assetPath";

export type ReadTextFile = (path: string) => Promise<string>;
export type ReadBinaryFile = (path: string) => Promise<string /* base64 */>;

type InlineOptions = {
  basePath: string;
  readText: ReadTextFile;
  readBinary: ReadBinaryFile;
  /** Folder tree the document may pull assets from; defaults to the doc folder. */
  boundary?: string;
  /** False for secret paths (e.g. .env); mirrors the AI read guard. */
  isReadable?: (absolutePath: string) => boolean;
  maxTotalBytes?: number;
  maxDepth?: number;
};

// Keep well below backend MAX_PREVIEW_BYTES (8 MiB) to leave room for base64
// expansion (~33%) and the surrounding HTML markup.
const DEFAULT_MAX_TOTAL = 5 * 1024 * 1024;
const DEFAULT_MAX_DEPTH = 20;

const MIME_BY_EXT: Record<string, string> = {
  css: "text/css; charset=utf-8",
  svg: "image/svg+xml",
  png: "image/png",
  jpg: "image/jpeg",
  jpeg: "image/jpeg",
  gif: "image/gif",
  webp: "image/webp",
  avif: "image/avif",
  bmp: "image/bmp",
  ico: "image/x-icon",
  woff: "font/woff",
  woff2: "font/woff2",
  ttf: "font/ttf",
  otf: "font/otf",
  eot: "application/vnd.ms-fontobject",
  mp4: "video/mp4",
  webm: "video/webm",
  ogg: "video/ogg",
  mp3: "audio/mpeg",
  wav: "audio/wav",
};

function extOf(path: string): string {
  const name = path.split(/[/\\]/).pop() ?? "";
  const i = name.lastIndexOf(".");
  return i > 0 ? name.slice(i + 1).toLowerCase() : "";
}

function mimeFor(path: string): string {
  return MIME_BY_EXT[extOf(path)] ?? "application/octet-stream";
}

function isLocalRef(ref: string): boolean {
  const t = ref.trim();
  if (!t) return false;
  const lower = t.toLowerCase();
  if (lower.startsWith("#")) return false;
  if (lower.startsWith("data:")) return false;
  if (lower.startsWith("http:") || lower.startsWith("https:")) return false;
  if (lower.startsWith("//")) return false;
  if (lower.startsWith("mailto:") || lower.startsWith("tel:")) return false;
  if (lower.startsWith("javascript:")) return false;
  if (lower.startsWith("blob:")) return false;
  // Absolute filesystem-like paths are not "relative to this html folder" in the
  // way we want to auto-inline; resolveRelativeAsset will also reject them.
  return true;
}

function splitRef(ref: string): { path: string; suffix: string } {
  const m = ref.match(/^([^?#]*)([\s\S]*)$/);
  if (!m) return { path: ref, suffix: "" };
  return { path: m[1], suffix: m[2] ?? "" };
}

function toBase64Utf8(text: string): string {
  if (typeof Buffer !== "undefined") return Buffer.from(text, "utf8").toString("base64");
  // btoa expects binary string; encode UTF-8 first.
  return btoa(unescape(encodeURIComponent(text)));
}

function approxBytesFromB64(b64: string): number {
  return Math.ceil((b64.length * 3) / 4);
}

type Ctx = {
  readText: ReadTextFile;
  readBinary: ReadBinaryFile;
  textCache: Map<string, Promise<string | null>>;
  binCache: Map<string, Promise<string | null>>;
  budget: { used: number; max: number };
  maxDepth: number;
  /** Reject absolute paths outside the preview boundary or secret paths. */
  allowed: (absolutePath: string) => boolean;
};

function charge(ctx: Ctx, bytes: number): boolean {
  ctx.budget.used += bytes;
  return ctx.budget.used <= ctx.budget.max;
}

async function readText(ctx: Ctx, abs: string): Promise<string | null> {
  if (!ctx.textCache.has(abs)) {
    ctx.textCache.set(
      abs,
      (async () => {
        try {
          const text = await ctx.readText(abs);
          if (!charge(ctx, text.length)) return null;
          return text;
        } catch {
          return null;
        }
      })(),
    );
  }
  return ctx.textCache.get(abs)!;
}

async function readBinaryAsDataUrl(ctx: Ctx, abs: string, forcedMime?: string): Promise<string | null> {
  const key = `${abs}\0${forcedMime ?? ""}`;
  if (!ctx.binCache.has(key)) {
    ctx.binCache.set(
      key,
      (async () => {
        try {
          const b64 = await ctx.readBinary(abs);
          if (!charge(ctx, approxBytesFromB64(b64))) return null;
          return `data:${forcedMime ?? mimeFor(abs)};base64,${b64}`;
        } catch {
          return null;
        }
      })(),
    );
  }
  return ctx.binCache.get(key)!;
}

async function resolveAndInlineCssUrlToken(
  rawUrl: string,
  cssFilePath: string,
  ctx: Ctx,
  depth: number,
): Promise<string | null> {
  if (!isLocalRef(rawUrl)) return null;
  const { path: refPath, suffix } = splitRef(rawUrl.trim());
  const abs = resolveRelativeAsset(cssFilePath, refPath);
  if (!abs || !ctx.allowed(abs)) return null;
  if (extOf(abs) === "css") {
    const inlined = await inlineCssFile(abs, ctx, depth + 1);
    if (inlined == null) return null;
    const b64 = toBase64Utf8(inlined);
    if (!charge(ctx, approxBytesFromB64(b64))) return null;
    return `data:text/css;charset=utf-8;base64,${b64}${suffix}`;
  }
  const data = await readBinaryAsDataUrl(ctx, abs);
  return data ? `${data}${suffix}` : null;
}

async function rewriteCssUrls(css: string, cssFilePath: string, ctx: Ctx, depth: number): Promise<string> {
  const urlRe = /url\(\s*(['"]?)([^"')]+)\1\s*\)/gi;
  let out = "";
  let last = 0;
  let m: RegExpExecArray | null;
  while ((m = urlRe.exec(css)) !== null) {
    const full = m[0];
    const raw = m[2];
    const replacement = await resolveAndInlineCssUrlToken(raw, cssFilePath, ctx, depth);
    if (replacement == null) continue;
    out += css.slice(last, m.index) + `url("${replacement}")`;
    last = m.index + full.length;
  }
  out += css.slice(last);
  return out;
}

async function inlineCssFile(absPath: string, ctx: Ctx, depth: number): Promise<string | null> {
  if (depth > ctx.maxDepth) return null;
  let css = await readText(ctx, absPath);
  if (css == null) return null;

  // Resolve @import rules so imported files are pulled in with correct base paths.
  const importRe =
    /@import\s+(?:url\(\s*(['"]?)([^"')]+)\1\s*\)|(["'])([^"']+)\3)\s*([^;]*);?/gi;
  let out = "";
  let last = 0;
  let im: RegExpExecArray | null;
  while ((im = importRe.exec(css)) !== null) {
    const full = im[0];
    const raw = im[2] ?? im[4] ?? "";
    const media = (im[5] ?? "").trim();
    out += css.slice(last, im.index);
    last = im.index + full.length;
    if (!isLocalRef(raw)) {
      out += full;
      continue;
    }
    const { path: refPath } = splitRef(raw.trim());
    const target = resolveRelativeAsset(absPath, refPath);
    if (!target || !ctx.allowed(target) || extOf(target) !== "css") {
      out += full;
      continue;
    }
    const imported = await inlineCssFile(target, ctx, depth + 1);
    if (imported == null) {
      out += full;
      continue;
    }
    out += media ? `@media ${media} {\n${imported}\n}\n` : `${imported}\n`;
  }
  out += css.slice(last);
  css = out;

  css = await rewriteCssUrls(css, absPath, ctx, depth);
  return css;
}

function getAttr(tag: string, attr: string): string | null {
  const re = new RegExp(`\\s${attr}\\s*=\\s*(?:\"([^\"]*)\"|'([^']*)'|([^\\s>]+))`, "i");
  const m = tag.match(re);
  return m ? (m[1] ?? m[2] ?? m[3] ?? "") : null;
}

function setAttr(tag: string, attr: string, value: string): string {
  const safe = value.replace(/"/g, "&quot;");
  const dq = new RegExp(`(\\s${attr}\\s*=\\s*)\"[^\"]*\"`, "i");
  if (dq.test(tag)) return tag.replace(dq, `$1"${safe}"`);
  const sq = new RegExp(`(\\s${attr}\\s*=\\s*)'[^']*'`, "i");
  if (sq.test(tag)) return tag.replace(sq, `$1"${safe}"`);
  const uq = new RegExp(`(\\s${attr}\\s*=\\s*)([^\\s>]+)`, "i");
  if (uq.test(tag)) return tag.replace(uq, `$1"${safe}"`);
  if (/\/>$/.test(tag)) return tag.replace(/\/>$/, ` ${attr}="${safe}" />`);
  return tag.replace(/>$/, ` ${attr}="${safe}">`);
}

function relIsStylesheet(tag: string): boolean {
  const rel = getAttr(tag, "rel");
  return !!rel && rel.toLowerCase().split(/\s+/).includes("stylesheet");
}

export async function inlineRelativeAssets(html: string, options: InlineOptions): Promise<string> {
  const { basePath } = options;
  // Mirror the markdown preview scoping: a document may only inline assets that
  // stay under its folder tree and under the workspace root (when in one), and
  // are not secret paths.
  const boundary = options.boundary ?? previewBoundary(basePath, null);
  const isReadable = options.isReadable ?? ((abs: string) => checkReadable(abs).ok);
  const allowed = (abs: string): boolean =>
    !!abs && isInsideRoot(boundary, abs) && isReadable(abs);
  const ctx: Ctx = {
    readText: options.readText,
    readBinary: options.readBinary,
    textCache: new Map(),
    binCache: new Map(),
    budget: { used: 0, max: options.maxTotalBytes ?? DEFAULT_MAX_TOTAL },
    maxDepth: options.maxDepth ?? DEFAULT_MAX_DEPTH,
    allowed,
  };
  let out = html;

  // 1) <link rel="stylesheet" href="local.css"> -> inline <style>
  const linkRe = /<link\b[^>]*>/gi;
  const linkMatches: Array<{ start: number; end: number; tag: string }> = [];
  let m: RegExpExecArray | null;
  while ((m = linkRe.exec(out)) !== null) {
    linkMatches.push({ start: m.index, end: m.index + m[0].length, tag: m[0] });
  }
  if (linkMatches.length) {
    const repls = await Promise.all(
      linkMatches.map(async (item) => {
        if (!relIsStylesheet(item.tag)) return null;
        const href = getAttr(item.tag, "href");
        if (!href || !isLocalRef(href)) return null;
        const { path: refPath } = splitRef(href.trim());
        const abs = resolveRelativeAsset(basePath, refPath);
        if (!abs || !ctx.allowed(abs) || extOf(abs) !== "css") return null;
        const css = await inlineCssFile(abs, ctx, 0);
        if (css == null) return null;
        const safeCss = css.replace(/<\//g, "<\\/"); // avoid breaking out of <style>
        return { ...item, text: `<style data-terax-inlined="${refPath}">\n${safeCss}\n</style>` };
      }),
    );
    for (let i = repls.length - 1; i >= 0; i--) {
      const r = repls[i];
      if (!r) continue;
      out = out.slice(0, r.start) + r.text + out.slice(r.end);
    }
  }

  // 2) Rewrite url(...) inside <style>...</style> blocks relative to the HTML file.
  const styleBlockRe = /(<style\b[^>]*>)([\s\S]*?)(<\/style>)/gi;
  const styleBlocks: Array<{ start: number; end: number; open: string; body: string; close: string }> = [];
  while ((m = styleBlockRe.exec(out)) !== null) {
    styleBlocks.push({
      start: m.index,
      end: m.index + m[0].length,
      open: m[1],
      body: m[2],
      close: m[3],
    });
  }
  if (styleBlocks.length) {
    const repls = await Promise.all(
      styleBlocks.map(async (b) => {
        const body = await rewriteCssUrls(b.body, basePath, ctx, 0);
        // rewriteCssUrls already gates every resolved url via ctx.allowed.
        return body === b.body ? null : { ...b, text: b.open + body + b.close };
      }),
    );
    for (let i = repls.length - 1; i >= 0; i--) {
      const r = repls[i];
      if (!r) continue;
      out = out.slice(0, r.start) + r.text + out.slice(r.end);
    }
  }

  // 3) url(...) inside style="..." attributes.
  const styleAttrRe = /(<[^>]+\sstyle\s*=\s*)(?:"([^"]*)"|'([^']*)'|([^\s>]+))/gi;
  const styleAttrs: Array<{ start: number; end: number; prefix: string; body: string; quote: string }> = [];
  while ((m = styleAttrRe.exec(out)) !== null) {
    const body = m[2] ?? m[3] ?? m[4] ?? "";
    const quote = m[2] != null ? '"' : m[3] != null ? "'" : '"';
    if (!/url\s*\(/i.test(body)) continue;
    styleAttrs.push({ start: m.index, end: m.index + m[0].length, prefix: m[1], body, quote });
  }
  if (styleAttrs.length) {
    const repls = await Promise.all(
      styleAttrs.map(async (a) => {
        const body = await rewriteCssUrls(a.body, basePath, ctx, 0);
        if (body === a.body) return null;
        const escaped = a.quote === '"' ? body.replace(/"/g, "&quot;") : body.replace(/'/g, "&#39;");
        return { ...a, text: `${a.prefix}${a.quote}${escaped}${a.quote}` };
      }),
    );
    for (let i = repls.length - 1; i >= 0; i--) {
      const r = repls[i];
      if (!r) continue;
      out = out.slice(0, r.start) + r.text + out.slice(r.end);
    }
  }

  // 4) Common element src/data/href attributes for local assets.
  // - <object data>, <embed src>, <input type=image src>
  // - <img>/<source>/<video>/<audio> src
  // - <use href/xlink:href> for SVG sprites
  const elRe = /<(img|source|video|audio|input|embed|object|use)\b[^>]*>/gi;
  const els: Array<{ start: number; end: number; tag: string; name: string }> = [];
  while ((m = elRe.exec(out)) !== null) {
    els.push({ start: m.index, end: m.index + m[0].length, tag: m[0], name: m[1].toLowerCase() });
  }
  if (els.length) {
    const repls = await Promise.all(
      els.map(async (e) => {
        let attr = e.name === "object" ? "data" : "src";
        if (e.name === "use") attr = getAttr(e.tag, "href") != null ? "href" : "xlink:href";
        const val = getAttr(e.tag, attr);
        if (!val || !isLocalRef(val)) return null;
        if (e.name === "input") {
          const type = (getAttr(e.tag, "type") || "").toLowerCase();
          if (type !== "image") return null;
        }
        const { path: refPath, suffix } = splitRef(val.trim());
        const abs = resolveRelativeAsset(basePath, refPath);
        if (!abs || !ctx.allowed(abs)) return null;
        const data = await readBinaryAsDataUrl(ctx, abs);
        if (!data) return null;
        return { ...e, text: setAttr(e.tag, attr, `${data}${suffix}`) };
      }),
    );
    for (let i = repls.length - 1; i >= 0; i--) {
      const r = repls[i];
      if (!r) continue;
      out = out.slice(0, r.start) + r.text + out.slice(r.end);
    }
  }

  return out;
}

export type InlineScope = {
  /** Folder tree the document may pull assets from; defaults to the doc folder. */
  boundary?: string;
  /** False for secret paths (e.g. .env); mirrors the AI read guard. */
  isReadable?: (absolutePath: string) => boolean;
};

export async function maybeInlineHtmlAssets(
  html: string,
  basePath: string | null | undefined,
  opts: { readText: ReadTextFile; readBinary: ReadBinaryFile },
  scope?: InlineScope,
): Promise<string> {
  if (!basePath) return html;
  try {
    return await inlineRelativeAssets(html, {
      basePath,
      ...opts,
      boundary: scope?.boundary,
      isReadable: scope?.isReadable,
    });
  } catch {
    // Preview should still render something rather than fail completely.
    return html;
  }
}
