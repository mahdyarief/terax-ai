import { leafIds } from "@/modules/terminal/lib/panes";
import { tabAgentStatus } from "@/modules/terminal/lib/agentActivity";
import type { AgentPhase } from "@/modules/terminal/lib/agentActivity";
import type { Tab } from "@/modules/tabs";
import {
  workspaceEnvLabel,
  workspaceScopeKey,
  type WorkspaceEnv,
} from "@/modules/workspace/env";

/** One tab as the remote registry stores it (camelCase over IPC). */
export type RemoteTabPayload = {
  id: number;
  title: string;
  kind: string;
  spaceId?: string;
  workspace?: { key: string; label: string };
  mode?: "terminal" | "blocks";
  panes?: { leafId: number; ptyId: number | null; active: boolean }[];
  agent?: { kind: string | null; state: string } | null;
};

export type RemoteTabSources = {
  tabs: readonly Tab[];
  spaces: readonly { id: string; env: WorkspaceEnv }[];
  ptyIdForLeaf: (leafId: number) => number | null;
  phases: Record<number, AgentPhase>;
  agents: Record<number, string>;
};

/**
 * What the browser sees: every tab of every space this window hosts.
 * Non-terminal tabs are listed by id, title and kind only; their content is
 * out of scope for the web client.
 */
export function buildRemoteTabs(sources: RemoteTabSources): RemoteTabPayload[] {
  const { tabs, spaces, ptyIdForLeaf, phases, agents } = sources;
  const spaceEnv = new Map(spaces.map((space) => [space.id, space.env]));
  const payload: RemoteTabPayload[] = [];
  for (const tab of tabs) {
    if (tab.kind !== "terminal") {
      payload.push({ id: tab.id, title: tab.title, kind: tab.kind });
      continue;
    }
    const env = spaceEnv.get(tab.spaceId);
    const leaves = leafIds(tab.paneTree);
    const panes = leaves.map((leafId) => ({
      leafId,
      ptyId: ptyIdForLeaf(leafId),
      active: leafId === tab.activeLeafId,
    }));
    const ptyIds = panes.flatMap((pane) =>
      pane.ptyId === null ? [] : [pane.ptyId],
    );
    const status = tabAgentStatus(phases, agents, ptyIds);
    payload.push({
      id: tab.id,
      title: tab.title,
      kind: tab.kind,
      spaceId: tab.spaceId,
      workspace: env
        ? {
            key: workspaceScopeKey(env),
            label: workspaceEnvLabel(env, "Local"),
          }
        : undefined,
      mode: tab.blocks ? "blocks" : "terminal",
      panes,
      agent: status.state
        ? { kind: status.agent, state: status.state }
        : null,
    });
  }
  return payload;
}
