import { describe, expect, it } from "vitest";
import {
  addCustomAgent,
  BUILTIN_AGENTS,
  commandForWorkspace,
  DEFAULT_AGENT_SETTINGS,
  enabledAgentProfiles,
  moveAgent,
  normalizeAgentSettings,
  removeCustomAgent,
  resetAgent,
  resolveAgentProfiles,
  setWorkspaceCommand,
  updateAgent,
  type AgentSettings,
  type CustomAgent,
} from "./profiles";

const ids = (settings = DEFAULT_AGENT_SETTINGS) =>
  resolveAgentProfiles(settings).map((a) => a.id);

const router: CustomAgent = {
  id: "custom-router1",
  label: "Claude Router",
  command: "ccr code",
  enabled: true,
  integration: "claude",
  icon: "claude",
};

describe("built-in agents", () => {
  it("ship Antigravity (agy) in place of Gemini", () => {
    expect(ids()).toEqual([
      "claude",
      "codex",
      "antigravity",
      "pi",
      "opencode",
      "grok",
    ]);
    const agy = resolveAgentProfiles(DEFAULT_AGENT_SETTINGS)[2];
    expect(agy).toMatchObject({
      label: "Antigravity",
      command: "agy",
      integration: null,
    });
    expect(BUILTIN_AGENTS.some((b) => b.id === "gemini")).toBe(false);
  });
});

describe("normalizeAgentSettings", () => {
  it("drops malformed overrides, custom agents and order entries", () => {
    const settings = normalizeAgentSettings({
      overrides: {
        claude: { command: "cc", enabled: false, label: "" },
        nope: { command: "x" },
        codex: { command: "a\nb" },
      },
      custom: [
        router,
        { ...router, label: "dup" },
        { id: "bad id", label: "x", command: "x" },
        { id: "custom-zzzz1", label: "No command", command: " " },
        {
          id: "custom-zzzz2",
          label: "Odd",
          command: "odd",
          integration: "bogus",
          icon: "?",
        },
      ],
      order: ["grok", "grok", "missing", 3, "custom-router1"],
    });
    expect(settings.overrides).toEqual({
      claude: { command: "cc", enabled: false },
    });
    expect(settings.custom.map((c) => c.id)).toEqual([
      "custom-router1",
      "custom-zzzz2",
    ]);
    expect(settings.custom[1]).toMatchObject({
      integration: null,
      icon: "agent",
      enabled: true,
    });
    expect(settings.order).toEqual(["grok", "custom-router1"]);
  });

  it("carries over the older command map once", () => {
    const settings = normalizeAgentSettings(undefined, {
      claude: "cc --model opus",
      codex: "codex",
      gemini: "gemini --yolo",
    });
    expect(settings.overrides).toEqual({
      claude: { command: "cc --model opus" },
    });
    expect(settings.custom).toEqual([
      {
        id: "custom-gemini0",
        label: "Gemini",
        command: "gemini --yolo",
        enabled: true,
        integration: "gemini",
        icon: "gemini",
      },
    ]);
    // An unchanged Gemini command is replaced by Antigravity.
    expect(
      normalizeAgentSettings(undefined, { gemini: "gemini" }).custom,
    ).toEqual([]);
    // Stored settings win over the older map.
    expect(
      normalizeAgentSettings(DEFAULT_AGENT_SETTINGS, { claude: "cc" })
        .overrides,
    ).toEqual({});
  });
});

describe("editing agents", () => {
  it("stores only what differs from a built-in, and resets it", () => {
    let s = updateAgent(DEFAULT_AGENT_SETTINGS, "claude", {
      command: "cc",
      enabled: false,
    });
    expect(s.overrides.claude).toEqual({ command: "cc", enabled: false });
    s = updateAgent(s, "claude", { command: "claude", enabled: true });
    expect(s.overrides.claude).toBeUndefined();
    s = resetAgent(updateAgent(s, "codex", { label: "OpenAI" }), "codex");
    expect(s.overrides).toEqual({});
  });

  it("launches only enabled agents, in the user's order", () => {
    let s = addCustomAgent(DEFAULT_AGENT_SETTINGS, router);
    s = updateAgent(s, "grok", { enabled: false });
    s = moveAgent(s, "custom-router1", -1);
    expect(enabledAgentProfiles(s).map((a) => a.id)).toEqual([
      "claude",
      "codex",
      "antigravity",
      "pi",
      "opencode",
      "custom-router1",
    ]);
    // Disabled agents keep their place.
    expect(ids(s).slice(-2)).toEqual(["custom-router1", "grok"]);
    expect(moveAgent(s, "claude", -1)).toBe(s);
  });

  it("removes a custom agent and its place in the order", () => {
    const s = moveAgent(
      addCustomAgent(DEFAULT_AGENT_SETTINGS, router),
      "custom-router1",
      -1,
    );
    const removed = removeCustomAgent(s, "custom-router1");
    expect(removed.custom).toEqual([]);
    expect(removed.order).not.toContain("custom-router1");
  });
});

describe("workspace commands", () => {
  const profile = (s: AgentSettings, id: string) => {
    const found = resolveAgentProfiles(s).find((a) => a.id === id);
    if (!found) throw new Error(`no agent ${id}`);
    return found;
  };
  const claude = (s = DEFAULT_AGENT_SETTINGS) => profile(s, "claude");

  it("uses a workspace's own command, and the default elsewhere", () => {
    const s = setWorkspaceCommand(
      DEFAULT_AGENT_SETTINGS,
      claude(),
      "ssh:dypod",
      "~/.local/bin/claude",
    );
    expect(commandForWorkspace(claude(s), "ssh:dypod")).toBe(
      "~/.local/bin/claude",
    );
    expect(commandForWorkspace(claude(s), "local")).toBe("claude");
    // Removing the last one leaves the built-in unchanged.
    const cleared = setWorkspaceCommand(s, claude(s), "ssh:dypod", null);
    expect(cleared.overrides).toEqual({});
  });

  it("works for custom agents and survives a reload", () => {
    let s = addCustomAgent(DEFAULT_AGENT_SETTINGS, router);
    const agent = profile(s, router.id);
    s = setWorkspaceCommand(s, agent, "wsl:Ubuntu", "ccr code --wsl");
    const reloaded = normalizeAgentSettings(JSON.parse(JSON.stringify(s)));
    expect(reloaded.custom[0].workspaceCommands).toEqual({
      "wsl:Ubuntu": "ccr code --wsl",
    });
  });

  it("drops malformed workspace keys and commands", () => {
    const s = normalizeAgentSettings({
      overrides: {
        claude: {
          workspaceCommands: {
            local: "cc",
            "ssh:dy pod": "x",
            "nope:x": "x",
            "wsl:Ubuntu": "a\nb",
          },
        },
      },
    });
    expect(s.overrides.claude).toEqual({ workspaceCommands: { local: "cc" } });
  });
});
