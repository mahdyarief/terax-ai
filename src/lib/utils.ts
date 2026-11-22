import { clsx, type ClassValue } from "clsx"
import { twMerge } from "tailwind-merge"

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs))
}

export function isMarkdownPath(path: string): boolean {
  return /\.(md|markdown|mdx)$/i.test(path)
}

export function isHtmlPath(path: string): boolean {
  return /\.html?$/i.test(path)
}

export function isMermaidPath(path: string): boolean {
  return /\.(mmd|mermaid)$/i.test(path)
}

/** Text files the editor can show as source, preview, or both side by side. */
export function hasSourcePreview(path: string): boolean {
  return isMarkdownPath(path) || isMermaidPath(path)
}

/**
 * The source/preview layout an editor tab shows when it has not picked one.
 * A Markdown file opened in the editor was opened to be edited, so it starts
 * as source; a diagram file is opened to be looked at, so it starts rendered.
 */
export function defaultSourceView(path: string): "rendered" | "raw" {
  return isMermaidPath(path) ? "rendered" : "raw"
}
