import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { cn } from "@/lib/utils";
import { AgentIcon } from "@/modules/agents/lib/agentIcon";
import { useAgentCommandFound } from "@/modules/agents/lib/commandAvailability";
import {
  type AgentInstanceCount,
  type AgentLaunchRequest,
  validateAgentLaunchCommand,
} from "@/modules/agents/lib/launcher";
import {
  builtinAgent,
  commandForWorkspace,
  enabledAgentProfiles,
  setWorkspaceCommand,
  updateAgent,
} from "@/modules/agents/lib/profiles";
import { openSettingsWindow } from "@/modules/settings/openSettingsWindow";
import { usePreferencesStore } from "@/modules/settings/preferences";
import { setAgentSettings } from "@/modules/settings/store";
import {
  useWorkspaceEnvStore,
  workspaceEnvLabel,
  workspaceScopeKey,
} from "@/modules/workspace/env";
import {
  ArrowLeft01Icon,
  PlayIcon,
  Refresh01Icon,
  Settings01Icon,
} from "@hugeicons/core-free-icons";
import { HugeiconsIcon } from "@hugeicons/react";
import { useEffect, useMemo, useState } from "react";

type Props = {
  onBack: () => void;
  onLaunch: (request: AgentLaunchRequest) => void;
};

const INSTANCE_COUNTS: AgentInstanceCount[] = [1, 2, 3, 4];

export function AgentLauncherPanel({ onBack, onLaunch }: Props) {
  const settings = usePreferencesStore((s) => s.agents);
  const hydrated = usePreferencesStore((s) => s.hydrated);
  const agents = useMemo(() => enabledAgentProfiles(settings), [settings]);
  const [picked, setPicked] = useState<string | null>(null);
  const [instances, setInstances] = useState<AgentInstanceCount>(2);
  const agent = agents.find((a) => a.id === picked) ?? agents[0] ?? null;
  const env = useWorkspaceEnvStore((s) => s.env);
  const scope = workspaceScopeKey(env);
  const workspaceName = workspaceEnvLabel(env, "this computer");
  // A command set for this workspace wins over the agent's own.
  const scoped = agent ? agent.workspaceCommands[scope] !== undefined : false;
  const storedCommand = agent ? commandForWorkspace(agent, scope) : "";
  const [draft, setDraft] = useState(storedCommand);
  const command = draft;
  const validation = validateAgentLaunchCommand(command);
  const found = useAgentCommandFound(
    validation.ok ? validation.command : "",
    env,
  );
  const defaultCommand = agent ? builtinAgent(agent.id)?.command : undefined;

  // Show the stored command whenever another agent is picked, or it changes
  // in Settings.
  const agentId = agent?.id;
  // biome-ignore lint/correctness/useExhaustiveDependencies: reset the draft when another agent is picked.
  useEffect(() => {
    setDraft(storedCommand);
  }, [agentId, storedCommand]);

  const persist = (value: string): string | null => {
    if (!agent) return null;
    const result = validateAgentLaunchCommand(value);
    if (!result.ok) return null;
    if (result.command !== storedCommand) {
      void setAgentSettings(
        scoped
          ? setWorkspaceCommand(settings, agent, scope, result.command)
          : updateAgent(settings, agent.id, { command: result.command }),
      ).catch((error) => {
        console.error("[terax] failed to save the agent command:", error);
      });
    }
    return result.command;
  };

  const selectAgent = (id: string) => {
    persist(command);
    setPicked(id);
  };

  const save = (next: Parameters<typeof setAgentSettings>[0]) =>
    void setAgentSettings(next).catch((error) => {
      console.error("[terax] failed to save the agent command:", error);
    });

  const resetCommand = () => {
    if (!agent) return;
    // A workspace's own command goes first, back to the agent's.
    if (scoped) {
      save(setWorkspaceCommand(settings, agent, scope, null));
      return;
    }
    if (!defaultCommand) return;
    setDraft(defaultCommand);
    persist(defaultCommand);
  };

  const keepForWorkspace = () => {
    if (!agent || !validation.ok) return;
    save(setWorkspaceCommand(settings, agent, scope, validation.command));
  };

  const submit = () => {
    if (!agent) return;
    const next = persist(command);
    if (!next) return;
    onLaunch({
      agent: agent.id,
      label: agent.label,
      command: next,
      integration: agent.integration,
      instances,
    });
  };

  return (
    <form
      className="animate-in fade-in-0 slide-in-from-right-2 duration-150"
      onSubmit={(event) => {
        event.preventDefault();
        submit();
      }}
    >
      <div className="flex h-9 items-center gap-2 px-1">
        <Button
          type="button"
          variant="ghost"
          size="icon-xs"
          className="rounded-md text-muted-foreground"
          onClick={onBack}
          aria-label="Back to new tab menu"
        >
          <HugeiconsIcon icon={ArrowLeft01Icon} size={14} strokeWidth={1.75} />
        </Button>
        <div className="min-w-0">
          <div className="text-[13px] font-medium text-foreground">
            Launch agents
          </div>
          <div className="text-[10px] text-muted-foreground">
            One workspace, up to four panes
          </div>
        </div>
      </div>

      {agents.length === 0 ? (
        <div className="mt-1 border-t border-border/60 px-1 pt-3 text-xs text-muted-foreground">
          No agents are turned on. Turn one on, or add your own, in Settings.
        </div>
      ) : null}
      <div className="mt-1 grid grid-cols-2 gap-1 border-t border-border/60 pt-1.5">
        {agents.map((option) => {
          const selected = option.id === agent?.id;
          return (
            <button
              key={option.id}
              type="button"
              disabled={!hydrated}
              aria-pressed={selected}
              title={option.command}
              onClick={() => selectAgent(option.id)}
              className={cn(
                "flex min-w-0 items-center gap-2 rounded-xl border px-2.5 py-2 text-left transition-colors outline-none focus-visible:border-ring focus-visible:ring-2 focus-visible:ring-ring/25 disabled:pointer-events-none disabled:opacity-50",
                selected
                  ? "border-primary/35 bg-primary/10 text-foreground"
                  : "border-transparent bg-foreground/[0.035] text-muted-foreground hover:bg-accent hover:text-foreground",
              )}
            >
              <AgentIcon
                agent={option.icon}
                size={16}
                className={cn(
                  "shrink-0",
                  selected ? "text-primary" : "text-muted-foreground",
                )}
              />
              <span className="truncate text-xs font-medium">
                {option.label}
              </span>
            </button>
          );
        })}
      </div>

      <fieldset className="mt-3">
        <legend className="mb-1.5 text-[11px] font-medium text-muted-foreground">
          Instances
        </legend>
        <div className="grid grid-cols-4 gap-1">
          {INSTANCE_COUNTS.map((count) => (
            <button
              key={count}
              type="button"
              disabled={!hydrated}
              aria-label={`${count} ${count === 1 ? "instance" : "instances"}`}
              aria-pressed={instances === count}
              onClick={() => setInstances(count)}
              className={cn(
                "flex h-9 items-center justify-center gap-1.5 rounded-xl border text-xs font-medium transition-colors outline-none focus-visible:border-ring focus-visible:ring-2 focus-visible:ring-ring/25 disabled:pointer-events-none disabled:opacity-50",
                instances === count
                  ? "border-primary/35 bg-primary/10 text-foreground"
                  : "border-border/60 bg-background/30 text-muted-foreground hover:bg-accent hover:text-foreground",
              )}
            >
              <PaneGlyph count={count} />
              {count}
            </button>
          ))}
        </div>
      </fieldset>

      <div className="mt-3">
        <div className="mb-1.5 flex items-center">
          <label
            htmlFor="agent-start-command"
            className="text-[11px] font-medium text-muted-foreground"
          >
            Start command
          </label>
          <button
            type="button"
            onClick={resetCommand}
            disabled={
              !hydrated ||
              (!scoped && (!defaultCommand || command === defaultCommand))
            }
            className="ml-auto flex items-center gap-1 rounded-md px-1 text-[10px] text-muted-foreground transition-colors hover:text-foreground disabled:pointer-events-none disabled:opacity-40"
            title={
              scoped
                ? `Use ${agent?.command} here too`
                : defaultCommand
                  ? `Reset to ${defaultCommand}`
                  : undefined
            }
          >
            <HugeiconsIcon icon={Refresh01Icon} size={11} strokeWidth={1.75} />
            Reset
          </button>
        </div>
        <Input
          id="agent-start-command"
          disabled={!hydrated || !agent}
          value={command}
          spellCheck={false}
          autoCapitalize="off"
          autoCorrect="off"
          aria-invalid={!validation.ok}
          onChange={(event) => setDraft(event.target.value)}
          onBlur={() => persist(command)}
          className="h-8 rounded-xl bg-input/40 px-2.5 font-mono text-xs"
          placeholder={defaultCommand}
        />
        <div
          className={cn(
            "mt-1 min-h-4 text-[10px]",
            !validation.ok
              ? "text-destructive"
              : found === false
                ? "text-amber-600 dark:text-amber-400"
                : "text-muted-foreground",
          )}
        >
          {!validation.ok ? (
            validation.error
          ) : found === false ? (
            `Not found on PATH (${workspaceName}). An alias still works.`
          ) : scoped ? (
            `Set for ${workspaceName} only. Reset goes back to ${agent?.command}.`
          ) : (
            <>
              Aliases and flags are supported.{" "}
              <button
                type="button"
                disabled={!hydrated || !agent}
                onClick={keepForWorkspace}
                className="underline-offset-2 hover:text-foreground hover:underline"
              >
                Set for {workspaceName} only
              </button>
            </>
          )}
        </div>
      </div>

      <Button
        type="submit"
        size="sm"
        className="mt-1 w-full rounded-xl"
        disabled={!hydrated || !agent || !validation.ok}
      >
        <HugeiconsIcon icon={PlayIcon} size={13} strokeWidth={2} />
        Launch {instances} {instances === 1 ? "agent" : "agents"}
      </Button>
      <button
        type="button"
        onClick={() => void openSettingsWindow("cli-agents")}
        className="mt-1.5 flex w-full items-center justify-center gap-1 rounded-md py-1 text-[10px] text-muted-foreground transition-colors hover:text-foreground"
      >
        <HugeiconsIcon icon={Settings01Icon} size={11} strokeWidth={1.75} />
        Manage agents…
      </button>
    </form>
  );
}

function PaneGlyph({ count }: { count: AgentInstanceCount }) {
  const cell = "rounded-[1px] bg-current opacity-75";
  if (count === 1) {
    return (
      <span className="grid size-3 grid-cols-1 gap-px rounded-[2px] border border-current/50 p-px">
        <span className={cell} />
      </span>
    );
  }
  if (count === 2) {
    return (
      <span className="grid size-3 grid-cols-2 gap-px rounded-[2px] border border-current/50 p-px">
        <span className={cell} />
        <span className={cell} />
      </span>
    );
  }
  if (count === 3) {
    return (
      <span className="grid size-3 grid-cols-2 grid-rows-2 gap-px rounded-[2px] border border-current/50 p-px">
        <span className={cn(cell, "row-span-2")} />
        <span className={cell} />
        <span className={cell} />
      </span>
    );
  }
  return (
    <span className="grid size-3 grid-cols-2 grid-rows-2 gap-px rounded-[2px] border border-current/50 p-px">
      <span className={cell} />
      <span className={cell} />
      <span className={cell} />
      <span className={cell} />
    </span>
  );
}
