/** What a failed save tells the user; a server's space names its host. */
export function saveFailure(error: unknown): string {
  const text = error instanceof Error ? error.message : String(error);
  const host = /^not connected to (.+)$/.exec(text)?.[1];
  return host
    ? `${host} is not connected. Your changes are kept; save again once it is back.`
    : text;
}
