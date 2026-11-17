import { beforeEach, describe, expect, it, vi } from "vitest";
import { emit } from "@tauri-apps/api/event";
import { HOST_CHANGED_REASONS, SPACE_HOST_CHANGED_EVENT } from "./crossWindowSync";
import type { SpaceMeta } from "./store";
import { useSpaces } from "./useSpaces";

vi.mock("@tauri-apps/api/event", () => ({
  emit: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("@/modules/settings/preferences", () => ({
  usePreferencesStore: {
    getState: () => ({ defaultWorkspaceEnv: { kind: "local" } }),
  },
}));

vi.mock("@/modules/workspace", () => ({
  parseWorkspaceScopeKey: vi.fn(() => ({ kind: "local" })),
}));

vi.mock("./store", () => ({
  deleteSpaceData: vi.fn().mockResolvedValue(undefined),
  newSpaceId: vi.fn(() => "new-id"),
  saveActiveId: vi.fn().mockResolvedValue(undefined),
  saveActiveIdForWindow: vi.fn().mockResolvedValue(undefined),
  saveSpacesList: vi.fn().mockResolvedValue(undefined),
}));

const space: SpaceMeta = {
  id: "a",
  name: "A",
  root: null,
  env: { kind: "local" },
  hostWindowLabel: "main",
  createdAt: 1,
  updatedAt: 1,
};

describe("assignSpaceToWindow", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useSpaces.setState({
      spaces: [{ ...space }],
      activeId: "a",
      windowLabel: "main",
      hydrated: true,
    });
  });

  it("broadcasts the new host so other windows can update their registry", async () => {
    await useSpaces.getState().assignSpaceToWindow("a", "space-a");

    expect(useSpaces.getState().spaces[0].hostWindowLabel).toBe("space-a");
    expect(emit).toHaveBeenCalledWith(
      SPACE_HOST_CHANGED_EVENT,
      expect.objectContaining({
        spaceId: "a",
        hostWindowLabel: "space-a",
      }),
    );
  });

  it("marks a live move as a handoff so receivers never restore from disk", async () => {
    await useSpaces.getState().assignSpaceToWindow("a", "space-a");

    expect(emit).toHaveBeenCalledWith(
      SPACE_HOST_CHANGED_EVENT,
      expect.objectContaining({ reason: HOST_CHANGED_REASONS.handoff }),
    );
  });

  it("forwards the window-closed reason from the Destroyed hook", async () => {
    await useSpaces.getState().assignSpaceToWindow(
      "a",
      "main",
      HOST_CHANGED_REASONS.windowClosed,
    );

    expect(emit).toHaveBeenCalledWith(
      SPACE_HOST_CHANGED_EVENT,
      expect.objectContaining({
        hostWindowLabel: "main",
        reason: HOST_CHANGED_REASONS.windowClosed,
      }),
    );
  });
});
