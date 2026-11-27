import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { cn } from "@/lib/utils";
import { AgentIcon } from "@/modules/agents/lib/agentIcon";
import { useAgentCommandFound } from "@/modules/agents/lib/commandAvailability";
import { validateAgentLaunchCommand } from "@/modules/agents/lib/launcher";
import {
  AGENT_ICON_KEYS,
  AGENT_INTEGRATIONS,
  type AgentIconKey,
  type AgentIntegration,
  type AgentProfile,
  type AgentSettings,
  addCustomAgent,
  builtinAgent,
  canAddCustomAgent,
  moveAgent,
  newCustomAgentId,
  removeCustomAgent,
  resetAgent,
  resolveAgentProfiles,
  updateAgent,
  validateAgentLabel,
} from "@/modules/agents/lib/profiles";
import { usePreferencesStore } from "@/modules/settings/preferences";
import { setAgentSettings } from "@/modules/settings/store";
import {
  LOCAL_WORKSPACE,
  parseWorkspaceScopeKey,
  workspaceEnvLabel,
} from "@/modules/workspace/env";
import {
  Add01Icon,
  ArrowDown01Icon,
  ArrowUp01Icon,
  Delete02Icon,
  Edit02Icon,
  Refresh01Icon,
} from "@hugeicons/core-free-icons";
import { HugeiconsIcon } from "@hugeicons/react";
import { invoke } from "@tauri-apps/api/core";
import { useEffect, useMemo, useState } from "react";
import { toast } from "sonner";
import { SectionHeader } from "../components/SectionHeader";

const NO_INTEGRATION = "none";

const save = (next: AgentSettings) =>
  void setAgentSettings(next).catch((e: unknown) =>
    toast.error("Could not save agents", { description: String(e) }),
  );

/**
 * The CLI agents the launcher offers. Each is a command Terax types into a
 * new terminal, so aliases, flags and the shell's PATH all apply.
 */
export function CliAgentsSection() {
  const settings = usePreferencesStore((s) => s.agents);
  const hydrated = usePreferencesStore((s) => s.hydrated);
  const agents = useMemo(() => resolveAgentProfiles(settings), [settings]);
  const [editing, setEditing] = useState<AgentProfile | null>(null);

  return (
    <div className="flex flex-col gap-7">
      <SectionHeader
        title="CLI Agents"
        description="Agents the launcher starts in a new terminal. Turn them on or off, change their commands, or add your own."
      />

      <section className="flex flex-col gap-2">
        <div className="flex items-center justify-between">
          <span className="text-[11px] font-medium tracking-tight text-muted-foreground">
            Agents
          </span>
          <Button
            size="sm"
            variant="outline"
            className="h-7 gap-1.5 px-2 text-[11px]"
            disabled={!hydrated || !canAddCustomAgent(settings)}
            onClick={() =>
              setEditing({
                id: newCustomAgentId(),
                label: "",
                command: "",
                enabled: true,
                integration: null,
                icon: "agent",
                workspaceCommands: {},
                builtin: false,
              })
            }
          >
            <HugeiconsIcon icon={Add01Icon} size={12} strokeWidth={1.75} />
            Add agent
          </Button>
        </div>
        <ul className="flex flex-col gap-1.5">
          {agents.map((agent, i) => (
            <AgentRow
              key={agent.id}
              agent={agent}
              first={i === 0}
              last={i === agents.length - 1}
              changed={
                agent.builtin && settings.overrides[agent.id] !== undefined
              }
              disabled={!hydrated}
              onToggle={(enabled) =>
                save(updateAgent(settings, agent.id, { enabled }))
              }
              onMove={(by) => save(moveAgent(settings, agent.id, by))}
              onEdit={() => setEditing(agent)}
              onReset={() => save(resetAgent(settings, agent.id))}
              onDelete={() => save(removeCustomAgent(settings, agent.id))}
            />
          ))}
        </ul>
        <p className="text-[10.5px] leading-relaxed text-muted-foreground">
          Commands run in your shell as if you typed them. Integration installs
          that CLI's hooks, so its turns raise notifications and show on the
          tab; pick it for a wrapper of that CLI, such as{" "}
          <code className="font-mono">ccr code</code> for Claude Code.
        </p>
      </section>

      <AgentDialog
        agent={editing}
        isNew={editing !== null && !agents.some((a) => a.id === editing.id)}
        onClose={() => setEditing(null)}
        onSave={(agent, isNew) => {
          if (isNew) {
            save(
              addCustomAgent(settings, {
                id: agent.id,
                label: agent.label,
                command: agent.command,
                enabled: true,
                integration: agent.integration,
                icon: agent.icon,
                workspaceCommands: agent.workspaceCommands,
              }),
            );
          } else {
            save(
              updateAgent(settings, agent.id, {
                label: agent.label,
                command: agent.command,
                workspaceCommands: agent.workspaceCommands,
                ...(agent.builtin
                  ? {}
                  : { integration: agent.integration, icon: agent.icon }),
              }),
            );
          }
          setEditing(null);
        }}
      />
    </div>
  );
}

function IconButton({
  label,
  icon,
  onClick,
  disabled,
  danger,
}: {
  label: string;
  icon: typeof Edit02Icon;
  onClick: () => void;
  disabled?: boolean;
  danger?: boolean;
}) {
  return (
    <Button
      size="icon"
      variant="ghost"
      className={cn(
        "size-7 text-muted-foreground",
        danger && "hover:text-destructive",
      )}
      onClick={onClick}
      disabled={disabled}
      title={label}
      aria-label={label}
    >
      <HugeiconsIcon icon={icon} size={12} strokeWidth={1.75} />
    </Button>
  );
}

function AgentRow({
  agent,
  first,
  last,
  changed,
  disabled,
  onToggle,
  onMove,
  onEdit,
  onReset,
  onDelete,
}: {
  agent: AgentProfile;
  first: boolean;
  last: boolean;
  changed: boolean;
  disabled: boolean;
  onToggle: (enabled: boolean) => void;
  onMove: (by: -1 | 1) => void;
  onEdit: () => void;
  onReset: () => void;
  onDelete: () => void;
}) {
  const integration = AGENT_INTEGRATIONS.find(
    (i) => i.id === agent.integration,
  );
  const found = useAgentCommandFound(agent.command, LOCAL_WORKSPACE);
  const workspaces = Object.keys(agent.workspaceCommands);
  return (
    <li
      className={cn(
        "flex items-center gap-2 rounded-lg border border-border/60 bg-card/60 px-3 py-2",
        !agent.enabled && "opacity-60",
      )}
    >
      <div className="flex size-7 shrink-0 items-center justify-center rounded-md bg-muted/40">
        <AgentIcon agent={agent.icon} size={14} />
      </div>
      <div className="flex min-w-0 flex-1 flex-col">
        <span className="flex items-center gap-1.5 text-[12px] font-medium">
          <span className="truncate">{agent.label}</span>
          {agent.builtin ? (
            <span className="rounded bg-muted/50 px-1 py-0.5 text-[9px] tracking-wide text-muted-foreground uppercase">
              Built-in
            </span>
          ) : null}
          {integration ? (
            <span
              className="rounded bg-primary/10 px-1 py-0.5 text-[9px] tracking-wide text-primary uppercase"
              title={`${integration.label} hooks: notifications and tab state`}
            >
              Hooks
            </span>
          ) : null}
        </span>
        <span className="flex min-w-0 items-center gap-1.5 text-[10.5px] text-muted-foreground">
          <code className="truncate font-mono">{agent.command}</code>
          {found === false ? (
            <span
              className="shrink-0 text-amber-600 dark:text-amber-400"
              title="Not on PATH on this computer. An alias, or a command used only in WSL or over SSH, still works."
            >
              not found
            </span>
          ) : null}
          {workspaces.length > 0 ? (
            <span
              className="shrink-0"
              title={workspaces.map((key) => workspaceName(key)).join(", ")}
            >
              +{workspaces.length}{" "}
              {workspaces.length === 1 ? "workspace" : "workspaces"}
            </span>
          ) : null}
        </span>
      </div>
      <IconButton
        label="Move up"
        icon={ArrowUp01Icon}
        onClick={() => onMove(-1)}
        disabled={disabled || first}
      />
      <IconButton
        label="Move down"
        icon={ArrowDown01Icon}
        onClick={() => onMove(1)}
        disabled={disabled || last}
      />
      <IconButton
        label="Edit"
        icon={Edit02Icon}
        onClick={onEdit}
        disabled={disabled}
      />
      {agent.builtin ? (
        <IconButton
          label={`Reset to ${builtinAgent(agent.id)?.command ?? "default"}`}
          icon={Refresh01Icon}
          onClick={onReset}
          disabled={disabled || !changed}
        />
      ) : (
        <IconButton
          label="Delete"
          icon={Delete02Icon}
          onClick={onDelete}
          disabled={disabled}
          danger
        />
      )}
      <Switch
        checked={agent.enabled}
        disabled={disabled}
        onCheckedChange={onToggle}
        aria-label={`Show ${agent.label} in the launcher`}
      />
    </li>
  );
}

function AgentDialog({
  agent,
  isNew,
  onClose,
  onSave,
}: {
  agent: AgentProfile | null;
  isNew: boolean;
  onClose: () => void;
  onSave: (agent: AgentProfile, isNew: boolean) => void;
}) {
  const [draft, setDraft] = useState<AgentProfile | null>(agent);
  useEffect(() => setDraft(agent), [agent]);
  if (!draft) return null;

  const label = validateAgentLabel(draft.label);
  const command = validateAgentLaunchCommand(draft.command);
  const workspaceCommands = Object.entries(draft.workspaceCommands).map(
    ([key, value]) => [key, validateAgentLaunchCommand(value)] as const,
  );
  const canSave =
    label.ok && command.ok && workspaceCommands.every(([, c]) => c.ok);
  // A built-in agent keeps its own icon and hooks.
  const fixed = draft.builtin;

  return (
    <Dialog open={!!agent} onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="max-w-lg">
        <DialogHeader>
          <DialogTitle className="text-[14px]">
            {isNew ? "Add agent" : `Edit ${agent?.label ?? "agent"}`}
          </DialogTitle>
        </DialogHeader>
        <form
          className="flex flex-col gap-3"
          onSubmit={(e) => {
            e.preventDefault();
            if (canSave)
              onSave(
                {
                  ...draft,
                  label: label.label,
                  command: command.command,
                  workspaceCommands: Object.fromEntries(
                    workspaceCommands.flatMap(([key, c]) =>
                      c.ok ? [[key, c.command]] : [],
                    ),
                  ),
                },
                isNew,
              );
          }}
        >
          {!fixed ? (
            <div className="flex flex-col gap-1">
              <FieldLabel>Icon</FieldLabel>
              <div className="flex flex-wrap gap-1">
                {AGENT_ICON_KEYS.map((key) => (
                  <button
                    key={key}
                    type="button"
                    title={key}
                    aria-pressed={draft.icon === key}
                    onClick={() =>
                      setDraft({ ...draft, icon: key as AgentIconKey })
                    }
                    className={cn(
                      "flex size-7 items-center justify-center rounded-md border transition-colors",
                      draft.icon === key
                        ? "border-foreground/40 bg-accent"
                        : "border-border/60 hover:bg-accent/40",
                    )}
                  >
                    <AgentIcon agent={key} size={13} />
                  </button>
                ))}
              </div>
            </div>
          ) : null}
          <div className="flex flex-col gap-1">
            <FieldLabel>Name</FieldLabel>
            <Input
              value={draft.label}
              onChange={(e) => setDraft({ ...draft, label: e.target.value })}
              placeholder="e.g. Aider"
              className="h-8 text-[12px]"
              autoFocus
            />
          </div>
          <div className="flex flex-col gap-1">
            <FieldLabel>Start command</FieldLabel>
            <Input
              value={draft.command}
              onChange={(e) => setDraft({ ...draft, command: e.target.value })}
              placeholder="e.g. aider --model sonnet"
              spellCheck={false}
              autoCapitalize="off"
              autoCorrect="off"
              aria-invalid={draft.command !== "" && !command.ok}
              className="h-8 font-mono text-[12px]"
            />
            <span
              className={cn(
                "text-[10.5px]",
                draft.command !== "" && !command.ok
                  ? "text-destructive"
                  : "text-muted-foreground",
              )}
            >
              {draft.command !== "" && !command.ok
                ? command.error
                : "Typed into a new terminal; aliases and flags work."}
            </span>
          </div>
          <WorkspaceCommandsField
            value={draft.workspaceCommands}
            fallback={draft.command}
            onChange={(next) => setDraft({ ...draft, workspaceCommands: next })}
          />
          {!fixed ? (
            <div className="flex flex-col gap-1">
              <FieldLabel>Integration</FieldLabel>
              <Select
                value={draft.integration ?? NO_INTEGRATION}
                onValueChange={(value) =>
                  setDraft({
                    ...draft,
                    integration:
                      value === NO_INTEGRATION
                        ? null
                        : (value as AgentIntegration),
                  })
                }
              >
                <SelectTrigger className="h-8 w-full text-[12px]">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value={NO_INTEGRATION}>None</SelectItem>
                  {AGENT_INTEGRATIONS.map((i) => (
                    <SelectItem key={i.id} value={i.id}>
                      Works like {i.label}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              <span className="text-[10.5px] text-muted-foreground">
                Installs that CLI's hooks before launching, for notifications
                and tab state.
              </span>
            </div>
          ) : null}
          <DialogFooter>
            <Button type="button" variant="ghost" size="sm" onClick={onClose}>
              Cancel
            </Button>
            <Button type="submit" size="sm" disabled={!canSave}>
              {isNew ? "Add" : "Save"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

const workspaceName = (key: string) =>
  workspaceEnvLabel(parseWorkspaceScopeKey(key), "This computer");

/** Workspaces the user can pick: this computer, WSL distros, SSH hosts. */
function useWorkspaceKeys(): string[] {
  const [keys, setKeys] = useState<string[]>(["local"]);
  useEffect(() => {
    let live = true;
    void Promise.all([
      invoke<{ name: string }[]>("wsl_list_distros")
        .then((d) => d.map((x) => `wsl:${x.name}`))
        .catch(() => [] as string[]),
      invoke<string[]>("remote_ssh_hosts")
        .then((h) => h.map((x) => `ssh:${x}`))
        .catch(() => [] as string[]),
    ]).then(([wsl, ssh]) => {
      if (live) setKeys(["local", ...wsl, ...ssh]);
    });
    return () => {
      live = false;
    };
  }, []);
  return keys;
}

function WorkspaceCommandsField({
  value,
  fallback,
  onChange,
}: {
  value: Record<string, string>;
  fallback: string;
  onChange: (next: Record<string, string>) => void;
}) {
  const known = useWorkspaceKeys();
  const free = known.filter((key) => !(key in value));
  const [pick, setPick] = useState("");
  const chosen = free.includes(pick) ? pick : (free[0] ?? "");
  const entries = Object.entries(value);

  return (
    <div className="flex flex-col gap-1">
      <FieldLabel>Per-workspace commands</FieldLabel>
      {entries.map(([key, command]) => {
        const check = validateAgentLaunchCommand(command);
        return (
          <div key={key} className="flex items-center gap-1.5">
            <span
              className="w-32 shrink-0 truncate text-[11px] text-muted-foreground"
              title={workspaceName(key)}
            >
              {workspaceName(key)}
            </span>
            <Input
              value={command}
              onChange={(e) => onChange({ ...value, [key]: e.target.value })}
              spellCheck={false}
              autoCapitalize="off"
              autoCorrect="off"
              aria-invalid={!check.ok}
              aria-label={`Command in ${workspaceName(key)}`}
              className="h-7 font-mono text-[11.5px]"
            />
            <IconButton
              label={`Use the start command in ${workspaceName(key)}`}
              icon={Delete02Icon}
              danger
              onClick={() => {
                const next = { ...value };
                delete next[key];
                onChange(next);
              }}
            />
          </div>
        );
      })}
      {free.length > 0 ? (
        <div className="flex items-center gap-1.5">
          <Select value={chosen} onValueChange={setPick}>
            <SelectTrigger className="h-7 w-48 text-[11.5px]">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {free.map((key) => (
                <SelectItem key={key} value={key}>
                  {workspaceName(key)}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          <Button
            type="button"
            size="sm"
            variant="outline"
            className="h-7 gap-1 px-2 text-[11px]"
            disabled={!chosen}
            onClick={() => onChange({ ...value, [chosen]: fallback })}
          >
            <HugeiconsIcon icon={Add01Icon} size={11} strokeWidth={1.75} />
            Add
          </Button>
        </div>
      ) : null}
      <span className="text-[10.5px] text-muted-foreground">
        A different command where the CLI is installed elsewhere, such as a
        server where it lives in <code className="font-mono">~/.local/bin</code>
        . Other workspaces use the start command.
      </span>
    </div>
  );
}

function FieldLabel({ children }: { children: React.ReactNode }) {
  return (
    <span className="text-[11px] font-medium tracking-tight text-muted-foreground">
      {children}
    </span>
  );
}
