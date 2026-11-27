---
title: "CLI agents are profiles: built-ins in code, only the user's changes and custom agents stored"
author: Mahdy
date: 2026-11-27
branch: dydev
cost: { minutes: 26, tokens: 97097 }
id: l-573df52a
scope: repo
status: active
src: agent
touches:
  - src/modules/agents/lib/profiles.ts
  - src/modules/agents/lib/profiles.test.ts
  - src/settings/sections/CliAgentsSection.tsx
  - src/settings/sections/CliAgentsSection.test.tsx
  - src/modules/agents/components/AgentLauncherPanel.tsx
---

## Decided
- The launcher's agents are profiles. Built-ins are defined in code, and the `agents` preference stores only what the user changed (label, command, enabled) plus custom agents and an order. An agent added in a later release then shows up on its own, and a reset just drops the override.
- Ids are stable (`custom-<random>` for user agents) and never derived from labels, because tabs and settings refer to them.
- An agent is still a command typed into a new terminal, not an exec. That keeps aliases, PATH, nvm and WSL/SSH working. Commands only ever come from the user's settings, never from a file in a repository.
- Hooks are tied to an integration (claude, codex, gemini, pi), not to the agent id. A custom agent that wraps one of those CLIs (for example `ccr code`) picks that integration and gets its hooks.
- Antigravity (`agy`, no hooks) replaced Gemini as a built-in at the user's request. The older `agentLaunchCommands` map is carried over once: a changed Gemini command becomes a custom agent with the gemini integration.

## Watch out
- The terminal agents live in Settings > "CLI Agents" (tab id `cli-agents`). The "Agents" tab is the AI personas.
- No "not found on PATH" check yet. It is only meaningful for local workspaces, since on SSH the PATH is the server's.

In the code: src/modules/agents/lib/profiles.ts, src/settings/sections/CliAgentsSection.tsx, src/modules/agents/components/AgentLauncherPanel.tsx, settings store key `agents`.
