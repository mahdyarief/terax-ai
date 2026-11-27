import { validateAgentLaunchCommand } from "./launcher";

/**
 * CLIs whose hooks Terax can install, so their turns raise notifications and
 * show their state on the tab. A custom agent can name one when it wraps that
 * CLI (an alias, a router like `ccr code`).
 */
export const AGENT_INTEGRATIONS = [
  { id: "claude", label: "Claude Code" },
  { id: "codex", label: "Codex" },
  { id: "gemini", label: "Gemini CLI" },
  { id: "pi", label: "Pi" },
] as const;

export type AgentIntegration = (typeof AGENT_INTEGRATIONS)[number]["id"];

/** Icons an agent can show; each is a key `AgentIcon` understands. */
export const AGENT_ICON_KEYS = [
  "claude",
  "codex",
  "gemini",
  "antigravity",
  "pi",
  "opencode",
  "grok",
  "agent",
] as const;

export type AgentIconKey = (typeof AGENT_ICON_KEYS)[number];

/** A CLI agent the launcher can start: a command typed into a new terminal. */
export type AgentProfile = {
  /** Stable: tabs and settings refer to it, never to the label. */
  id: string;
  label: string;
  command: string;
  enabled: boolean;
  integration: AgentIntegration | null;
  icon: AgentIconKey;
  /**
   * Commands for particular workspaces, by workspace scope key (`local`,
   * `wsl:<distro>`, `ssh:<host>`): a server where the CLI sits elsewhere, or
   * a distro that needs other flags. Any other workspace uses `command`.
   */
  workspaceCommands: Record<string, string>;
  /** Shipped with Terax: it can be reset, but not deleted. */
  builtin: boolean;
};

type BuiltinAgent = Omit<
  AgentProfile,
  "enabled" | "builtin" | "workspaceCommands"
>;

export const BUILTIN_AGENTS: readonly BuiltinAgent[] = [
  {
    id: "claude",
    label: "Claude",
    command: "claude",
    integration: "claude",
    icon: "claude",
  },
  {
    id: "codex",
    label: "Codex",
    command: "codex",
    integration: "codex",
    icon: "codex",
  },
  {
    id: "antigravity",
    label: "Antigravity",
    command: "agy",
    integration: null,
    icon: "antigravity",
  },
  { id: "pi", label: "Pi", command: "pi", integration: "pi", icon: "pi" },
  {
    id: "opencode",
    label: "OpenCode",
    command: "opencode",
    integration: null,
    icon: "opencode",
  },
  {
    id: "grok",
    label: "Grok",
    command: "grok",
    integration: null,
    icon: "grok",
  },
];

/** What a user changed about a built-in agent. */
export type BuiltinOverride = {
  label?: string;
  command?: string;
  enabled?: boolean;
  workspaceCommands?: Record<string, string>;
};

export type CustomAgent = Omit<
  AgentProfile,
  "builtin" | "workspaceCommands"
> & {
  workspaceCommands?: Record<string, string>;
};

/**
 * The agents preference. Built-ins live in code and only the user's changes
 * are stored, so an agent added in a later release shows up on its own and a
 * reset just drops the override.
 */
export type AgentSettings = {
  overrides: Record<string, BuiltinOverride>;
  custom: CustomAgent[];
  /** Agent ids in the user's order; agents not listed follow in default order. */
  order: string[];
};

export const DEFAULT_AGENT_SETTINGS: AgentSettings = {
  overrides: {},
  custom: [],
  order: [],
};

const MAX_LABEL_LENGTH = 40;
const MAX_CUSTOM_AGENTS = 32;
const CUSTOM_ID = /^custom-[a-z0-9]{4,32}$/;

export function validateAgentLabel(
  value: unknown,
): { ok: true; label: string } | { ok: false; error: string } {
  if (typeof value !== "string" || !value.trim())
    return { ok: false, error: "Enter a name." };
  const label = value.trim();
  if (label.length > MAX_LABEL_LENGTH)
    return {
      ok: false,
      error: `Keep the name under ${MAX_LABEL_LENGTH} characters.`,
    };
  if (/[\u0000-\u001f\u007f]/.test(label))
    return { ok: false, error: "Use a single-line name." };
  return { ok: true, label };
}

export function newCustomAgentId(): string {
  const bytes = new Uint8Array(6);
  crypto.getRandomValues(bytes);
  return `custom-${Array.from(bytes, (b) => b.toString(36).padStart(2, "0")).join("")}`;
}

const isIntegration = (v: unknown): v is AgentIntegration =>
  AGENT_INTEGRATIONS.some((i) => i.id === v);
const isIconKey = (v: unknown): v is AgentIconKey =>
  (AGENT_ICON_KEYS as readonly unknown[]).includes(v);
const record = (v: unknown): Record<string, unknown> =>
  typeof v === "object" && v !== null && !Array.isArray(v)
    ? (v as Record<string, unknown>)
    : {};

const MAX_WORKSPACE_COMMANDS = 32;
const WORKSPACE_KEY =
  /^(local|wsl:[^\u0000-\u001f\u007f]{1,128}|ssh:[^\u0000-\u001f\u007f\s]{1,255})$/;

/** Keeps the well-formed workspace commands; `undefined` when none are left. */
function normalizeWorkspaceCommands(
  value: unknown,
): Record<string, string> | undefined {
  const out: Record<string, string> = {};
  for (const [key, raw] of Object.entries(record(value)).slice(
    0,
    MAX_WORKSPACE_COMMANDS,
  )) {
    const command = validateAgentLaunchCommand(raw);
    if (WORKSPACE_KEY.test(key) && command.ok) out[key] = command.command;
  }
  return Object.keys(out).length > 0 ? out : undefined;
}

/**
 * Reads the stored preference, dropping anything malformed. With nothing
 * stored yet, the older per-agent command map (`agentLaunchCommands`) is
 * carried over: a changed command becomes an override, and a changed Gemini
 * command, whose built-in Antigravity replaced, becomes a custom agent.
 */
export function normalizeAgentSettings(
  value: unknown,
  legacyCommands?: unknown,
): AgentSettings {
  if (value === undefined || value === null) return fromLegacy(legacyCommands);
  const stored = record(value);
  const overrides: Record<string, BuiltinOverride> = {};
  for (const [id, raw] of Object.entries(record(stored.overrides))) {
    if (!BUILTIN_AGENTS.some((b) => b.id === id)) continue;
    const o = record(raw);
    const next: BuiltinOverride = {};
    const label = validateAgentLabel(o.label);
    if (label.ok) next.label = label.label;
    const command = validateAgentLaunchCommand(o.command);
    if (command.ok) next.command = command.command;
    if (typeof o.enabled === "boolean") next.enabled = o.enabled;
    const workspaceCommands = normalizeWorkspaceCommands(o.workspaceCommands);
    if (workspaceCommands) next.workspaceCommands = workspaceCommands;
    if (Object.keys(next).length > 0) overrides[id] = next;
  }
  const custom: CustomAgent[] = [];
  const list = Array.isArray(stored.custom) ? stored.custom : [];
  for (const raw of list.slice(0, MAX_CUSTOM_AGENTS)) {
    const c = record(raw);
    const label = validateAgentLabel(c.label);
    const command = validateAgentLaunchCommand(c.command);
    if (
      typeof c.id !== "string" ||
      !CUSTOM_ID.test(c.id) ||
      custom.some((o) => o.id === c.id) ||
      !label.ok ||
      !command.ok
    )
      continue;
    const workspaceCommands = normalizeWorkspaceCommands(c.workspaceCommands);
    custom.push({
      id: c.id,
      label: label.label,
      command: command.command,
      enabled: c.enabled !== false,
      integration: isIntegration(c.integration) ? c.integration : null,
      icon: isIconKey(c.icon) ? c.icon : "agent",
      ...(workspaceCommands ? { workspaceCommands } : {}),
    });
  }
  const known = new Set([
    ...BUILTIN_AGENTS.map((b) => b.id),
    ...custom.map((c) => c.id),
  ]);
  const order = (Array.isArray(stored.order) ? stored.order : []).filter(
    (id, i, all): id is string =>
      typeof id === "string" && known.has(id) && all.indexOf(id) === i,
  );
  return { overrides, custom, order };
}

function fromLegacy(legacy: unknown): AgentSettings {
  const commands = record(legacy);
  const overrides: Record<string, BuiltinOverride> = {};
  for (const builtin of BUILTIN_AGENTS) {
    const command = validateAgentLaunchCommand(commands[builtin.id]);
    if (command.ok && command.command !== builtin.command)
      overrides[builtin.id] = { command: command.command };
  }
  const custom: CustomAgent[] = [];
  const gemini = validateAgentLaunchCommand(commands.gemini);
  if (gemini.ok && gemini.command !== "gemini") {
    custom.push({
      id: "custom-gemini0",
      label: "Gemini",
      command: gemini.command,
      enabled: true,
      integration: "gemini",
      icon: "gemini",
    });
  }
  return { overrides, custom, order: [] };
}

/** Every agent, built-in and custom, in the user's order; disabled ones too. */
export function resolveAgentProfiles(settings: AgentSettings): AgentProfile[] {
  const all: AgentProfile[] = [
    ...BUILTIN_AGENTS.map((b) => {
      const o = settings.overrides[b.id] ?? {};
      return {
        ...b,
        label: o.label ?? b.label,
        command: o.command ?? b.command,
        enabled: o.enabled ?? true,
        workspaceCommands: o.workspaceCommands ?? {},
        builtin: true,
      };
    }),
    ...settings.custom.map((c) => ({
      ...c,
      workspaceCommands: c.workspaceCommands ?? {},
      builtin: false,
    })),
  ];
  const rank = (id: string) => {
    const i = settings.order.indexOf(id);
    return i < 0
      ? settings.order.length + all.findIndex((a) => a.id === id)
      : i;
  };
  return all.sort((a, b) => rank(a.id) - rank(b.id));
}

export function enabledAgentProfiles(settings: AgentSettings): AgentProfile[] {
  return resolveAgentProfiles(settings).filter((a) => a.enabled);
}

export function builtinAgent(id: string): BuiltinAgent | undefined {
  return BUILTIN_AGENTS.find((b) => b.id === id);
}

/**
 * Applies a change to one agent and returns the new settings. For a built-in
 * agent, a field set back to its default is dropped from the override.
 */
export function updateAgent(
  settings: AgentSettings,
  id: string,
  change: Partial<
    Pick<
      AgentProfile,
      | "label"
      | "command"
      | "enabled"
      | "integration"
      | "icon"
      | "workspaceCommands"
    >
  >,
): AgentSettings {
  const builtin = builtinAgent(id);
  if (builtin) {
    const current: BuiltinOverride = { ...(settings.overrides[id] ?? {}) };
    if (change.label !== undefined) current.label = change.label;
    if (change.command !== undefined) current.command = change.command;
    if (change.enabled !== undefined) current.enabled = change.enabled;
    if (change.workspaceCommands !== undefined)
      current.workspaceCommands = change.workspaceCommands;
    if (current.label === builtin.label) delete current.label;
    if (current.command === builtin.command) delete current.command;
    if (current.enabled === true) delete current.enabled;
    if (
      current.workspaceCommands &&
      Object.keys(current.workspaceCommands).length === 0
    )
      delete current.workspaceCommands;
    const overrides = { ...settings.overrides };
    if (Object.keys(current).length > 0) overrides[id] = current;
    else delete overrides[id];
    return { ...settings, overrides };
  }
  return {
    ...settings,
    custom: settings.custom.map((c) => {
      if (c.id !== id) return c;
      const next = { ...c, ...change };
      if (
        next.workspaceCommands &&
        Object.keys(next.workspaceCommands).length === 0
      )
        delete next.workspaceCommands;
      return next;
    }),
  };
}

/** The command an agent starts with in a workspace, by its scope key. */
export function commandForWorkspace(
  agent: AgentProfile,
  scopeKey: string,
): string {
  return agent.workspaceCommands[scopeKey] ?? agent.command;
}

/**
 * Sets, or with `null` removes, the command an agent uses in one workspace.
 */
export function setWorkspaceCommand(
  settings: AgentSettings,
  agent: AgentProfile,
  scopeKey: string,
  command: string | null,
): AgentSettings {
  const workspaceCommands = { ...agent.workspaceCommands };
  if (command === null) delete workspaceCommands[scopeKey];
  else workspaceCommands[scopeKey] = command;
  return updateAgent(settings, agent.id, { workspaceCommands });
}

/** Drops a built-in agent's changes, so it is as Terax ships it. */
export function resetAgent(settings: AgentSettings, id: string): AgentSettings {
  const overrides = { ...settings.overrides };
  delete overrides[id];
  return { ...settings, overrides };
}

export function addCustomAgent(
  settings: AgentSettings,
  agent: CustomAgent,
): AgentSettings {
  return { ...settings, custom: [...settings.custom, agent] };
}

export function removeCustomAgent(
  settings: AgentSettings,
  id: string,
): AgentSettings {
  return {
    ...settings,
    custom: settings.custom.filter((c) => c.id !== id),
    order: settings.order.filter((o) => o !== id),
  };
}

/** Moves an agent one place up or down in the user's order. */
export function moveAgent(
  settings: AgentSettings,
  id: string,
  by: -1 | 1,
): AgentSettings {
  const ids = resolveAgentProfiles(settings).map((a) => a.id);
  const at = ids.indexOf(id);
  const to = at + by;
  if (at < 0 || to < 0 || to >= ids.length) return settings;
  [ids[at], ids[to]] = [ids[to], ids[at]];
  return { ...settings, order: ids };
}

export function canAddCustomAgent(settings: AgentSettings): boolean {
  return settings.custom.length < MAX_CUSTOM_AGENTS;
}
