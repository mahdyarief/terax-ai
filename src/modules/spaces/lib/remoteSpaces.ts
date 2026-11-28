export { isHomeWindow, isRemoteWindowLabel } from "./handoff";
import type { SpaceMeta } from "./store";
import { selectHostedSpaces } from "./useSpaces";

export type RemoteSpacePlan =
  | { kind: "none" }
  | { kind: "reclaim"; spaceIds: string[] }
  | { kind: "create"; name: string };

/**
 * What a browser session does when it hosts no space yet, so New Tab never
 * lands in a space nothing shows.
 *
 * - Headless host (`--remote-serve`): nobody looks at the hidden main window,
 *   so its spaces come over with their tabs and live terminals.
 * - A desktop with a person at it: the browser makes a space of its own and
 *   leaves the desktop's tabs alone; they can still be brought over by hand.
 */
export function planRemoteSpaces(input: {
  spaces: SpaceMeta[];
  windowLabel: string;
  headless: boolean;
  deviceName: string | null;
}): RemoteSpacePlan {
  const { spaces, windowLabel, headless, deviceName } = input;
  if (selectHostedSpaces(spaces, windowLabel).length > 0) return { kind: "none" };
  if (headless) {
    const fromMain = spaces
      .filter((s) => (s.hostWindowLabel ?? "main") === "main")
      .map((s) => s.id);
    if (fromMain.length > 0) return { kind: "reclaim", spaceIds: fromMain };
  }
  return { kind: "create", name: deviceName ?? "Browser" };
}
