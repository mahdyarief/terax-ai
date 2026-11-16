import { buildCommitHandoff, type PtyHandoffCheckpoint, type PtyReplayPayload } from "@/modules/terminal/lib/ptyHandoff";
import type { SerializedTab } from "./serialize";
import { SPACE_TABS_MOVED_EVENT } from "./crossWindowSync";

export type TabsMovedPayload = {
  spaceId: string;
  fromWindow: string;
  toWindow: string;
  handoffId: string;
  tabs: SerializedTab[];
  checkpoints: PtyHandoffCheckpoint[];
};

export type TabsAttachedPayload = {
  handoffId: string;
  replays: PtyReplayPayload[];
  error?: string;
};

export const SPACE_TABS_ATTACHED_EVENT = "spaces:tabs-attached";

class SourceUnavailableError extends Error {
  constructor() {
    super("source-unavailable");
    this.name = "SourceUnavailableError";
  }
}

type Invoke = (command: string, args?: Record<string, unknown>) => Promise<unknown>;

type Options = {
  handoffId: string;
  tabs: SerializedTab[];
  targetWindow: string;
  spaceId: string;
  fromWindow?: string;
  invoke: Invoke;
  emitTo: (window: string, event: string, payload: unknown) => Promise<void>;
  waitForAttach: (handoffId: string) => Promise<TabsAttachedPayload>;
};

export async function moveTabsAcrossWindows({
  handoffId,
  tabs,
  targetWindow,
  spaceId,
  fromWindow = "main",
  invoke,
  emitTo,
  waitForAttach,
}: Options): Promise<
  { ok: true; attached: TabsAttachedPayload } | { ok: false; error: string }
> {
  let emitted = false;
  let checkpoints: PtyHandoffCheckpoint[] = [];
  try {
    checkpoints = (await invoke("pty_prepare_handoff", { ids: [] })) as PtyHandoffCheckpoint[];
    await invoke("pty_detach", { handoffId });
    const payload: TabsMovedPayload = {
      spaceId,
      fromWindow,
      toWindow: targetWindow,
      handoffId,
      tabs,
      checkpoints,
    };
    await emitTo(targetWindow, SPACE_TABS_MOVED_EVENT, payload);
    emitted = true;
    const attached = await waitForAttach(handoffId);
    if (attached.error) throw new Error(attached.error);
    await invoke(
      "pty_commit_handoff",
      buildCommitHandoff(
        handoffId,
        attached.replays.map((replay) => ({
          ptyId: replay.ptyId,
          attachToken: replay.attachToken,
          leaseEpoch: replay.leaseEpoch,
        })),
      ),
    );
    return { ok: true, attached };
  } catch (error) {
    const reason = error instanceof Error ? error.message : String(error);
    if (emitted) {
      const correction: TabsMovedPayload = {
        spaceId,
        fromWindow,
        toWindow: targetWindow,
        handoffId,
        tabs: [],
        checkpoints: [],
      };
      await Promise.resolve(
        emitTo(targetWindow, SPACE_TABS_MOVED_EVENT, correction),
      ).catch(() => {});
    }
    try {
      await invoke("pty_rollback_handoff", { handoffId });
    } catch {
      // The source window is gone; nobody can resume these sessions, so close
      // them here rather than leaving orphaned native processes behind.
      await Promise.all(
        checkpoints.map((checkpoint) =>
          invoke("pty_close", { id: checkpoint.ptyId }).catch(() => {}),
        ),
      );
      return { ok: false, error: new SourceUnavailableError().message };
    }
    return { ok: false, error: reason };
  }
}
