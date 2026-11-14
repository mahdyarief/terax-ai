export type PreviewNavigator = {
  /**
   * Opens an http(s) URL in an in-app preview tab. Returns false when the host
   * has no preview surface available, so the caller can fall back to the
   * system browser.
   */
  openUrl: (url: string) => boolean;
};

let current: PreviewNavigator | null = null;

export function setPreviewNavigator(nav: PreviewNavigator | null): void {
  current = nav;
}

export function getPreviewNavigator(): PreviewNavigator | null {
  return current;
}

/**
 * Sends a terminal link to an in-app preview tab when possible. Returns false
 * when no preview host is registered or the URL is not previewable, letting the
 * caller hand the address to the system browser instead.
 */
export function openUrlInPreview(url: string): boolean {
  const nav = current;
  if (!nav) return false;
  try {
    return nav.openUrl(url);
  } catch {
    return false;
  }
}
