import type { WorkspaceEnv } from "@/modules/workspace/env";
import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";

/** A fresh answer after this long, so a CLI installed meanwhile is seen. */
const FRESH_MS = 30_000;
const DEBOUNCE_MS = 400;

const cache = new Map<string, { at: number; found: Promise<boolean | null> }>();

/**
 * Whether the program a start command runs is on PATH in a workspace.
 * `null` when it cannot be told: a server over SSH, shell syntax, or a
 * failed check. Only a hint; an alias is invisible to the lookup on this
 * machine, so it never blocks a launch.
 */
export function checkAgentCommand(
  command: string,
  env: WorkspaceEnv,
): Promise<boolean | null> {
  if (env.kind === "ssh" || !command.trim()) return Promise.resolve(null);
  const distro = env.kind === "wsl" ? env.distro : null;
  const key = `${distro ?? ""}\u0000${command}`;
  const hit = cache.get(key);
  if (hit && Date.now() - hit.at < FRESH_MS) return hit.found;
  const found = invoke<boolean | null>("agent_command_available", {
    command,
    distro,
  })
    .then((v) => (typeof v === "boolean" ? v : null))
    .catch(() => null);
  cache.set(key, { at: Date.now(), found });
  return found;
}

/** `false` only once a check says the command's program is missing. */
export function useAgentCommandFound(
  command: string,
  env: WorkspaceEnv,
): boolean | null {
  const [found, setFound] = useState<boolean | null>(null);
  const distro = env.kind === "wsl" ? env.distro : null;
  const kind = env.kind;
  useEffect(() => {
    setFound(null);
    let live = true;
    const workspace: WorkspaceEnv =
      kind === "wsl" && distro ? { kind: "wsl", distro } : { kind: "local" };
    if (kind === "ssh") return;
    const timer = setTimeout(() => {
      void checkAgentCommand(command, workspace).then((v) => {
        if (live) setFound(v);
      });
    }, DEBOUNCE_MS);
    return () => {
      live = false;
      clearTimeout(timer);
    };
  }, [command, kind, distro]);
  return found;
}
