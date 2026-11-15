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
    to: `space-${space.spaceId}`,
  };
}
