export const SPACE_WINDOW_PREFIX = "space-";

export function spaceWindowLabel(spaceId: string): string {
  return `${SPACE_WINDOW_PREFIX}${spaceId}`;
}

/** Inverse of {@link spaceWindowLabel}; null for the "main" window. */
export function spaceIdFromWindowLabel(label: string): string | null {
  if (!label.startsWith(SPACE_WINDOW_PREFIX)) return null;
  const id = label.slice(SPACE_WINDOW_PREFIX.length);
  return id.length > 0 ? id : null;
}

/** Browser sessions of the full app use `remote-<device id>` windows. */
export function isRemoteWindowLabel(label: string): boolean {
  return label.startsWith("remote-");
}

/**
 * Windows that can bring a space hosted elsewhere back to themselves: the
 * desktop's main window, and a browser session, which is the only window a
 * person sees on a headless host.
 */
export function isHomeWindow(label: string): boolean {
  return label === "main" || isRemoteWindowLabel(label);
}

/** True for the window dedicated to one space (as opposed to "main"). */
export function isSpaceWindow(label: string): boolean {
  return spaceIdFromWindowLabel(label) !== null;
}

/**
 * Spaces this window lists in its switcher. Main is the hub and lists every
 * space (including ones hosted elsewhere, to focus or bring them back); a
 * space window is focused on its own space and lists only that one.
 */
export function visibleSpacesForWindow<
  T extends { id: string },
>(spaces: T[], windowLabel: string): T[] {
  const own = spaceIdFromWindowLabel(windowLabel);
  if (own === null) return spaces;
  return spaces.filter((s) => s.id === own);
}

/**
 * Where tabs that cannot travel with `spaceId` stay in `windowLabel`: the
 * first other space this window still hosts, or null when there is none.
 */
export function pickHandoffFallback<
  T extends { id: string; hostWindowLabel?: string | null },
>(spaces: T[], spaceId: string, windowLabel: string): string | null {
  const other = spaces.find(
    (s) => s.id !== spaceId && (s.hostWindowLabel ?? "main") === windowLabel,
  );
  return other?.id ?? null;
}

export type HandoffPlan =
  | { kind: "focus"; windowLabel: string }
  | { kind: "handoff"; spaceId: string; from: string; to: string }
  | null;

export function planSpaceHandoff(
  space: { spaceId: string; hostWindowLabel: string | null | undefined },
  currentWindowLabel: string,
): HandoffPlan {
  const host = space.hostWindowLabel ?? "main";
  if (host !== currentWindowLabel) return { kind: "focus", windowLabel: host };
  if (currentWindowLabel !== "main") return null;
  return {
    kind: "handoff",
    spaceId: space.spaceId,
    from: currentWindowLabel,
    to: spaceWindowLabel(space.spaceId),
  };
}
