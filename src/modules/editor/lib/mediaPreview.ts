export type MediaPreviewKind =
  | "image"
  | "video"
  | "audio"
  | "pdf"
  | "docx"
  | "xlsx";

const IMAGE_EXT = [
  "png",
  "jpg",
  "jpeg",
  "gif",
  "webp",
  "avif",
  "bmp",
  "svg",
  "ico",
];
const VIDEO_EXT = ["mp4", "webm", "ogg", "mov"];
const AUDIO_EXT = ["mp3", "wav", "flac", "aac", "m4a"];

export function mediaPreviewKind(path: string): MediaPreviewKind | null {
  const ext = path.split(".").pop()?.toLowerCase() ?? "";
  if (IMAGE_EXT.includes(ext)) return "image";
  if (VIDEO_EXT.includes(ext)) return "video";
  if (AUDIO_EXT.includes(ext)) return "audio";
  if (ext === "pdf") return "pdf";
  if (ext === "docx") return "docx";
  if (ext === "xlsx") return "xlsx";
  return null;
}

export function isSvgPath(path: string): boolean {
  return path.toLowerCase().endsWith(".svg");
}

/** Above this an SVG opens as source only; see EditorPane. */
export const SVG_RENDER_MAX_CHARS = 2 * 1024 * 1024;

/**
 * An SVG document as an <img> source. Loaded that way its scripts never run
 * and it cannot fetch anything, so previewing the live buffer is safe.
 */
export function svgDataUrl(svg: string): string {
  return `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`;
}

type MediaPlayback = Pick<
  HTMLMediaElement,
  "pause" | "addEventListener" | "removeEventListener"
>;

export function bindMediaPlayback(media: MediaPlayback, active: boolean) {
  const pause = () => media.pause();
  if (!active) {
    pause();
    media.addEventListener("play", pause);
  }
  return () => {
    media.removeEventListener("play", pause);
    pause();
  };
}
